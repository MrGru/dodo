//! Reading a collection file the user picked into [`Node`]s the tree can merge.
//!
//! Two shapes are understood: dodo's own saved format (a JSON array of nodes,
//! the same the store writes) and a Postman v2 collection (`{ "info", "item" }`).
//! Anything else is a reported error rather than a guess. Ids on the returned
//! nodes are placeholders — [`CollectionTree::import`] re-numbers them so they
//! cannot collide with the tree they are merged into.
//!
//! [`CollectionTree::import`]: crate::api_explorer::models::collection::CollectionTree::import

use serde_json::Value;

use crate::api_explorer::models::collection::{Node, NodeKind};
use crate::api_explorer::models::key_value::KeyValue;
use crate::api_explorer::models::method::HttpMethod;
use crate::api_explorer::models::snapshot::RequestSnapshot;
use crate::i18n::Str;

/// Why an import could not be read.
#[derive(Debug)]
pub struct ImportError {
    detail: String,
}

impl ImportError {
    fn new(detail: impl Into<String>) -> Self {
        Self {
            detail: detail.into(),
        }
    }

    pub fn message(&self) -> Str {
        Str::CollectionImportError(self.detail.clone())
    }
}

/// Parses a picked file into collections ready to merge into the tree.
pub fn parse_import(bytes: &[u8]) -> Result<Vec<Node>, ImportError> {
    let value: Value =
        serde_json::from_slice(bytes).map_err(|err| ImportError::new(err.to_string()))?;

    match &value {
        // dodo's own format: an array of nodes.
        Value::Array(_) => serde_json::from_value::<Vec<Node>>(value)
            .map_err(|err| ImportError::new(err.to_string())),
        // Postman v2: an object carrying `info` and `item`.
        Value::Object(map) if map.contains_key("item") => Ok(vec![postman_collection(map)]),
        _ => Err(ImportError::new("unrecognized collection format")),
    }
}

/// Builds one collection node from a Postman v2 collection object.
fn postman_collection(map: &serde_json::Map<String, Value>) -> Node {
    let name = map
        .get("info")
        .and_then(|info| info.get("name"))
        .and_then(Value::as_str)
        .unwrap_or("Imported collection")
        .to_string();

    let children = map
        .get("item")
        .and_then(Value::as_array)
        .map(|items| items.iter().map(postman_item).collect())
        .unwrap_or_default();

    Node {
        id: 0,
        name,
        kind: NodeKind::Collection,
        children,
        expanded: true,
    }
}

/// A Postman item is a folder if it has a nested `item`, otherwise a request.
fn postman_item(item: &Value) -> Node {
    let name = item
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("Untitled")
        .to_string();

    if let Some(nested) = item.get("item").and_then(Value::as_array) {
        return Node {
            id: 0,
            name,
            kind: NodeKind::Folder,
            children: nested.iter().map(postman_item).collect(),
            expanded: true,
        };
    }

    let snapshot = item.get("request").map(postman_request).unwrap_or_default();

    Node {
        id: 0,
        name,
        kind: NodeKind::Request(Box::new(snapshot)),
        children: Vec::new(),
        expanded: true,
    }
}

/// Extracts method, URL and headers from a Postman request. Body and auth are
/// left at their defaults — the parts Postman and dodo agree on cleanly are
/// what get imported, rather than a lossy guess at the rest.
fn postman_request(request: &Value) -> RequestSnapshot {
    let method = request
        .get("method")
        .and_then(Value::as_str)
        .map(postman_method)
        .unwrap_or_default();

    let url = match request.get("url") {
        Some(Value::String(raw)) => raw.clone(),
        Some(Value::Object(url)) => url
            .get("raw")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        _ => String::new(),
    };

    let headers = request
        .get("header")
        .and_then(Value::as_array)
        .map(|headers| headers.iter().map(postman_header).collect())
        .unwrap_or_default();

    RequestSnapshot {
        method,
        url,
        headers,
        ..RequestSnapshot::default()
    }
}

fn postman_header(header: &Value) -> KeyValue {
    KeyValue {
        // Postman's `disabled` flag is the inverse of dodo's `enabled`.
        enabled: !header
            .get("disabled")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        key: header
            .get("key")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        value: header
            .get("value")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
    }
}

fn postman_method(method: &str) -> HttpMethod {
    HttpMethod::ALL
        .into_iter()
        .find(|candidate| candidate.as_str().eq_ignore_ascii_case(method))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::parse_import;
    use crate::api_explorer::models::collection::NodeKind;
    use crate::api_explorer::models::method::HttpMethod;

    #[test]
    fn dodos_own_saved_format_imports() {
        let json = r#"[
            {"id":3,"name":"APIs","kind":"Collection","children":[
                {"id":4,"name":"Ping","kind":{"Request":{"method":"Get","url":"https://x/ping"}},"children":[]}
            ]}
        ]"#;
        let roots = parse_import(json.as_bytes()).expect("imports");
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].name, "APIs");
        assert_eq!(
            roots[0].children[0].snapshot().map(|s| s.url.as_str()),
            Some("https://x/ping")
        );
    }

    #[test]
    fn a_postman_collection_imports_folders_and_requests() {
        let json = r#"{
            "info": {"name": "My API"},
            "item": [
                {"name": "Auth", "item": [
                    {"name": "Login", "request": {
                        "method": "POST",
                        "url": {"raw": "https://api.example.com/login"},
                        "header": [{"key": "Accept", "value": "application/json"}]
                    }}
                ]},
                {"name": "Health", "request": {"method": "GET", "url": "https://api.example.com/health"}}
            ]
        }"#;
        let roots = parse_import(json.as_bytes()).expect("imports");
        assert_eq!(roots.len(), 1);
        let collection = &roots[0];
        assert_eq!(collection.name, "My API");
        assert!(matches!(collection.kind, NodeKind::Collection));

        let folder = &collection.children[0];
        assert_eq!(folder.name, "Auth");
        assert!(matches!(folder.kind, NodeKind::Folder));

        let login = folder.children[0].snapshot().expect("a request");
        assert_eq!(login.method, HttpMethod::Post);
        assert_eq!(login.url, "https://api.example.com/login");
        assert_eq!(login.headers[0].key, "Accept");

        let health = collection.children[1].snapshot().expect("a request");
        assert_eq!(health.method, HttpMethod::Get);
        assert_eq!(health.url, "https://api.example.com/health");
    }

    #[test]
    fn an_unrecognized_shape_is_a_reported_error() {
        assert!(parse_import(b"\"just a string\"").is_err());
        assert!(parse_import(b"not json at all").is_err());
    }
}
