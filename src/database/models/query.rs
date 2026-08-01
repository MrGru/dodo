//! What a driver is asked to run, and what it reports back.

use std::time::Duration;

/// One statement to execute.
///
/// The text is **exactly what the user typed**, and dodo does not rewrite it.
/// The design report reserves `limit`/`offset` fields here for the day
/// table-data views append `LIMIT n OFFSET m` to a statement *dodo itself*
/// generated; they are not declared yet, because nothing in this round can set
/// them and a field no caller fills is a decision nobody has made. See
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
    /// Server notices and warnings — a PostgreSQL `NOTICE`, a
    /// `RAISE NOTICE` from a function. Kept verbatim: they are the server's
    /// English and there is nothing to translate them with.
    pub notices: Vec<String>,
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
        assert!(execution.notices.is_empty());
        assert_eq!(execution.elapsed.as_nanos(), 0);
    }
}
