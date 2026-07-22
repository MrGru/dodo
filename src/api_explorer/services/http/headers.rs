//! The header list `prepare` builds, and the one rule everything that writes
//! to it obeys.
//!
//! # An explicit header always wins
//!
//! Two things want to write headers the user did not type: the Body tab's
//! `Content-Type` and the Auth tab's `Authorization`. Neither may overwrite a
//! row the user wrote in the Headers tab — a client that silently replaces
//! what you typed is a client you cannot debug with. So both go through
//! [`set_if_absent`], and the row in the table stays authoritative.

/// Header names written on the user's behalf. Not localized: these are wire
/// tokens.
pub const CONTENT_TYPE: &str = "Content-Type";
pub const AUTHORIZATION: &str = "Authorization";

/// Whether `headers` already carries `name`, matched the way HTTP matches
/// header names — case-insensitively.
pub fn contains(headers: &[(String, String)], name: &str) -> bool {
    headers
        .iter()
        .any(|(existing, _)| existing.eq_ignore_ascii_case(name))
}

/// Appends `name: value` unless the user already set `name`.
///
/// Returns whether it was written, which is only of interest to tests: callers
/// have nothing useful to do with the answer.
pub fn set_if_absent(headers: &mut Vec<(String, String)>, name: &str, value: String) -> bool {
    if contains(headers, name) {
        return false;
    }
    headers.push((name.to_string(), value));
    true
}

#[cfg(test)]
mod tests {
    use super::{CONTENT_TYPE, contains, set_if_absent};

    fn header(name: &str, value: &str) -> (String, String) {
        (name.to_string(), value.to_string())
    }

    #[test]
    fn header_names_match_case_insensitively() {
        let headers = [header("content-type", "text/plain")];
        assert!(contains(&headers, CONTENT_TYPE));
        assert!(contains(&headers, "CONTENT-TYPE"));
        assert!(!contains(&headers, "Accept"));
    }

    #[test]
    fn an_explicit_header_is_never_replaced() {
        let mut headers = vec![header("Content-Type", "application/vnd.api+json")];
        assert!(!set_if_absent(
            &mut headers,
            CONTENT_TYPE,
            "application/json".into()
        ));
        assert_eq!(
            headers,
            [header("Content-Type", "application/vnd.api+json")]
        );
    }

    #[test]
    fn a_missing_header_is_added_at_the_end() {
        let mut headers = vec![header("Accept", "*/*")];
        assert!(set_if_absent(
            &mut headers,
            CONTENT_TYPE,
            "application/json".into()
        ));
        assert_eq!(
            headers,
            [
                header("Accept", "*/*"),
                header(CONTENT_TYPE, "application/json")
            ]
        );
    }
}
