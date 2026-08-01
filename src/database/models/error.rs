//! Why a database operation did not complete, in terms the UI can act on.
//!
//! Mirrors `docker::services::DockerError` and `api_explorer`'s
//! `TransportError` — named in prose rather than linked, because this module
//! reaches for nothing outside itself: the driver's own message is third-party
//! English kept
//! verbatim inside a translated frame, because there is nothing to translate it
//! with.
//!
//! # Why there are three variants
//!
//! Round 1 had two, and said the third — `Cancelled` — would land with the
//! button that produces it. Round 2 is that round. It is a variant of its own
//! rather than a `Server` error with a well-known code because the two mean
//! opposite things to the user: a server error says the statement was wrong, a
//! cancellation says they stopped it, and rendering the second as the first
//! would report the user's own click as a fault.
//!
//! **Only a driver constructs it, and only from the server's own answer.** The
//! PostgreSQL driver maps SQLSTATE `57014` (`query_canceled`) and the SQLite
//! driver maps `SQLITE_INTERRUPT`; neither invents it from the fact that
//! something was cancelled locally. That is what makes "the query really
//! stopped at the server" the thing the UI is reporting, rather than "the UI
//! stopped waiting".

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
    /// The server stopped the statement because dodo asked it to.
    ///
    /// Constructed **only** from the backend's own report of a cancellation —
    /// PostgreSQL's `57014`, SQLite's `SQLITE_INTERRUPT` — so it is evidence
    /// that the work stopped where the work was happening, not a note that the
    /// UI gave up waiting.
    Cancelled,
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
            DbError::Cancelled => Str::DbCancelledMessage,
        }
    }

    /// The driver's own words, for a message that needs them inside a frame of
    /// its own rather than the one [`message`](Self::message) picks. Empty for
    /// a cancellation, which is dodo's own doing and has no driver text.
    pub fn detail(&self) -> &str {
        match self {
            DbError::Unreachable(detail) | DbError::Server { detail, .. } => detail,
            DbError::Cancelled => "",
        }
    }

    /// Whether this is a cancellation the user asked for.
    ///
    /// Asked rather than matched at the call sites, so a future driver that
    /// reports cancellation some other way has one place to be taught about.
    pub fn is_cancelled(&self) -> bool {
        matches!(self, DbError::Cancelled)
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

    /// A cancellation is the user's own doing, so it must not read as a fault
    /// and must not be mistaken for one by the code either.
    #[test]
    fn a_cancellation_is_its_own_thing_and_says_so_in_both_languages() {
        let cancelled = DbError::Cancelled;
        assert!(cancelled.is_cancelled());
        assert!(!DbError::server("boom").is_cancelled());
        assert!(!DbError::Unreachable("no listener".into()).is_cancelled());

        for language in Language::ALL {
            let text = cancelled.message().text(language).into_owned();
            assert!(
                !text.trim().is_empty(),
                "{} has no wording",
                language.code()
            );
        }
    }
}
