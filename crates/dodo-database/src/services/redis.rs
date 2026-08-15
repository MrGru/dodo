//! Redis/Valkey as a non-SQL driver.
//!
//! The tree is logical databases → Redis type groups → keys. Keys are read
//! with one cursor page of `SCAN ... TYPE` per expansion; a translated “More…”
//! branch carries the next cursor, so neither the driver nor the tree ever
//! holds the whole keyspace. Key values are fetched only when their detail is
//! opened. The editor runs one Redis command per non-empty line.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use redis::{ConnectionAddr, IntoConnectionInfo as _, RedisConnectionInfo};

use crate::models::catalog::{CatalogNode, GroupLabel, NodeId, NodeKind};
use crate::models::connection::ConnectionProfile;
use crate::models::detail::{DATA_PAGE_SIZE, DdlSource, DetailRequest, DetailTab};
use crate::models::error::DbError;
use crate::models::page::{Flow, RowSink};
use crate::models::query::{Execution, QueryRequest};
use crate::models::value::{ColumnMeta, Row, Value};
use crate::services::{Capabilities, DetailResult, Driver};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const DEFAULT_DATABASES: usize = 16;
const SCAN_COUNT: usize = 200;
const REDIS_TYPES: [&str; 6] = ["string", "hash", "list", "set", "zset", "stream"];

pub struct RedisDriver {
    /// The command console's session. Kept separate so browsing another
    /// logical database cannot silently change where the next command runs.
    connection: Mutex<redis::Connection>,
    catalog: Mutex<redis::Connection>,
    database: i64,
}

pub fn connect(profile: &ConnectionProfile) -> Result<Arc<RedisDriver>, DbError> {
    let database = profile
        .database
        .trim()
        .parse::<i64>()
        .map_err(|_| DbError::Unreachable("the logical database is not a number".into()))?;
    if database < 0 {
        return Err(DbError::Unreachable(
            "the logical database cannot be negative".into(),
        ));
    }

    let mut redis = RedisConnectionInfo::default().set_db(database);
    if !profile.user.trim().is_empty() {
        redis = redis.set_username(profile.user.trim());
    }
    if !profile.password.is_empty() {
        redis = redis.set_password(&profile.password);
    }
    let info = ConnectionAddr::Tcp(profile.host.trim().to_string(), profile.port)
        .into_connection_info()
        .map_err(unreachable)?
        .set_redis_settings(redis);
    let client = redis::Client::open(info).map_err(unreachable)?;
    let connection = client
        .get_connection_with_timeout(CONNECT_TIMEOUT)
        .map_err(unreachable)?;
    let catalog = client
        .get_connection_with_timeout(CONNECT_TIMEOUT)
        .map_err(unreachable)?;
    Ok(Arc::new(RedisDriver {
        connection: Mutex::new(connection),
        catalog: Mutex::new(catalog),
        database,
    }))
}

fn unreachable(error: redis::RedisError) -> DbError {
    DbError::Unreachable(error.to_string())
}

fn server_error(error: redis::RedisError) -> DbError {
    DbError::Server {
        code: error.code().map(str::to_string),
        detail: error.to_string(),
    }
}

mod id {
    pub const SEP: char = '\u{1f}';

    pub fn database(db: i64) -> String {
        format!("db{SEP}{db}")
    }
    pub fn type_group(db: i64, kind: &str) -> String {
        format!("ty{SEP}{db}{SEP}{kind}")
    }
    pub fn page(db: i64, kind: &str, cursor: u64) -> String {
        format!("pg{SEP}{db}{SEP}{kind}{SEP}{cursor}")
    }
    pub fn key(db: i64, kind: &str, page: u64, key: &[u8]) -> String {
        // SCAN may legally return the same key on two pages. Including the page
        // cursor keeps TreeItem ids globally unique without changing the key.
        format!("key{SEP}{db}{SEP}{kind}{SEP}{page}{SEP}{}", super::hex(key))
    }
    pub fn parse(id: &str) -> Option<(&str, Vec<&str>)> {
        let mut parts = id.split(SEP);
        Some((parts.next()?, parts.collect()))
    }
}

