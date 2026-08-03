//! Running the editor's buffer, and what the result footer says about it.
//!
//! [`run`] is one blocking function over a `&dyn Driver`, which is what makes
//! the *ordering* and the *reporting* testable with a fake driver and no server
//! — the same trick `api_explorer::services::send` uses.
//!
//! # What Execute does with a buffer of several statements
//!
//! It splits ([`models::split`](crate::database::models::split)) and runs them
//! **in order, stopping at the first failure**. The result on screen is the
//! last statement that returned a result set; if none did, it is the last
//! statement that ran, reporting what it changed. That is the behaviour a
//! `CREATE TABLE …; INSERT …; SELECT …` buffer wants, and it is why the footer
//! names the statement it is showing: with several statements in the editor,
//! "42 rows" without saying *which* statement produced them is not an answer.
//!
//! A failure names the statement that failed, for the same reason.
//!
//! # No `LIMIT` is ever appended
//!
//! Every statement goes to the driver exactly as written. The bound on what
//! comes back is the sink's — see `models::page` for why that is the only way
//! to bound a statement dodo did not write.

use std::time::Duration;

use crate::database::models::error::DbError;
use crate::database::models::page::{PageBudget, PageBuffer};
use crate::database::models::query::QueryRequest;
use crate::database::models::split::split_statements;
use crate::database::models::value::{ColumnMeta, Row};
use crate::database::services::Driver;
use crate::i18n::Str;

/// Where the query pane is.
#[derive(Default)]
pub enum QueryState {
    /// Nothing has been run in this tab yet.
    #[default]
    Idle,
    Running,
    Done(Outcome),
    Failed(Failure),
}

/// What one Execute produced.
#[derive(Clone, Debug, PartialEq)]
pub struct Outcome {
    /// The statement whose result is on screen, exactly as the user wrote it.
    pub statement: String,
    pub columns: Vec<ColumnMeta>,
    pub rows: Vec<Row>,
    /// `Some` for a statement that changed rows rather than returning them.
    pub rows_affected: Option<u64>,
    pub truncated: bool,
    pub capped_cells: usize,
    pub elapsed: Duration,
    /// How many statements ran in total, so a buffer of several can say that
    /// the row count belongs to one of them.
    pub statements_run: usize,
}

/// Why an Execute produced nothing.
#[derive(Clone, Debug, PartialEq)]
pub enum Failure {
    /// The buffer held no statement — it was empty, or nothing but comments.
    Nothing,
    /// A statement was rejected. Both halves matter: the message says what went
    /// wrong, and the statement says which of several it went wrong in.
    Rejected { statement: String, error: DbError },
    /// The user cancelled, and the **server** said it stopped.
    ///
    /// Its own variant rather than a `Rejected` carrying
    /// [`DbError::Cancelled`], because the pane draws it differently: a
    /// cancellation is not a fault and must not be shown in the danger tone
    /// beside a red triangle. [`Failure::is_cancelled`] is what the view asks.
    Cancelled { statement: String },
}

impl Failure {
    pub fn message(&self) -> Str {
        match self {
            Failure::Nothing => Str::DbNoStatement,
            Failure::Rejected { error, .. } => error.message(),
            Failure::Cancelled { .. } => Str::DbCancelledMessage,
        }
    }

    /// The statement to show beside the message, if there is one worth showing.
    pub fn statement(&self) -> Option<&str> {
        match self {
            Failure::Nothing => None,
            Failure::Rejected { statement, .. } | Failure::Cancelled { statement } => {
                Some(statement)
            }
        }
    }

    pub fn is_cancelled(&self) -> bool {
        matches!(self, Failure::Cancelled { .. })
    }
}

