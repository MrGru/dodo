//! Streaming result export.
//!
//! Export deliberately re-runs the statement with this file-backed [`RowSink`]
//! instead of serialising the bounded page on screen. The driver hands over one
//! row at a time and the sink keeps none, so exporting a million rows does not
//! turn into a million-row allocation. No `LIMIT` is injected.

use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use crate::models::error::DbError;
use crate::models::page::{Flow, RowSink};
use crate::models::query::QueryRequest;
use crate::models::value::{ColumnMeta, Row, Value};
use crate::services::Driver;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportFormat {
    Csv,
    Json,
}

impl ExportFormat {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Csv => "csv",
            Self::Json => "json",
        }
    }
}

#[derive(Debug)]
pub enum ExportError {
    Query(DbError),
    File(String),
}

impl ExportError {
    pub fn detail(&self) -> String {
        match self {
            Self::Query(error) => error.detail().to_string(),
            Self::File(detail) => detail.clone(),
        }
    }

    pub fn is_cancelled(&self) -> bool {
        matches!(self, Self::Query(DbError::Cancelled))
    }
}

/// Re-runs one statement and writes every row to `path`.
///
/// Blocking database and file IO: call only from the background executor.
pub fn export(
    driver: &dyn Driver,
    statement: &str,
    path: &Path,
    format: ExportFormat,
) -> Result<usize, ExportError> {
    let temporary = temporary_path(path);
    let file = File::create(&temporary).map_err(|error| file_error(&temporary, error))?;
    let mut sink = ExportSink::new(file, format);

    let result = driver
        .execute(&QueryRequest::new(statement), &mut sink)
        .map_err(ExportError::Query)
        .and_then(|_| sink.finish());

    let rows = match result {
        Ok(rows) => rows,
        Err(error) => {
            let _ = std::fs::remove_file(&temporary);
            return Err(error);
        }
    };

    // `rename` replaces atomically on Unix. Windows refuses an existing target,
    // so retry after removing it only once the complete temporary file exists.
    if let Err(first) = std::fs::rename(&temporary, path)
        && (!path.exists()
            || std::fs::remove_file(path).is_err()
            || std::fs::rename(&temporary, path).is_err())
    {
        let _ = std::fs::remove_file(&temporary);
        return Err(file_error(path, first));
    }

    Ok(rows)
}

fn temporary_path(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("export");
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    path.with_file_name(format!(".{name}.dodo-{}-{nonce}", std::process::id()))
}

fn file_error(path: &Path, error: std::io::Error) -> ExportError {
    ExportError::File(format!("{}: {error}", path.display()))
}

struct ExportSink {
    writer: BufWriter<File>,
    format: ExportFormat,
    columns: Vec<ColumnMeta>,
    started: bool,
    first_row: bool,
    rows: usize,
    error: Option<String>,
}

impl ExportSink {
    fn new(file: File, format: ExportFormat) -> Self {
        Self {
            writer: BufWriter::new(file),
            format,
            columns: Vec::new(),
            started: false,
            first_row: true,
            rows: 0,
            error: None,
        }
    }

    fn write_columns(&mut self) -> std::io::Result<()> {
        match self.format {
            ExportFormat::Csv => {
                for index in 0..self.columns.len() {
                    if index > 0 {
                        self.writer.write_all(b",")?;
                    }
                    write_csv_field(&mut self.writer, &self.columns[index].name)?;
                }
                self.writer.write_all(b"\n")
            }
            ExportFormat::Json => self.writer.write_all(b"["),
        }
    }

    fn write_row(&mut self, row: &Row) -> std::io::Result<()> {
        match self.format {
            ExportFormat::Csv => {
                for (index, value) in row.iter().enumerate() {
                    if index > 0 {
                        self.writer.write_all(b",")?;
                    }
                    write_csv_value(&mut self.writer, value)?;
                }
                self.writer.write_all(b"\n")
            }
            ExportFormat::Json => {
                if !self.first_row {
                    self.writer.write_all(b",")?;
                }
                self.writer.write_all(b"\n  {")?;
                for (index, column) in self.columns.iter().enumerate() {
                    if index > 0 {
                        self.writer.write_all(b",")?;
                    }
                    serde_json::to_writer(&mut self.writer, &column.name)
                        .map_err(std::io::Error::other)?;
                    self.writer.write_all(b":")?;
                    write_json_value(&mut self.writer, row.get(index).unwrap_or(&Value::Null))?;
                }
                self.writer.write_all(b"}")?;
                self.first_row = false;
                Ok(())
            }
        }
    }

    fn finish(mut self) -> Result<usize, ExportError> {
        if let Some(error) = self.error.take() {
            return Err(ExportError::File(error));
        }
        if self.format == ExportFormat::Json {
            if !self.started {
                self.writer
                    .write_all(b"[")
                    .map_err(|error| ExportError::File(error.to_string()))?;
            }
            self.writer
                .write_all(if self.first_row { b"]\n" } else { b"\n]\n" })
                .map_err(|error| ExportError::File(error.to_string()))?;
        }
        self.writer
            .flush()
            .map_err(|error| ExportError::File(error.to_string()))?;
        Ok(self.rows)
    }
}