impl RedisDriver {
    fn with_connection<T>(
        &self,
        f: impl FnOnce(&mut redis::Connection) -> redis::RedisResult<T>,
    ) -> Result<T, DbError> {
        let mut connection = self
            .connection
            .lock()
            .map_err(|_| DbError::Unreachable("the connection was poisoned by a panic".into()))?;
        f(&mut connection).map_err(server_error)
    }

    fn with_catalog<T>(
        &self,
        f: impl FnOnce(&mut redis::Connection) -> redis::RedisResult<T>,
    ) -> Result<T, DbError> {
        let mut connection = self.catalog.lock().map_err(|_| {
            DbError::Unreachable("the catalog connection was poisoned by a panic".into())
        })?;
        f(&mut connection).map_err(server_error)
    }

    fn select(connection: &mut redis::Connection, database: i64) -> redis::RedisResult<()> {
        // `redis::Connection::get_db` is the database from its original
        // ConnectionInfo and is not updated by later SELECT commands, so a
        // guard around this would leave the catalog on whichever db was opened
        // last. SELECT is cheap and correctness wins.
        redis::cmd("SELECT").arg(database).query::<()>(connection)
    }

    fn roots(&self) -> Result<Vec<CatalogNode>, DbError> {
        self.with_catalog(|connection| {
            Self::select(connection, self.database)?;
            let info: String = redis::cmd("INFO").arg("keyspace").query(connection)?;
            let counts = keyspace_counts(&info);
            let configured = redis::cmd("CONFIG")
                .arg("GET")
                .arg("databases")
                .query::<Vec<String>>(connection)
                .ok()
                .and_then(|parts| parts.last()?.parse::<usize>().ok())
                .unwrap_or_else(|| {
                    counts
                        .keys()
                        .copied()
                        .max()
                        .map_or(DEFAULT_DATABASES, |highest| {
                            DEFAULT_DATABASES.max(highest as usize + 1)
                        })
                });
            Ok((0..configured)
                .map(|db| {
                    let count = counts.get(&(db as i64)).copied().unwrap_or(0);
                    CatalogNode::branch(
                        id::database(db as i64),
                        NodeKind::Namespace,
                        format!("db{db}"),
                    )
                    .with_detail(format!("{count} keys"))
                })
                .collect())
        })
    }

    fn type_groups(&self, database: i64) -> Vec<CatalogNode> {
        REDIS_TYPES
            .into_iter()
            .map(|kind| {
                CatalogNode::branch(id::type_group(database, kind), NodeKind::Namespace, kind)
            })
            .collect()
    }

    fn scan_keys(
        &self,
        database: i64,
        kind: &str,
        cursor: u64,
    ) -> Result<Vec<CatalogNode>, DbError> {
        self.with_catalog(|connection| {
            Self::select(connection, database)?;
            let (next, keys): (u64, Vec<Vec<u8>>) = redis::cmd("SCAN")
                .arg(cursor)
                .arg("COUNT")
                .arg(SCAN_COUNT)
                .arg("TYPE")
                .arg(kind)
                .query(connection)?;
            let mut nodes: Vec<CatalogNode> = keys
                .into_iter()
                .map(|key| {
                    CatalogNode::leaf(
                        id::key(database, kind, cursor, &key),
                        NodeKind::Key,
                        display_key(&key),
                    )
                    .with_detail(kind)
                })
                .collect();
            if next != 0 {
                nodes.push(CatalogNode::group(
                    id::page(database, kind, next),
                    GroupLabel::More,
                ));
            }
            Ok(nodes)
        })
    }

