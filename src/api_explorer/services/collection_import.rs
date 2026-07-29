//! Reading a collection file the user picked into [`Node`]s the tree can merge.
//!
//! Two shapes are understood: dodo's own saved format (a JSON array of nodes,
//! the same the store writes) and a Postman v2 collection (`{ "info", "item" }`).
//! Anything else is a reported error rather than a guess. Ids on the returned
//! nodes are placeholders — [`CollectionTree::import`] re-numbers them so they
//! cannot collide with the tree they are merged into.
//!
//! # A collection brings its variables with it
//!
//! A Postman collection may carry a top-level `variable` array, and every
//! `{{baseUrl}}` in its requests refers to it. Importing the requests without
//! it used to leave a tree in which nothing resolved, so [`parse_import`]
//! returns an [`Import`] — the nodes *and* those variables — and the page files
//! them into the collection scope. A folder-level `variable` array is not a
//! Postman concept; there is nowhere for one to come from.
//!
//! [`CollectionTree::import`]: crate::api_explorer::models::collection::CollectionTree::import

use serde_json::Value;

use crate::api_explorer::models::collection::{Node, NodeKind};
use crate::api_explorer::models::key_value::KeyValue;
use crate::api_explorer::models::method::HttpMethod;
use crate::api_explorer::models::snapshot::RequestSnapshot;
use crate::api_explorer::models::variables::Variable;
use crate::api_explorer::services::environment_import::postman_variable;
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

/// Everything one imported file contributes.
///
/// A struct rather than a bare `Vec<Node>` because a Postman collection carries
/// variables its requests depend on; see this module's doc. dodo's own saved
/// format is nodes only, so `variables` is empty for it.
#[derive(Debug, Default)]
pub struct Import {
    pub roots: Vec<Node>,
    pub variables: Vec<Variable>,
}

/// Parses a picked file into collections ready to merge into the tree.
pub fn parse_import(bytes: &[u8]) -> Result<Import, ImportError> {
    let value: Value =
        serde_json::from_slice(bytes).map_err(|err| ImportError::new(err.to_string()))?;

    match &value {
        // dodo's own format: an array of nodes.
        Value::Array(_) => serde_json::from_value::<Vec<Node>>(value)
            .map(|roots| Import {
                roots,
                variables: Vec::new(),
            })
            .map_err(|err| ImportError::new(err.to_string())),
        // Postman v2: an object carrying `info` and `item`.
        Value::Object(map) if map.contains_key("item") => Ok(Import {
            roots: vec![postman_collection(map)],
            variables: postman_variables(map),
        }),
        _ => Err(ImportError::new("unrecognized collection format")),
    }
}

/// A Postman collection's own `variable` array. Absent on most exports, which
/// is an empty list rather than an error.
fn postman_variables(map: &serde_json::Map<String, Value>) -> Vec<Variable> {
    map.get("variable")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .map(postman_variable)
                .filter(|variable| !variable.key.trim().is_empty())
                .collect()
        })
        .unwrap_or_default()
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
        ..KeyValue::text(
            header
                .get("key")
                .and_then(Value::as_str)
                .unwrap_or_default(),
            header
                .get("value")
                .and_then(Value::as_str)
                .unwrap_or_default(),
        )
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
        let imported = parse_import(json.as_bytes()).expect("imports");
        assert_eq!(imported.roots.len(), 1);
        assert_eq!(imported.roots[0].name, "APIs");
        assert_eq!(
            imported.roots[0].children[0]
                .snapshot()
                .map(|s| s.url.as_str()),
            Some("https://x/ping")
        );
        assert!(imported.variables.is_empty());
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
        let imported = parse_import(json.as_bytes()).expect("imports");
        assert_eq!(imported.roots.len(), 1);
        let collection = &imported.roots[0];
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
    fn a_collections_own_variable_array_is_imported_alongside_its_requests() {
        let json = r#"{
            "info": {"name": "My API"},
            "item": [{"name": "Health", "request": {"method": "GET", "url": "{{baseUrl}}/health"}}],
            "variable": [
                {"key": "baseUrl", "value": "https://api.example.com", "type": "string"},
                {"key": "apiKey", "value": "abc", "type": "secret"},
                {"key": "  ", "value": "dropped"}
            ]
        }"#;
        let imported = parse_import(json.as_bytes()).expect("imports");

        // The request survives with its reference intact, unsubstituted.
        let health = imported.roots[0].children[0].snapshot().expect("a request");
        assert_eq!(health.url, "{{baseUrl}}/health");

        // …and the variables it refers to came with it.
        assert_eq!(imported.variables.len(), 2, "an unnamed row was kept");
        assert_eq!(imported.variables[0].key, "baseUrl");
        assert_eq!(imported.variables[0].value, "https://api.example.com");
        assert!(imported.variables[0].enabled);
        assert!(!imported.variables[0].secret);
        assert!(imported.variables[1].secret);
    }

    #[test]
    fn a_collection_with_no_variable_array_imports_none() {
        let json = r#"{"info": {"name": "X"}, "item": []}"#;
        assert!(
            parse_import(json.as_bytes())
                .expect("imports")
                .variables
                .is_empty()
        );
    }

    #[test]
    fn an_unrecognized_shape_is_a_reported_error() {
        assert!(parse_import(b"\"just a string\"").is_err());
        assert!(parse_import(b"not json at all").is_err());
    }
}