impl RowSink for ExportSink {
    fn columns(&mut self, columns: Vec<ColumnMeta>) {
        self.columns = columns;
        self.started = true;
        if let Err(error) = self.write_columns() {
            self.error = Some(error.to_string());
        }
    }

    fn row(&mut self, row: Row) -> Flow {
        if self.error.is_some() {
            return Flow::Stop;
        }
        match self.write_row(&row) {
            Ok(()) => {
                self.rows += 1;
                Flow::Continue
            }
            Err(error) => {
                self.error = Some(error.to_string());
                Flow::Stop
            }
        }
    }
}

fn write_csv_value(writer: &mut impl Write, value: &Value) -> std::io::Result<()> {
    match value {
        Value::Null => Ok(()),
        Value::Text(text) | Value::Json(text) => write_csv_field(writer, text),
        Value::Bytes(bytes) => write_hex(writer, bytes),
        _ => write_csv_field(writer, &value.display()),
    }
}

fn write_csv_field(writer: &mut impl Write, value: &str) -> std::io::Result<()> {
    if !value
        .bytes()
        .any(|byte| matches!(byte, b',' | b'"' | b'\r' | b'\n'))
    {
        return writer.write_all(value.as_bytes());
    }
    writer.write_all(b"\"")?;
    let mut start = 0;
    for (index, _) in value.match_indices('"') {
        writer.write_all(&value.as_bytes()[start..index])?;
        writer.write_all(b"\"\"")?;
        start = index + 1;
    }
    writer.write_all(&value.as_bytes()[start..])?;
    writer.write_all(b"\"")
}

fn write_json_value(writer: &mut impl Write, value: &Value) -> std::io::Result<()> {
    match value {
        Value::Null => writer.write_all(b"null"),
        Value::Bool(value) => writer.write_all(value.to_string().as_bytes()),
        Value::Int(value) => writer.write_all(value.to_string().as_bytes()),
        Value::Float(value) if value.is_finite() => writer.write_all(value.to_string().as_bytes()),
        Value::Json(json) => writer.write_all(json.as_bytes()),
        Value::Bytes(bytes) => {
            writer.write_all(b"\"")?;
            write_hex(writer, bytes)?;
            writer.write_all(b"\"")
        }
        _ => serde_json::to_writer(writer, &value.display()).map_err(std::io::Error::other),
    }
}

fn write_hex(writer: &mut impl Write, bytes: &[u8]) -> std::io::Result<()> {
    writer.write_all(b"\\x")?;
    const HEX: &[u8; 16] = b"0123456789abcdef";
    for byte in bytes {
        writer.write_all(&[HEX[(byte >> 4) as usize], HEX[(byte & 0xf) as usize]])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{ExportFormat, export, write_csv_field, write_json_value};
    use crate::models::value::Value;
    use crate::services::fake::FakeDriver;
    use std::fs;

    fn path(extension: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("dodo-export-{}.{}", std::process::id(), extension))
    }

    #[test]
    fn csv_reruns_the_statement_and_writes_rows_beyond_the_screen_budget() {
        let path = path("csv");
        let driver = FakeDriver::sql().with_rows(1_500);
        let rows = export(
            &driver,
            "SELECT * FROM everything",
            &path,
            ExportFormat::Csv,
        )
        .expect("exports");
        let text = fs::read_to_string(&path).expect("reads");
        let _ = fs::remove_file(path);

        assert_eq!(rows, 1_500);
        assert_eq!(text.lines().count(), 1_501, "header plus every row");
        assert_eq!(
            driver.executed.lock().unwrap().as_slice(),
            ["SELECT * FROM everything"]
        );
        assert_eq!(*driver.offered.lock().unwrap(), 1_500);
    }

    #[test]
    fn csv_quotes_delimiters_newlines_and_quotes() {
        let mut bytes = Vec::new();
        write_csv_field(&mut bytes, "a,\"b\"\n").expect("writes");
        assert_eq!(String::from_utf8(bytes).unwrap(), "\"a,\"\"b\"\"\n\"");
    }

    #[test]
    fn json_keeps_database_json_as_json_rather_than_a_quoted_string() {
        let mut bytes = Vec::new();
        write_json_value(&mut bytes, &Value::Json("{\"ok\":true}".into())).expect("writes");
        assert_eq!(String::from_utf8(bytes).unwrap(), "{\"ok\":true}");
    }

    #[test]
    fn json_is_an_array_of_objects_and_replaces_an_existing_file() {
        let path = path("json");
        fs::write(&path, "old").expect("seed");
        let driver = FakeDriver::sql();
        export(&driver, "SELECT 1", &path, ExportFormat::Json).expect("exports");
        let value: serde_json::Value =
            serde_json::from_slice(&fs::read(&path).expect("reads")).expect("valid json");
        let _ = fs::remove_file(path);

        assert_eq!(value.as_array().map(Vec::len), Some(3));
        assert_eq!(value[0]["id"], 1);
        assert_eq!(value[0]["name"], "row-1");
    }
}