    fn key_detail(
        &self,
        database: i64,
        kind: &str,
        key: &[u8],
        offset: u64,
        sink: &mut dyn RowSink,
    ) -> Result<DetailResult, DbError> {
        self.with_catalog(|connection| {
            Self::select(connection, database)?;
            let (columns, rows, more) = match kind {
                "string" => {
                    let value: redis::Value = redis::cmd("GET").arg(key).query(connection)?;
                    (
                        vec![ColumnMeta::new("value", "string")],
                        vec![vec![redis_value(value)]],
                        false,
                    )
                }
                "list" => {
                    let length: u64 = redis::cmd("LLEN").arg(key).query(connection)?;
                    let values: Vec<Vec<u8>> = redis::cmd("LRANGE")
                        .arg(key)
                        .arg(offset)
                        .arg(offset + DATA_PAGE_SIZE)
                        .query(connection)?;
                    let rows = values
                        .into_iter()
                        .take(DATA_PAGE_SIZE as usize)
                        .enumerate()
                        .map(|(index, value)| {
                            vec![
                                Value::Int((offset + index as u64) as i64),
                                bytes_value(value),
                            ]
                        })
                        .collect();
                    (
                        vec![
                            ColumnMeta::new("index", "integer"),
                            ColumnMeta::new("value", "string"),
                        ],
                        rows,
                        offset + DATA_PAGE_SIZE < length,
                    )
                }
                "hash" => scan_detail(connection, "HSCAN", key, offset, 2, |parts| {
                    vec![bytes_value(parts[0].clone()), bytes_value(parts[1].clone())]
                })
                .map(|(rows, more)| {
                    (
                        vec![
                            ColumnMeta::new("field", "string"),
                            ColumnMeta::new("value", "string"),
                        ],
                        rows,
                        more,
                    )
                })?,
                "set" => scan_detail(connection, "SSCAN", key, offset, 1, |parts| {
                    vec![bytes_value(parts[0].clone())]
                })
                .map(|(rows, more)| (vec![ColumnMeta::new("member", "string")], rows, more))?,
                "zset" => scan_detail(connection, "ZSCAN", key, offset, 2, |parts| {
                    vec![bytes_value(parts[0].clone()), bytes_value(parts[1].clone())]
                })
                .map(|(rows, more)| {
                    (
                        vec![
                            ColumnMeta::new("member", "string"),
                            ColumnMeta::new("score", "double"),
                        ],
                        rows,
                        more,
                    )
                })?,
                "stream" => stream_detail(connection, key, offset)?,
                _ => return Ok(DetailResult::Unavailable),
            };
            Ok(feed(columns, rows, more, sink))
        })
    }
}

impl Driver for RedisDriver {
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            editor_language: "text",
            cancel: false,
            explain: false,
            detail: true,
            ddl: DdlSource::None,
            mutation: None,
        }
    }

    fn ping(&self) -> Result<(), DbError> {
        self.with_connection(|connection| redis::cmd("PING").query::<String>(connection))
            .map(|_| ())
    }

    fn statements(&self, buffer: &str) -> Vec<String> {
        buffer
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty() && !line.starts_with('#'))
            .map(str::to_string)
            .collect()
    }

    fn children(&self, parent: Option<&NodeId>) -> Result<Vec<CatalogNode>, DbError> {
        let Some(parent) = parent else {
            return self.roots();
        };
        let Some((tag, parts)) = id::parse(parent.as_str()) else {
            return Ok(Vec::new());
        };
        match (tag, parts.as_slice()) {
            ("db", [database]) => database
                .parse()
                .map(|database| self.type_groups(database))
                .map_err(|_| DbError::Server {
                    code: None,
                    detail: "invalid logical database node".into(),
                }),
            ("ty", [database, kind]) => database
                .parse()
                .map_err(|_| DbError::Server {
                    code: None,
                    detail: "invalid logical database node".into(),
                })
                .and_then(|database| self.scan_keys(database, kind, 0)),
            ("pg", [database, kind, cursor]) => {
                let database = database.parse().map_err(|_| DbError::Server {
                    code: None,
                    detail: "invalid logical database node".into(),
                })?;
                let cursor = cursor.parse().map_err(|_| DbError::Server {
                    code: None,
                    detail: "invalid scan cursor".into(),
                })?;
                self.scan_keys(database, kind, cursor)
            }
            _ => Ok(Vec::new()),
        }
    }

    fn detail(
        &self,
        request: &DetailRequest,
        sink: &mut dyn RowSink,
    ) -> Result<DetailResult, DbError> {
        if request.tab != DetailTab::Data || request.target.kind != NodeKind::Key {
            return Ok(DetailResult::Unavailable);
        }
        let Some(("key", parts)) = id::parse(request.target.node.as_str()) else {
            return Ok(DetailResult::Unavailable);
        };
        let [database, kind, _page, encoded] = parts.as_slice() else {
            return Ok(DetailResult::Unavailable);
        };
        let database = database.parse().map_err(|_| DbError::Server {
            code: None,
            detail: "invalid logical database node".into(),
        })?;
        let key = unhex(encoded).ok_or_else(|| DbError::Server {
            code: None,
            detail: "invalid key node".into(),
        })?;
        self.key_detail(database, kind, &key, request.offset, sink)
    }

    fn execute(
        &self,
        request: &QueryRequest,
        sink: &mut dyn RowSink,
    ) -> Result<Execution, DbError> {
        let started = Instant::now();
        let arguments = parse_command(&request.statement)
            .map_err(|detail| DbError::Server { code: None, detail })?;
        if arguments.is_empty() {
            return Err(DbError::Server {
                code: None,
                detail: "empty Redis command".into(),
            });
        }
        let reply = self.with_connection(|connection| {
            let mut command = redis::Cmd::new();
            for argument in &arguments {
                command.arg(argument);
            }
            command.query::<redis::Value>(connection)
        })?;
        let command = String::from_utf8_lossy(&arguments[0]).to_ascii_uppercase();
        let (columns, rows) = reply_table(&command, reply);
        sink.columns(columns);
        let mut truncated = false;
        for row in rows {
            if sink.row(row) == Flow::Stop {
                truncated = true;
                break;
            }
        }
        Ok(Execution {
            rows_affected: None,
            truncated,
            elapsed: started.elapsed(),
        })
    }
}