/// Runs `buffer` against `driver`.
///
/// **Blocking**, like everything it calls: run it on GPUI's background
/// executor, never on the UI thread.
pub fn run(driver: &dyn Driver, buffer: &str, budget: PageBudget) -> Result<Outcome, Failure> {
    let statements = split_statements(buffer);
    if statements.is_empty() {
        return Err(Failure::Nothing);
    }

    let total = statements.len();
    let mut last: Option<Outcome> = None;

    for (index, statement) in statements.into_iter().enumerate() {
        let mut sink = PageBuffer::new(budget);
        let execution = driver
            .execute(&QueryRequest::new(statement.clone()), &mut sink)
            .map_err(|error| {
                // A cancelled statement is the user's own doing and is reported
                // as such — including the *rest* of a multi-statement buffer
                // never running, which is the same rule a rejection follows.
                if error.is_cancelled() {
                    Failure::Cancelled {
                        statement: statement.clone(),
                    }
                } else {
                    Failure::Rejected {
                        statement: statement.clone(),
                        error,
                    }
                }
            })?;

        let (columns, rows, truncated, capped_cells) = sink.into_parts();
        let outcome = Outcome {
            statement,
            columns,
            rows,
            rows_affected: execution.rows_affected,
            truncated: truncated || execution.truncated,
            capped_cells,
            elapsed: execution.elapsed,
            statements_run: index + 1,
        };

        // A statement that returned a result set wins over an earlier one that
        // did not: in `INSERT …; SELECT …` the rows are what the user is
        // looking for. A later result set replaces an earlier one, so the last
        // `SELECT` is what shows.
        last = match last {
            Some(previous) if outcome.columns.is_empty() && !previous.columns.is_empty() => {
                Some(Outcome {
                    // Keep the rows on screen, but let the footer report that
                    // more statements ran after them.
                    statements_run: outcome.statements_run,
                    ..previous
                })
            }
            _ => Some(outcome),
        };
    }

    // `total` statements were split, and the loop returned early on failure, so
    // reaching here means every one of them ran.
    Ok(Outcome {
        statements_run: total,
        ..last.expect("at least one statement ran")
    })
}

/// Runs PostgreSQL's non-executing plan command for every statement in
/// `buffer`.
///
/// Each statement is wrapped separately. Prefixing the whole buffer once would
/// explain only its first statement and then execute the rest normally — an
/// especially bad surprise for `SELECT …; DELETE …`.
pub fn explain(driver: &dyn Driver, buffer: &str, budget: PageBudget) -> Result<Outcome, Failure> {
    let statements = split_statements(buffer);
    if statements.is_empty() {
        return Err(Failure::Nothing);
    }
    let explained = statements
        .iter()
        .map(|statement| {
            driver
                .explain_statement(statement)
                .expect("Explain is reached only when the driver reports the capability")
        })
        .collect::<Vec<_>>()
        .join(";\n");
    run(driver, &explained, budget)
}

impl Outcome {
    /// The footer, as the pieces it is built from.
    ///
    /// Returned as a list rather than one sentence so that each piece is its
    /// own `Str` in both languages and the view joins them with a separator it
    /// controls. Pure, and the reason the footer's honesty is testable.
    pub fn footer(&self) -> Vec<Str> {
        let mut parts = Vec::new();

        match self.rows_affected {
            Some(count) => parts.push(Str::DbFooterRowsAffected(count)),
            None => parts.push(Str::DbFooterRows(self.rows.len())),
        }

        if self.truncated {
            parts.push(Str::DbFooterTruncated(self.rows.len()));
        }
        if self.capped_cells > 0 {
            parts.push(Str::DbFooterCapped(self.capped_cells));
        }

        parts.push(Str::DbFooterElapsed(format_elapsed(self.elapsed)));
        parts
    }

    /// Whether there is a grid to draw at all.
    pub fn has_grid(&self) -> bool {
        !self.columns.is_empty()
    }
}

/// An elapsed time at a magnitude a person reads.
///
/// Sub-millisecond work reports microseconds rather than the "0 ms" that makes
/// a fast query look like it did not run; anything over a second drops to one
/// decimal, because the third decimal of a four-second query is noise.
pub fn format_elapsed(elapsed: Duration) -> String {
    let micros = elapsed.as_micros();
    if micros < 1_000 {
        return format!("{micros} µs");
    }
    let millis = elapsed.as_secs_f64() * 1_000.0;
    if millis < 1_000.0 {
        return format!("{} ms", millis.round() as u64);
    }
    format!("{:.1} s", elapsed.as_secs_f64())
}

#[cfg(test)]
mod tests {
    use super::{Failure, QueryState, explain, format_elapsed, run};
    use crate::database::models::error::DbError;
    use crate::database::models::page::PageBudget;
    use crate::database::models::value::Value;
    use crate::database::services::Driver as _;
    use crate::database::services::fake::FakeDriver;
    use crate::i18n::{Language, Str};
    use std::time::Duration;

    fn budget() -> PageBudget {
        PageBudget::default()
    }

    #[test]
    fn a_single_statement_runs_and_its_rows_come_back() {
        let driver = FakeDriver::sql();
        let outcome = run(&driver, "SELECT * FROM users", budget()).expect("runs");

        assert_eq!(outcome.statement, "SELECT * FROM users");
        assert_eq!(outcome.rows.len(), 3);
        assert_eq!(outcome.columns.len(), 2);
        assert_eq!(outcome.statements_run, 1);
        assert!(outcome.has_grid());
    }

