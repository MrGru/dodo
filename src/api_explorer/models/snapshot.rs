//! A full, plain-data capture of one request.
//!
//! Where [`RequestDraft`](crate::api_explorer::models::request::RequestDraft) is
//! the subset the wire needs, a [`RequestSnapshot`] is everything an *editor*
//! needs to be restored exactly as it was: the scripts the draft drops, and a
//! name. It is what a saved collection entry and a history entry both store, so
//! reopening either rebuilds a request tab byte for byte.
//!
//! It is `Serialize`/`Deserialize` because the collection store writes it to
//! disk; nothing here touches GPUI, so it is unit testable on its own.

use serde::{Deserialize, Serialize};

use crate::api_explorer::models::auth::AuthDraft;
use crate::api_explorer::models::body::BodyDraft;
use crate::api_explorer::models::key_value::KeyValue;
use crate::api_explorer::models::method::HttpMethod;

/// Everything the request editor holds, captured as plain data.
///
/// New fields are added with `#[serde(default)]` so that a file written by an
/// older build still loads — a saved collection must survive the tool gaining a
/// field.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RequestSnapshot {
    pub method: HttpMethod,
    #[serde(default)]
    pub url: String,
    #[serde(default)]
    pub params: Vec<KeyValue>,
    #[serde(default)]
    pub headers: Vec<KeyValue>,
    #[serde(default)]
    pub body: BodyDraft,
    #[serde(default)]
    pub auth: AuthDraft,
    #[serde(default)]
    pub pre_request_script: String,
    #[serde(default)]
    pub post_response_script: String,
}

impl RequestSnapshot {
    /// A short one-line description of the request, used as a fallback name and
    /// in the history list: the method and the URL's path.
    ///
    /// Kept here rather than in a view so both the collections tree and the
    /// history list read it the same way, and so it is testable.
    pub fn summary(&self) -> String {
        let trimmed = self.url.trim();
        if trimmed.is_empty() {
            return self.method.as_str().to_string();
        }
        format!("{} {trimmed}", self.method.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::RequestSnapshot;
    use crate::api_explorer::models::method::HttpMethod;

    #[test]
    fn a_snapshot_round_trips_through_json() {
        let snapshot = RequestSnapshot {
            method: HttpMethod::Post,
            url: "https://example.com/things".into(),
            pre_request_script: "console.log('hi')".into(),
            ..RequestSnapshot::default()
        };
        let json = serde_json::to_string(&snapshot).expect("serializes");
        let back: RequestSnapshot = serde_json::from_str(&json).expect("deserializes");
        assert_eq!(back, snapshot);
    }

    #[test]
    fn an_older_file_without_new_fields_still_loads() {
        // A file written before scripts existed: the missing fields default.
        let json = r#"{"method":"Get","url":"https://example.com"}"#;
        let snapshot: RequestSnapshot = serde_json::from_str(json).expect("deserializes");
        assert_eq!(snapshot.method, HttpMethod::Get);
        assert_eq!(snapshot.url, "https://example.com");
        assert!(snapshot.pre_request_script.is_empty());
    }

    #[test]
    fn the_summary_is_the_method_and_url() {
        let snapshot = RequestSnapshot {
            method: HttpMethod::Delete,
            url: "  https://example.com/x  ".into(),
            ..RequestSnapshot::default()
        };
        assert_eq!(snapshot.summary(), "DELETE https://example.com/x");
    }

    #[test]
    fn a_blank_url_summarizes_as_the_method_alone() {
        let snapshot = RequestSnapshot::default();
        assert_eq!(snapshot.summary(), "GET");
    }
}