fn feed(
    columns: Vec<ColumnMeta>,
    rows: Vec<Row>,
    more: bool,
    sink: &mut dyn RowSink,
) -> DetailResult {
    sink.columns(columns);
    let mut truncated = more;
    for row in rows {
        if sink.row(row) == Flow::Stop {
            truncated = true;
            break;
        }
    }
    DetailResult::Rows {
        fields: None,
        truncated,
        notice: None,
    }
}

fn scan_detail(
    connection: &mut redis::Connection,
    command: &str,
    key: &[u8],
    offset: u64,
    width: usize,
    row: impl Fn(&[Vec<u8>]) -> Row,
) -> redis::RedisResult<(Vec<Row>, bool)> {
    let mut cursor = 0u64;
    let mut seen = 0u64;
    let mut rows = Vec::new();
    loop {
        let (next, values): (u64, Vec<Vec<u8>>) = redis::cmd(command)
            .arg(key)
            .arg(cursor)
            .arg("COUNT")
            .arg(SCAN_COUNT)
            .query(connection)?;
        for parts in values.chunks_exact(width) {
            if seen >= offset && rows.len() <= DATA_PAGE_SIZE as usize {
                rows.push(row(parts));
            }
            seen += 1;
            if rows.len() > DATA_PAGE_SIZE as usize {
                return Ok((rows, true));
            }
        }
        cursor = next;
        if cursor == 0 {
            return Ok((rows, false));
        }
    }
}

fn stream_detail(
    connection: &mut redis::Connection,
    key: &[u8],
    offset: u64,
) -> redis::RedisResult<(Vec<ColumnMeta>, Vec<Row>, bool)> {
    let mut start = "-".to_string();
    let mut seen = 0u64;
    let mut rows = Vec::new();
    loop {
        let reply: redis::Value = redis::cmd("XRANGE")
            .arg(key)
            .arg(&start)
            .arg("+")
            .arg("COUNT")
            .arg(SCAN_COUNT)
            .query(connection)?;
        let entries = stream_entries(reply);
        if entries.is_empty() {
            return Ok((stream_columns(), rows, false));
        }
        for (id, fields) in entries {
            start = format!("({id}");
            if seen >= offset && rows.len() <= DATA_PAGE_SIZE as usize {
                rows.push(vec![Value::Text(id), Value::Text(fields)]);
            }
            seen += 1;
            if rows.len() > DATA_PAGE_SIZE as usize {
                return Ok((stream_columns(), rows, true));
            }
        }
    }
}

fn stream_columns() -> Vec<ColumnMeta> {
    vec![
        ColumnMeta::new("id", "stream-id"),
        ColumnMeta::new("fields", "map"),
    ]
}