    #[test]
    fn explain_wraps_every_statement_so_none_can_run_normally() {
        let driver = FakeDriver::sql();
        let outcome =
            explain(&driver, "SELECT * FROM users; DELETE FROM users;", budget()).expect("plans");

        assert_eq!(outcome.statements_run, 2);
        assert_eq!(
            driver.executed.lock().unwrap().as_slice(),
            ["EXPLAIN SELECT * FROM users", "EXPLAIN DELETE FROM users"]
        );
    }

    /// The statement is what the footer names, so it must be what was sent —
    /// comments and all.
    #[test]
    fn the_statement_is_reported_exactly_as_it_was_written() {
        let driver = FakeDriver::sql();
        let outcome = run(&driver, "  SELECT 1 -- my note  \n", budget()).expect("runs");
        assert_eq!(outcome.statement, "SELECT 1 -- my note");
        assert_eq!(
            driver.executed.lock().unwrap().as_slice(),
            ["SELECT 1 -- my note"]
        );
    }

    #[test]
    fn an_empty_buffer_is_nothing_to_run_rather_than_a_query() {
        let driver = FakeDriver::sql();
        assert_eq!(run(&driver, "", budget()), Err(Failure::Nothing));
        assert_eq!(run(&driver, "   \n\t", budget()), Err(Failure::Nothing));
        assert_eq!(
            run(&driver, "-- just a note", budget()),
            Err(Failure::Nothing)
        );
        assert!(
            driver.executed.lock().unwrap().is_empty(),
            "nothing should have reached the server"
        );
    }

    #[test]
    fn several_statements_run_in_order() {
        let driver = FakeDriver::sql();
        run(&driver, "SELECT 1; SELECT 2; SELECT 3;", budget()).expect("runs");
        assert_eq!(
            driver.executed.lock().unwrap().as_slice(),
            ["SELECT 1", "SELECT 2", "SELECT 3"]
        );
    }

    #[test]
    fn the_last_statement_is_the_one_on_screen_and_the_footer_knows_how_many_ran() {
        let driver = FakeDriver::sql();
        let outcome = run(&driver, "SELECT 1; SELECT 2;", budget()).expect("runs");
        assert_eq!(outcome.statement, "SELECT 2");
        assert_eq!(outcome.statements_run, 2);
    }

    /// `INSERT …; SELECT …` and `SELECT …; INSERT …` should both leave the rows
    /// on screen: the rows are what the user is looking at.
    #[test]
    fn a_result_set_is_not_replaced_by_a_later_statement_that_returns_none() {
        let driver = FakeDriver::sql();
        let outcome = run(&driver, "SELECT * FROM users; SELECT 2;", budget()).expect("runs");
        assert!(outcome.has_grid());
        assert_eq!(outcome.rows.len(), 3);
        assert_eq!(outcome.statements_run, 2);
    }

    #[test]
    fn the_first_failure_stops_the_run_and_names_the_statement_that_failed() {
        let driver = FakeDriver::sql().failing(DbError::server("syntax error at or near \"FRUM\""));
        match run(&driver, "SELECT 1; SELECT FRUM;", budget()) {
            Err(Failure::Rejected { statement, error }) => {
                assert_eq!(
                    statement, "SELECT 1",
                    "the first statement is the one that failed"
                );
                assert!(matches!(error, DbError::Server { .. }));
            }
            other => panic!("expected a rejection, got {other:?}"),
        }

        assert_eq!(
            driver.executed.lock().unwrap().len(),
            1,
            "nothing after the failure should have been sent"
        );
    }

    /// A cancellation must reach the user as a cancellation — not as a server
    /// error, and above all not as a silent empty result.
    #[test]
    fn a_cancelled_statement_is_its_own_outcome_and_names_the_statement() {
        let driver = FakeDriver::sql();
        driver
            .cancel_handle()
            .expect("the fake reports the capability")
            .cancel()
            .expect("the handle fires");

        match run(&driver, "SELECT 1; SELECT 2;", budget()) {
            Err(failure @ Failure::Cancelled { .. }) => {
                assert!(failure.is_cancelled());
                assert_eq!(failure.statement(), Some("SELECT 1"));
                assert!(matches!(failure.message(), Str::DbCancelledMessage));
            }
            other => panic!("expected a cancellation, got {other:?}"),
        }

        assert_eq!(
            driver.executed.lock().unwrap().len(),
            1,
            "the rest of the buffer must not run after a cancel, exactly as after a rejection"
        );
    }

    #[test]
    fn a_rejection_is_not_mistaken_for_a_cancellation() {
        let driver = FakeDriver::sql().failing(DbError::server("syntax error"));
        let failure = run(&driver, "SELECT FRUM", budget()).expect_err("fails");
        assert!(!failure.is_cancelled());
        assert!(!Failure::Nothing.is_cancelled());
    }

