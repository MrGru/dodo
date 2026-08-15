//! Reading an environment file the user picked into [`Environment`]s.
//!
//! Two shapes are understood, for the same reason
//! [`collection_import`](crate::services::collection_import)
//! understands two:
//!
//! - **A Postman environment export** — `{ "name", "values": [{ "key",
//!   "value", "enabled", "type" }] }`. Postman's *globals* export has the same
//!   `values` array under a different scope marker, so it imports too, as an
//!   environment named after the file's own `name`.
//! - **dodo's own environments file** — the `{ "version", "environments" }`
//!   document the store writes, so a backup of `environments.json` can be
//!   imported back. Its version is checked by the same
//!   [`parse_document`](crate::services::variable_store::parse_document)
//!   the store uses, rather than a second copy of that rule.
//!
//! Anything else is a reported error rather than a guess. The ids on returned
//! environments are placeholders; the state layer renumbers them on merge so
//! they cannot collide with what is already open.

use serde_json::Value;

use crate::i18n::{Str, api_variables};
use crate::models::variables::{Environment, Variable};
use crate::services::variable_store::{VariableStoreError, parse_document};

/// Why an environment import could not be read.
#[derive(Debug)]
pub enum EnvironmentImportError {
    /// The file is not JSON, or is JSON of no shape this understands.
    Unreadable { detail: String },
    /// It *is* dodo's own document and the store refused it — almost always a
    /// schema version this build does not read. Carried through rather than
    /// flattened to a string, so the user gets the store's precise wording
    /// ("update dodo") instead of a generic import failure.
    Store(VariableStoreError),
}

impl EnvironmentImportError {
    fn new(detail: impl Into<String>) -> Self {
        Self::Unreadable {
            detail: detail.into(),
        }
    }

    pub fn message(&self) -> Str {
        match self {
            EnvironmentImportError::Unreadable { detail } => {
                api_variables::Text::EnvironmentImportError(detail.clone()).into()
            }
            EnvironmentImportError::Store(error) => error.message(),
        }
    }
}

/// Parses a picked file into environments ready to merge.
pub fn parse_environment_import(bytes: &[u8]) -> Result<Vec<Environment>, EnvironmentImportError> {
    let value: Value = serde_json::from_slice(bytes)
        .map_err(|err| EnvironmentImportError::new(err.to_string()))?;

    let Value::Object(map) = &value else {
        return Err(EnvironmentImportError::new(
            "unrecognized environment format",
        ));
    };

    // dodo's own document comes first: it is the only shape carrying `version`,
    // and its version rule belongs to the store rather than to this module.
    if map.contains_key("version") && map.contains_key("environments") {
        let document = parse_document(bytes).map_err(EnvironmentImportError::Store)?;
        return Ok(document.environments);
    }

    if let Some(values) = map.get("values").and_then(Value::as_array) {
        let name = map
            .get("name")
            .and_then(Value::as_str)
            .unwrap_or("Imported environment")
            .to_string();
        return Ok(vec![Environment {
            id: 0,
            name,
            variables: values.iter().map(postman_variable).collect(),
        }]);
    }

    Err(EnvironmentImportError::new(
        "unrecognized environment format",
    ))
}

/// One entry of a Postman `values` array.
///
/// Postman marks a hidden value with `"type": "secret"`; every other type
/// (`default`, `text`, absent) is an ordinary value. `enabled` is absent on
/// older exports and means enabled.
pub fn postman_variable(value: &Value) -> Variable {
    Variable {
        key: value
            .get("key")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        value: value
            .get("value")
            .and_then(Value::as_str)
            .map(str::to_string)
            // A number or a boolean in a Postman file is still a value someone
            // typed; rendering it is better than dropping the row.
            .unwrap_or_else(|| match value.get("value") {
                Some(Value::Null) | None => String::new(),
                Some(other) => other.to_string(),
            }),
        enabled: value
            .get("enabled")
            .and_then(Value::as_bool)
            .unwrap_or(true),
        secret: value
            .get("type")
            .and_then(Value::as_str)
            .is_some_and(|kind| kind.eq_ignore_ascii_case("secret")),
    }
}

