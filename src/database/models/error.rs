//! Why a database operation did not complete, in terms the UI can act on.
//!
//! Mirrors `docker::services::DockerError` and `api_explorer`'s
//! `TransportError` — named in prose rather than linked, because this module
//! reaches for nothing outside itself: the driver's own message is third-party
//! English kept
//! verbatim inside a translated frame, because there is nothing to translate it
//! with.
//!
//! # Why there are only two variants
//!
//! The design report proposes a third, `Cancelled`, so the UI can say
//! "cancelled" rather than "failed". Cancelling a running query is a later
//! round and nothing in this one can produce that value, so it is not declared
//! here — a variant no code constructs is a guess about a message nobody has
//! written yet. The driver choice that makes cancellation *possible* is already
//! made (see `Cargo.toml`); the variant lands with the button.

use crate::i18n::Str;

/// A database operation that failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DbError {
    /// Not reachable, or refused us: no listener, bad credentials, TLS refused,
    /// a SQLite file that cannot be opened. Drives a connection's Error state
    /// and its Reconnect.
    Unreachable(String),
    /// The server answered, and the answer was an error.
    ///
    /// `code` is the SQLSTATE (PostgreSQL) or the extended result code
    /// (SQLite) when the driver reports one, so the few cases worth
    /// special-casing later can be, without re-parsing English.
    Server {
        code: Option<String>,
        detail: String,
    },
}

impl DbError {
    /// A server error whose code the driver did not report.
    ///
    /// Test-only: a real driver always knows whether its backend gave it a
    /// code, and builds the variant directly.
    #[cfg(test)]
    pub fn server(detail: impl Into<String>) -> Self {
        Self::Server {
            code: None,
            detail: detail.into(),
        }
    }

    /// The message shown for this failure.
    pub fn message(&self) -> Str {
        match self {
            DbError::Unreachable(detail) => Str::DbUnreachable(detail.clone()),
            DbError::Server {
                code: Some(code),
                detail,
            } => Str::DbServerErrorCoded {
                code: code.clone(),
                detail: detail.clone(),
            },
            DbError::Server { code: None, detail } => Str::DbServerError(detail.clone()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::DbError;
    use crate::i18n::{Language, Str};

    #[test]
    fn every_error_keeps_the_drivers_own_words() {
        let detail = "connection refused";
        for error in [
            DbError::Unreachable(detail.into()),
            DbError::server(detail),
            DbError::Server {
                code: Some("42P01".into()),
                detail: detail.into(),
            },
        ] {
            for language in Language::ALL {
                let text = error.message().text(language).into_owned();
                assert!(
                    text.contains(detail),
                    "{} dropped the driver's own message: {text}",
                    language.code()
                );
            }
        }
    }

    #[test]
    fn a_code_the_driver_reported_reaches_the_message() {
        let error = DbError::Server {
            code: Some("42P01".into()),
            detail: "relation \"nope\" does not exist".into(),
        };
        for language in Language::ALL {
            assert!(
                error.message().text(language).contains("42P01"),
                "{} dropped the SQLSTATE",
                language.code()
            );
        }
    }

    #[test]
    fn a_missing_code_picks_the_uncoded_message() {
        assert!(matches!(
            DbError::server("boom").message(),
            Str::DbServerError(_)
        ));
    }
}