    /// A backend with no cancel mechanism is not a special case anywhere above
    /// `services/`: it reports the capability as absent and offers no handle.
    #[test]
    fn a_driver_that_cannot_cancel_says_so_rather_than_pretending() {
        let driver = FakeDriver::sql().without_cancel();
        assert!(!driver.capabilities().cancel);
        assert!(driver.cancel_handle().is_none());
    }

    #[test]
    fn a_failure_reads_in_both_languages_and_keeps_the_servers_words() {
        let failure = Failure::Rejected {
            statement: "SELECT 1".into(),
            error: DbError::server("relation \"nope\" does not exist"),
        };
        assert_eq!(failure.statement(), Some("SELECT 1"));
        for language in Language::ALL {
            assert!(failure.message().text(language).contains("nope"));
        }

        assert_eq!(Failure::Nothing.statement(), None);
        assert!(matches!(Failure::Nothing.message(), Str::DbNoStatement));
    }

    // ---- the footer ------------------------------------------------------

    #[test]
    fn the_footer_reports_a_row_count_and_an_elapsed_time() {
        let driver = FakeDriver::sql();
        let outcome = run(&driver, "SELECT 1", budget()).expect("runs");

        let footer = outcome.footer();
        assert!(matches!(footer[0], Str::DbFooterRows(3)));
        assert!(matches!(footer.last(), Some(Str::DbFooterElapsed(_))));
    }

    /// The whole point of the bound being honest: when it trips, the footer
    /// says so in as many words.
    #[test]
    fn the_footer_says_plainly_when_the_result_was_truncated() {
        let driver = FakeDriver::sql().with_rows(500);
        let outcome = run(
            &driver,
            "SELECT * FROM big",
            PageBudget {
                max_rows: 10,
                ..budget()
            },
        )
        .expect("runs");

        assert!(outcome.truncated);
        let footer = outcome.footer();
        assert!(
            footer
                .iter()
                .any(|part| matches!(part, Str::DbFooterTruncated(10))),
            "no truncation notice in {footer:?}"
        );

        for language in Language::ALL {
            let text = Str::DbFooterTruncated(10).text(language).into_owned();
            assert!(
                text.contains("10"),
                "{} dropped the count: {text}",
                language.code()
            );
        }
    }

    #[test]
    fn a_result_that_fits_says_nothing_about_truncation() {
        let driver = FakeDriver::sql();
        let outcome = run(&driver, "SELECT 1", budget()).expect("runs");
        assert!(
            !outcome
                .footer()
                .iter()
                .any(|part| matches!(part, Str::DbFooterTruncated(_))),
            "a complete result must not claim there was more"
        );
    }

    #[test]
    fn the_footer_counts_the_cells_that_were_shortened() {
        let driver = FakeDriver::sql();
        let outcome = run(
            &driver,
            "SELECT * FROM users",
            PageBudget {
                max_cell_bytes: 2,
                ..budget()
            },
        )
        .expect("runs");

        assert!(outcome.capped_cells > 0);
        assert!(
            outcome
                .footer()
                .iter()
                .any(|part| matches!(part, Str::DbFooterCapped(_)))
        );
        assert!(matches!(outcome.rows[0][1], Value::Truncated { .. }));
    }

    #[test]
    fn a_statement_that_changed_rows_reports_that_instead_of_a_row_count() {
        let outcome = super::Outcome {
            statement: "UPDATE users SET a = 1".into(),
            columns: Vec::new(),
            rows: Vec::new(),
            rows_affected: Some(7),
            truncated: false,
            capped_cells: 0,
            elapsed: Duration::from_millis(3),
            statements_run: 1,
        };

        assert!(!outcome.has_grid());
        assert!(matches!(outcome.footer()[0], Str::DbFooterRowsAffected(7)));
    }

    // ---- elapsed formatting ---------------------------------------------

    /// A query that took 300 µs reporting "0 ms" reads as though it did not
    /// run at all.
    #[test]
    fn a_sub_millisecond_query_reports_microseconds() {
        assert_eq!(format_elapsed(Duration::from_micros(300)), "300 µs");
        assert_eq!(format_elapsed(Duration::from_micros(999)), "999 µs");
    }

    #[test]
    fn milliseconds_and_seconds_pick_their_own_magnitude() {
        assert_eq!(format_elapsed(Duration::from_millis(12)), "12 ms");
        assert_eq!(format_elapsed(Duration::from_millis(999)), "999 ms");
        assert_eq!(format_elapsed(Duration::from_millis(1_000)), "1.0 s");
        assert_eq!(format_elapsed(Duration::from_millis(4_280)), "4.3 s");
    }

    #[test]
    fn a_fresh_query_state_is_idle() {
        assert!(matches!(QueryState::default(), QueryState::Idle));
    }
}