#[cfg(test)]
mod tests {
    use super::parse_environment_import;
    use crate::models::variables::SCHEMA_VERSION;

    #[test]
    fn a_postman_environment_imports_its_name_and_values() {
        let json = r#"{
            "id": "9d1b1e0a",
            "name": "Staging",
            "values": [
                {"key": "baseUrl", "value": "https://staging.example.com", "enabled": true, "type": "default"},
                {"key": "token", "value": "s3cr3t", "enabled": true, "type": "secret"},
                {"key": "legacy", "value": "x", "enabled": false, "type": "text"}
            ],
            "_postman_variable_scope": "environment"
        }"#;

        let environments = parse_environment_import(json.as_bytes()).expect("imports");
        assert_eq!(environments.len(), 1);
        let environment = &environments[0];
        assert_eq!(environment.name, "Staging");
        assert_eq!(environment.variables.len(), 3);

        assert_eq!(environment.variables[0].key, "baseUrl");
        assert_eq!(
            environment.variables[0].value,
            "https://staging.example.com"
        );
        assert!(environment.variables[0].enabled);
        assert!(!environment.variables[0].secret);

        assert!(
            environment.variables[1].secret,
            "Postman's secret type did not become dodo's secret flag"
        );
        assert_eq!(environment.variables[1].value, "s3cr3t");

        assert!(!environment.variables[2].enabled);
        assert!(!environment.variables[2].secret);
    }

    #[test]
    fn an_export_without_enabled_flags_imports_everything_switched_on() {
        let json = r#"{"name": "Old", "values": [{"key": "a", "value": "1"}]}"#;
        let environments = parse_environment_import(json.as_bytes()).expect("imports");
        assert!(environments[0].variables[0].enabled);
    }

    #[test]
    fn a_non_string_value_is_kept_rather_than_dropped() {
        let json = r#"{"name": "N", "values": [{"key": "port", "value": 8080}, {"key": "n", "value": null}]}"#;
        let environments = parse_environment_import(json.as_bytes()).expect("imports");
        assert_eq!(environments[0].variables[0].value, "8080");
        assert_eq!(environments[0].variables[1].value, "");
    }

    #[test]
    fn a_postman_globals_export_imports_as_an_environment() {
        let json = r#"{
            "name": "workspace globals",
            "values": [{"key": "g", "value": "1", "enabled": true}],
            "_postman_variable_scope": "globals"
        }"#;
        let environments = parse_environment_import(json.as_bytes()).expect("imports");
        assert_eq!(environments[0].name, "workspace globals");
        assert_eq!(environments[0].variables[0].key, "g");
    }

    #[test]
    fn an_unnamed_export_still_imports_under_a_fallback_name() {
        let json = r#"{"values": [{"key": "a", "value": "1"}]}"#;
        let environments = parse_environment_import(json.as_bytes()).expect("imports");
        assert!(!environments[0].name.is_empty());
    }

    #[test]
    fn dodos_own_environments_file_imports_back() {
        let json = format!(
            r#"{{"version":{SCHEMA_VERSION},"environments":[
                {{"id":4,"name":"Prod","variables":[
                    {{"key":"host","value":"example.com","enabled":true,"secret":false}},
                    {{"key":"key","value":"abc","enabled":true,"secret":true}}
                ]}}
            ],"collection_variables":[],"active_environment":4}}"#
        );
        let environments = parse_environment_import(json.as_bytes()).expect("imports");
        assert_eq!(environments.len(), 1);
        assert_eq!(environments[0].name, "Prod");
        assert!(environments[0].variables[1].secret);
    }

    #[test]
    fn a_dodo_file_from_a_newer_schema_is_refused_here_too() {
        let json = format!(r#"{{"version":{},"environments":[]}}"#, SCHEMA_VERSION + 5);
        assert!(parse_environment_import(json.as_bytes()).is_err());
    }

    #[test]
    fn an_unrecognized_shape_is_a_reported_error() {
        assert!(parse_environment_import(b"\"just a string\"").is_err());
        assert!(parse_environment_import(b"not json at all").is_err());
        assert!(parse_environment_import(br#"{"name":"no values here"}"#).is_err());
    }
}