fn stream_entries(reply: redis::Value) -> Vec<(String, String)> {
    let redis::Value::Array(entries) = reply else {
        return Vec::new();
    };
    entries
        .into_iter()
        .filter_map(|entry| {
            let redis::Value::Array(mut parts) = entry else {
                return None;
            };
            if parts.len() != 2 {
                return None;
            }
            let fields = parts.pop()?;
            let id = redis_value(parts.pop()?).display();
            let fields = match fields {
                redis::Value::Array(values) => values
                    .chunks(2)
                    .map(|pair| {
                        pair.iter()
                            .cloned()
                            .map(redis_value)
                            .map(|value| value.display())
                            .collect::<Vec<_>>()
                            .join("=")
                    })
                    .collect::<Vec<_>>()
                    .join(", "),
                other => redis_value(other).display(),
            };
            Some((id, fields))
        })
        .collect()
}

fn reply_table(command: &str, reply: redis::Value) -> (Vec<ColumnMeta>, Vec<Row>) {
    match reply {
        redis::Value::Map(entries) => (
            vec![
                ColumnMeta::new("key", "string"),
                ColumnMeta::new("value", "reply"),
            ],
            entries
                .into_iter()
                .map(|(key, value)| vec![redis_value(key), redis_value(value)])
                .collect(),
        ),
        redis::Value::Array(values) if command == "HGETALL" => (
            vec![
                ColumnMeta::new("field", "string"),
                ColumnMeta::new("value", "string"),
            ],
            values
                .chunks(2)
                .map(|pair| pair.iter().cloned().map(redis_value).collect())
                .collect(),
        ),
        redis::Value::Array(values) => (
            vec![ColumnMeta::new("reply", "array")],
            values
                .into_iter()
                .map(|value| vec![redis_value(value)])
                .collect(),
        ),
        value => (
            vec![ColumnMeta::new("reply", redis_type(&value))],
            vec![vec![redis_value(value)]],
        ),
    }
}

fn redis_type(value: &redis::Value) -> &'static str {
    match value {
        redis::Value::Nil => "nil",
        redis::Value::Int(_) => "integer",
        redis::Value::BulkString(_) | redis::Value::SimpleString(_) | redis::Value::Okay => {
            "string"
        }
        redis::Value::Array(_) => "array",
        redis::Value::Map(_) => "map",
        redis::Value::Set(_) => "set",
        redis::Value::Double(_) => "double",
        redis::Value::Boolean(_) => "boolean",
        redis::Value::VerbatimString { .. } => "verbatim",
        redis::Value::BigNumber(_) => "big-number",
        redis::Value::Push { .. } => "push",
        redis::Value::Attribute { data, .. } => redis_type(data),
        redis::Value::ServerError(_) => "error",
        _ => "reply",
    }
}

fn redis_value(value: redis::Value) -> Value {
    match value {
        redis::Value::Nil => Value::Null,
        redis::Value::Int(value) => Value::Int(value),
        redis::Value::BulkString(bytes) => bytes_value(bytes),
        redis::Value::SimpleString(text) => Value::Text(text),
        redis::Value::Okay => Value::Text("OK".into()),
        redis::Value::Double(value) => Value::Float(value),
        redis::Value::Boolean(value) => Value::Bool(value),
        redis::Value::VerbatimString { text, .. } => Value::Text(text),
        redis::Value::Attribute { data, .. } => redis_value(*data),
        redis::Value::Array(values) | redis::Value::Set(values) => Value::Text(
            values
                .into_iter()
                .map(redis_value)
                .map(|value| value.display())
                .collect::<Vec<_>>()
                .join(", "),
        ),
        redis::Value::Map(entries) => Value::Text(
            entries
                .into_iter()
                .map(|(key, value)| {
                    format!(
                        "{}={}",
                        redis_value(key).display(),
                        redis_value(value).display()
                    )
                })
                .collect::<Vec<_>>()
                .join(", "),
        ),
        redis::Value::BigNumber(bytes) => bytes_value(bytes),
        redis::Value::Push { data, .. } => redis_value(redis::Value::Array(data)),
        redis::Value::ServerError(error) => Value::Text(error.to_string()),
        _ => Value::Text("<unsupported reply>".into()),
    }
}

fn bytes_value(bytes: Vec<u8>) -> Value {
    match String::from_utf8(bytes) {
        Ok(text) => Value::Text(text),
        Err(error) => Value::Bytes(error.into_bytes()),
    }
}

