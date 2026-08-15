//! What a driver is asked to run, and what it reports back.

use std::time::Duration;

/// One statement to execute.
///
/// The text is **exactly what the user typed**, and dodo does not rewrite it.
/// Object-detail paging is deliberately a different request
/// (`models::detail::DetailRequest`): its statement is generated wholly by the
/// backend from an opaque catalog id. Keeping offset out of this type is what
/// makes it impossible for table paging to leak into editor text. See
/// `models::page` for why bounding at the sink, rather than by rewriting, is
/// the rule for anything the user wrote.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QueryRequest {
    pub statement: String,
}

impl QueryRequest {
    pub fn new(statement: impl Into<String>) -> Self {
        Self {
            statement: statement.into(),
        }
    }
}

/// What running one statement produced, beside its rows.
///
/// The design report also puts server notices here — a PostgreSQL `NOTICE`, a
/// `RAISE NOTICE` from a function. There is no field for them, because neither
/// driver in this round can produce one: the blocking `postgres` client exposes
/// no notice handler (`Client::notifications` is `LISTEN`/`NOTIFY`, which is a
/// different thing), and SQLite has no such concept. A field every driver fills
/// with an empty vector is a console pane nobody can ever see text in.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Execution {
    /// The row count for a statement that changed rows; `None` for one that
    /// returned a result set. The two are exclusive, and which one it is comes
    /// from the driver rather than from guessing at the statement's first word.
    pub rows_affected: Option<u64>,
    /// The sink stopped the read before the server ran out of rows. Set by the
    /// driver, from the sink's own answer — see `models::page`.
    pub truncated: bool,
    pub elapsed: Duration,
}

#[cfg(test)]
mod tests {
    use super::{Execution, QueryRequest};

    #[test]
    fn a_request_carries_the_statement_verbatim() {
        let statement = "SELECT 1 -- keep my comment";
        assert_eq!(QueryRequest::new(statement).statement, statement);
    }

    #[test]
    fn a_default_execution_claims_nothing() {
        let execution = Execution::default();
        assert_eq!(execution.rows_affected, None);
        assert!(!execution.truncated);
        assert_eq!(execution.elapsed.as_nanos(), 0);
    }
}