fn keyspace_counts(info: &str) -> HashMap<i64, u64> {
    info.lines()
        .filter_map(|line| {
            let (database, values) = line.trim().split_once(':')?;
            let database = database.strip_prefix("db")?.parse().ok()?;
            let keys = values
                .split(',')
                .find_map(|part| part.strip_prefix("keys="))?
                .parse()
                .ok()?;
            Some((database, keys))
        })
        .collect()
}

fn display_key(key: &[u8]) -> String {
    match std::str::from_utf8(key) {
        Ok(text) if !text.chars().any(char::is_control) => text.to_string(),
        _ => format!("0x{}", hex(key)),
    }
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(DIGITS[(byte >> 4) as usize] as char);
        output.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    output
}

fn unhex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    text.as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let high = (pair[0] as char).to_digit(16)?;
            let low = (pair[1] as char).to_digit(16)?;
            Some(((high << 4) | low) as u8)
        })
        .collect()
}

fn parse_command(command: &str) -> Result<Vec<Vec<u8>>, String> {
    let mut arguments = Vec::new();
    let mut current = Vec::new();
    let mut quote = None;
    let mut escaped = false;
    let mut started = false;

    for character in command.chars() {
        if escaped {
            let decoded = match character {
                'n' => '\n',
                'r' => '\r',
                't' => '\t',
                other => other,
            };
            let mut bytes = [0; 4];
            current.extend_from_slice(decoded.encode_utf8(&mut bytes).as_bytes());
            started = true;
            escaped = false;
            continue;
        }
        if character == '\\' {
            escaped = true;
            started = true;
            continue;
        }
        if let Some(delimiter) = quote {
            if character == delimiter {
                quote = None;
            } else {
                let mut bytes = [0; 4];
                current.extend_from_slice(character.encode_utf8(&mut bytes).as_bytes());
            }
            continue;
        }
        match character {
            '\'' | '"' => {
                quote = Some(character);
                started = true;
            }
            character if character.is_whitespace() => {
                if started {
                    arguments.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            character => {
                let mut bytes = [0; 4];
                current.extend_from_slice(character.encode_utf8(&mut bytes).as_bytes());
                started = true;
            }
        }
    }
    if escaped {
        return Err("a Redis command ends with an unfinished escape".into());
    }
    if quote.is_some() {
        return Err("a Redis command has an unclosed quote".into());
    }
    if started {
        arguments.push(current);
    }
    Ok(arguments)
}

#[cfg(test)]
mod tests {
    use super::{display_key, hex, keyspace_counts, parse_command, unhex};

    #[test]
    fn commands_keep_quoted_arguments_and_escapes() {
        assert_eq!(
            parse_command("SET greeting 'xin chào world'").expect("parses"),
            vec![
                b"SET".to_vec(),
                b"greeting".to_vec(),
                "xin chào world".as_bytes().to_vec()
            ]
        );
        assert_eq!(
            parse_command(r#"SET empty """#).expect("parses"),
            vec![b"SET".to_vec(), b"empty".to_vec(), Vec::new()]
        );
        assert!(parse_command("GET 'unfinished").is_err());
    }

    #[test]
    fn binary_keys_have_stable_round_trippable_labels() {
        let key = b"a\0\xff";
        assert_eq!(unhex(&hex(key)).as_deref(), Some(key.as_slice()));
        assert_eq!(display_key(key), "0x6100ff");
        assert_eq!(display_key(b"plain"), "plain");
    }

    #[test]
    fn info_keyspace_counts_only_database_rows() {
        let counts = keyspace_counts("# Keyspace\r\ndb0:keys=2,expires=1\r\ndb3:keys=9\r\n");
        assert_eq!(counts.get(&0), Some(&2));
        assert_eq!(counts.get(&3), Some(&9));
        assert_eq!(counts.len(), 2);
    }
}

/// Live coverage. It skips unless explicitly pointed at a throwaway Redis or
/// Valkey server; the configured logical database is flushed before and after.
#[cfg(test)]
mod live {
    use super::{RedisDriver, connect, id};
    use crate::models::catalog::{GroupLabel, NodeId, NodeKind, NodeLabel};
    use crate::models::connection::ConnectionProfile;
    use crate::models::detail::{DetailRequest, DetailTab, DetailTarget};
    use crate::models::engine::Engine;
    use crate::models::page::PageBuffer;
    use crate::models::query::QueryRequest;
    use crate::models::value::Value;
    use crate::services::Driver;
    use crate::state::detail::{DetailLoad, load as load_detail};
    use crate::state::query;
    use std::sync::Arc;

    fn profile() -> Option<ConnectionProfile> {
        let host = std::env::var("DODO_REDIS_TEST_HOST").ok()?;
        let mut profile = ConnectionProfile::new(1, Engine::Redis);
        profile.host = host;
        profile.port = std::env::var("DODO_REDIS_TEST_PORT")
            .ok()
            .and_then(|port| port.parse().ok())
            .unwrap_or(6379);
        profile.user = std::env::var("DODO_REDIS_TEST_USER").unwrap_or_default();
        profile.password = std::env::var("DODO_REDIS_TEST_PASSWORD").unwrap_or_default();
        profile.database = std::env::var("DODO_REDIS_TEST_DB").unwrap_or_else(|_| "15".into());
        Some(profile)
    }

    fn execute(driver: &RedisDriver, command: &str) -> PageBuffer {
        let mut sink = PageBuffer::default();
        driver
            .execute(&QueryRequest::new(command), &mut sink)
            .expect("Redis command runs");
        sink
    }

    struct Fixture {
        driver: Arc<RedisDriver>,
        database: i64,
    }

    impl Fixture {
        fn new() -> Option<Self> {
            let profile = profile()?;
            let database = profile.database.parse().expect("test database number");
            let driver = connect(&profile).expect("connects to configured Redis");
            execute(driver.as_ref(), "FLUSHDB");
            execute(driver.as_ref(), "SET greeting hello");
            execute(driver.as_ref(), "HSET person name Ada language Rust");
            execute(driver.as_ref(), "RPUSH queue first second third");
            for index in 0..1_000 {
                execute(driver.as_ref(), &format!("SET page:{index} {index}"));
            }
            Some(Self { driver, database })
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            execute(self.driver.as_ref(), "FLUSHDB");
        }
    }

    #[test]
    fn redis_console_cursor_tree_reply_grid_and_key_detail() {
        let Some(fixture) = Fixture::new() else {
            return;
        };
        assert!(fixture.driver.ping().is_ok());
        let outcome = query::run(
            fixture.driver.as_ref(),
            "PING\nGET greeting",
            crate::models::page::PageBudget::default(),
        )
        .expect("one command per line runs");
        assert_eq!(outcome.statements_run, 2);
        assert_eq!(outcome.grid.rows()[0][0], Value::Text("hello".into()));

        let roots = fixture.driver.children(None).expect("databases load");
        assert!(
            roots
                .iter()
                .any(|node| { node.label == NodeLabel::Name(format!("db{}", fixture.database)) })
        );
        let groups = fixture
            .driver
            .children(Some(&NodeId::new(id::database(fixture.database))))
            .expect("type groups load");
        assert!(
            groups
                .iter()
                .any(|node| node.label == NodeLabel::Name("hash".into()))
        );

        let first_page = fixture
            .driver
            .children(Some(&NodeId::new(id::type_group(
                fixture.database,
                "string",
            ))))
            .expect("SCAN page loads");
        assert!(
            first_page.iter().any(|node| {
                node.label == NodeLabel::Group(GroupLabel::More) && node.expandable
            })
        );
        assert!(
            first_page.len() < 1_001,
            "one expansion must not load the whole keyspace"
        );

        let key = b"person";
        let target = DetailTarget::new(
            NodeId::new(id::key(fixture.database, "hash", 0, key)),
            NodeKind::Key,
            "person",
        );
        let DetailLoad::Grid(detail) = load_detail(
            fixture.driver.as_ref(),
            &DetailRequest::new(target, DetailTab::Data, 0),
        ) else {
            panic!("hash detail did not load")
        };
        assert_eq!(detail.columns[0].name, "field");
        assert!(detail.grid.rows().iter().any(|row| {
            row[0] == Value::Text("name".into()) && row[1] == Value::Text("Ada".into())
        }));

        let hgetall = execute(fixture.driver.as_ref(), "HGETALL person");
        assert_eq!(hgetall.columns()[0].name, "field");
        assert_eq!(hgetall.rows().len(), 2);
    }
}
