//! The bound on how much of a result dodo holds, enforced rather than intended.
//!
//! "Do not load a huge dataset into memory" is the kind of requirement that
//! usually ends up as a comment above a `Vec`. Here it is the type: a driver
//! never allocates a whole result, it hands over **one row at a time** through
//! [`RowSink`] and stops the instant the sink says [`Flow::Stop`]. The sink is
//! [`PageBuffer`], which stops when either of two bounds trips.
//!
//! Three bounds, all in [`PageBudget`]:
//!
//! - `max_rows` — how many rows are kept at once.
//! - `max_bytes` — the total decoded payload kept at once, so a thousand rows
//!   of one-megabyte text cannot pass a row-count-only check.
//! - `max_cell_bytes` — the longest single value kept verbatim; a longer one
//!   becomes [`Value::Truncated`] with its real length recorded, so the grid
//!   can say what it did.
//!
//! # What dodo deliberately does not do
//!
//! **It never injects `LIMIT` into a statement the user wrote.** That would
//! silently change a `SELECT … FOR UPDATE`, break a CTE that has its own
//! `LIMIT`, and be simply wrong for a multi-statement buffer. Bounding at the
//! sink needs no parsing and cannot change what the statement means. The cost
//! is honest and stated in the footer: dodo shows the first `max_rows` rows and
//! says the result had more.
//!
//! **There is no on-disk spill.** That is a second storage system — lifecycle,
//! cleanup, its own failure modes — to serve a case (a human scrolling past row
//! 1,000 in a GUI) that does not happen. Exporting the full result re-runs the
//! statement into a file-backed sink with a peak footprint of one row; that is
//! the answer to the real need.

use super::value::{ColumnMeta, Row, cap_row, row_bytes};

/// Whether a driver should keep reading.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flow {
    Continue,
    Stop,
}

/// Where a driver puts rows *as it reads them*.
///
/// Implemented by [`PageBuffer`] in the shipping path and by test doubles that
/// count rows; the trait is what lets `execute`'s streaming behaviour be
/// asserted without a server.
pub trait RowSink {
    /// Called exactly once, before any row, with the result's shape. A
    /// statement that returns no result set (an `UPDATE`) never calls it.
    fn columns(&mut self, columns: Vec<ColumnMeta>);

    /// One row. Returning [`Flow::Stop`] means the sink is full: the driver
    /// closes its cursor and reports `truncated`.
    fn row(&mut self, row: Row) -> Flow;
}

/// How much of a result may be held at once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PageBudget {
    pub max_rows: usize,
    pub max_bytes: usize,
    pub max_cell_bytes: usize,
}

impl Default for PageBudget {
    /// 1,000 rows, 8 MiB, 64 KiB per cell.
    ///
    /// 1,000 rows is far more than a human reads and far less than a result
    /// that hurts; 8 MiB is the backstop for a result whose rows are wide
    /// rather than many; 64 KiB per cell keeps one `bytea` column from
    /// swallowing the whole byte budget on the first row, which would make the
    /// grid look empty for a reason the user cannot see.
    fn default() -> Self {
        Self {
            max_rows: 1_000,
            max_bytes: 8 * 1024 * 1024,
            max_cell_bytes: 64 * 1024,
        }
    }
}

/// One bounded page of a result, filled by a driver.
#[derive(Clone, Debug)]
pub struct PageBuffer {
    budget: PageBudget,
    columns: Vec<ColumnMeta>,
    rows: Vec<Row>,
    bytes: usize,
    truncated: bool,
    /// How many cells were cut down to fit `max_cell_bytes`. Reported so the
    /// footer can say it; a cell shown as `abc…` with no explanation is the
    /// thing this count exists to prevent.
    capped_cells: usize,
}

impl Default for PageBuffer {
    fn default() -> Self {
        Self::new(PageBudget::default())
    }
}

impl PageBuffer {
    pub fn new(budget: PageBudget) -> Self {
        Self {
            budget,
            columns: Vec::new(),
            rows: Vec::new(),
            bytes: 0,
            truncated: false,
            capped_cells: 0,
        }
    }

    /// What the page is holding.
    ///
    /// The shipping path takes [`PageBuffer::into_parts`] instead, so that a
    /// thousand rows move rather than being copied; these exist for the
    /// assertions that keep the bounds honest.
    #[cfg(test)]
    pub fn columns(&self) -> &[ColumnMeta] {
        &self.columns
    }

    #[cfg(test)]
    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// Whether a bound stopped the read before the server ran out of rows.
    #[cfg(test)]
    pub fn truncated(&self) -> bool {
        self.truncated
    }

    #[cfg(test)]
    pub fn capped_cells(&self) -> usize {
        self.capped_cells
    }

    /// The page as its parts, so the result state can take ownership without
    /// cloning a thousand rows.
    pub fn into_parts(self) -> (Vec<ColumnMeta>, Vec<Row>, bool, usize) {
        (self.columns, self.rows, self.truncated, self.capped_cells)
    }

    /// Whether another row would fit. Checked *before* accepting one, which is
    /// what makes the bounds hold rather than being exceeded by one row.
    fn full(&self) -> bool {
        self.rows.len() >= self.budget.max_rows || self.bytes >= self.budget.max_bytes
    }
}

impl RowSink for PageBuffer {
    fn columns(&mut self, columns: Vec<ColumnMeta>) {
        self.columns = columns;
    }

    fn row(&mut self, row: Row) -> Flow {
        if self.full() {
            // The driver offered a row we will not keep, which is exactly what
            // "the result had more" means.
            self.truncated = true;
            return Flow::Stop;
        }

        let capped = cap_row(row, &self.budget);
        self.capped_cells += capped
            .iter()
            .filter(|value| matches!(value, super::value::Value::Truncated { .. }))
            .count();
        self.bytes += row_bytes(&capped);
        self.rows.push(capped);

        // `Continue` even when this row filled the page, which is the whole
        // reason `truncated` can be trusted: saying "there was more" when there
        // was not is what makes a truncation notice worthless, and a full page
        // is not evidence either way. So the driver is allowed to offer exactly
        // one more row, and *that* offer — refused above — is the proof. The
        // cost is one extra row decoded per truncated result.
        Flow::Continue
    }
}

/// A sink that counts rows and keeps none.
///
/// Not used by the shipping path — it is what an export or a row-count would
/// use, and what the tests use to prove that a driver really does stream rather
/// than materialise. Kept here beside the trait it implements so the contract
/// has a second, deliberately different, implementation.
#[cfg(test)]
#[derive(Default)]
pub struct CountingSink {
    pub columns: Vec<ColumnMeta>,
    pub rows: usize,
}

#[cfg(test)]
impl RowSink for CountingSink {
    fn columns(&mut self, columns: Vec<ColumnMeta>) {
        self.columns = columns;
    }

    fn row(&mut self, _row: Row) -> Flow {
        self.rows += 1;
        Flow::Continue
    }
}

#[cfg(test)]
mod tests {
    use super::{CountingSink, Flow, PageBudget, PageBuffer, RowSink};
    use crate::models::value::{ColumnMeta, Row, Value};

    fn columns() -> Vec<ColumnMeta> {
        vec![
            ColumnMeta::new("id", "int4"),
            ColumnMeta::new("name", "text"),
        ]
    }

    fn row(n: i64) -> Row {
        vec![Value::Int(n), Value::Text(format!("name-{n}"))]
    }

    /// Offers `count` rows and stops the moment the sink says to — i.e. exactly
    /// what a driver's read loop does.
    ///
    /// It reports how many rows were *offered*, not how many were kept: a
    /// `Flow::Stop` is deliberately ambiguous about the row that came with it
    /// (the sink either had no room for it, or took it and then filled up), and
    /// a driver has no reason to care. The sink's own `rows()` is the authority
    /// on what was kept, which is what these tests assert against.
    fn feed(buffer: &mut PageBuffer, count: usize) -> usize {
        RowSink::columns(buffer, columns());
        let mut offered = 0;
        for n in 0..count {
            offered += 1;
            if buffer.row(row(n as i64)) == Flow::Stop {
                break;
            }
        }
        offered
    }

    #[test]
    fn a_result_smaller_than_the_budget_is_not_truncated() {
        let mut buffer = PageBuffer::default();
        feed(&mut buffer, 10);
        assert_eq!(buffer.rows().len(), 10);
        assert!(!buffer.truncated());
        assert_eq!(buffer.columns().len(), 2);
    }

    #[test]
    fn the_row_bound_holds_exactly_and_is_not_exceeded_by_one() {
        let budget = PageBudget {
            max_rows: 5,
            ..PageBudget::default()
        };
        let mut buffer = PageBuffer::new(budget);
        feed(&mut buffer, 100);
        assert_eq!(buffer.rows().len(), 5, "the bound is a bound, not a target");
    }

    /// The distinction that makes the footer trustworthy: filling the page is
    /// not the same as there being more.
    #[test]
    fn a_result_that_exactly_fills_the_page_is_not_reported_as_truncated() {
        let budget = PageBudget {
            max_rows: 3,
            ..PageBudget::default()
        };
        let mut buffer = PageBuffer::new(budget);
        RowSink::columns(&mut buffer, columns());
        for n in 0..3 {
            assert_eq!(
                buffer.row(row(n)),
                Flow::Continue,
                "filling the page is not itself a reason to stop — being offered \
                 one more row is what proves there was more"
            );
        }
        assert_eq!(buffer.rows().len(), 3);
        assert!(
            !buffer.truncated(),
            "the server never offered a fourth row, so there is no evidence of more"
        );
    }

    #[test]
    fn one_more_row_than_fits_is_what_proves_truncation() {
        let budget = PageBudget {
            max_rows: 3,
            ..PageBudget::default()
        };
        let mut buffer = PageBuffer::new(budget);
        let offered = feed(&mut buffer, 4);
        assert_eq!(offered, 4, "the fourth row is the one that proves it");
        assert_eq!(buffer.rows().len(), 3);
        assert!(buffer.truncated());
    }

    /// A row-count-only bound would let a thousand one-megabyte rows through.
    #[test]
    fn the_byte_bound_stops_a_result_that_is_wide_rather_than_long() {
        let budget = PageBudget {
            max_rows: 1_000_000,
            max_bytes: 4_000,
            max_cell_bytes: 64 * 1024,
        };
        let mut buffer = PageBuffer::new(budget);
        RowSink::columns(&mut buffer, vec![ColumnMeta::new("blob", "text")]);

        let mut accepted = 0;
        for _ in 0..100 {
            if buffer.row(vec![Value::Text("x".repeat(1_000))]) == Flow::Stop {
                break;
            }
            accepted += 1;
        }

        assert!(accepted <= 5, "the byte budget must stop this well short");
        assert!(
            accepted >= 3,
            "and must not stop it before the budget is used"
        );
        assert!(buffer.rows().len() < 100);
    }

    #[test]
    fn an_oversized_cell_is_capped_counted_and_does_not_eat_the_byte_budget() {
        let budget = PageBudget {
            max_rows: 100,
            max_bytes: 1_000_000,
            max_cell_bytes: 16,
        };
        let mut buffer = PageBuffer::new(budget);
        RowSink::columns(&mut buffer, vec![ColumnMeta::new("blob", "text")]);
        assert_eq!(
            buffer.row(vec![Value::Text("y".repeat(100_000))]),
            Flow::Continue
        );

        assert_eq!(buffer.capped_cells(), 1);
        match &buffer.rows()[0][0] {
            Value::Truncated { prefix, full_bytes } => {
                assert_eq!(prefix.len(), 16);
                assert_eq!(*full_bytes, 100_000);
            }
            other => panic!("expected a truncated cell, got {other:?}"),
        }
        assert!(
            !buffer.truncated(),
            "capping a cell does not truncate the result"
        );
    }

    #[test]
    fn a_page_hands_over_its_parts_without_copying_them() {
        let mut buffer = PageBuffer::default();
        feed(&mut buffer, 3);
        let (columns, rows, truncated, capped) = buffer.into_parts();
        assert_eq!(columns.len(), 2);
        assert_eq!(rows.len(), 3);
        assert!(!truncated);
        assert_eq!(capped, 0);
    }

    #[test]
    fn the_shipped_defaults_are_the_ones_documented() {
        let budget = PageBudget::default();
        assert_eq!(budget.max_rows, 1_000);
        assert_eq!(budget.max_bytes, 8 * 1024 * 1024);
        assert_eq!(budget.max_cell_bytes, 64 * 1024);
    }

    /// A second implementation of the trait, proving the contract is about the
    /// sink rather than about `PageBuffer`.
    #[test]
    fn a_sink_that_keeps_nothing_never_stops_the_driver() {
        let mut sink = CountingSink::default();
        RowSink::columns(&mut sink, columns());
        for n in 0..10_000 {
            assert_eq!(sink.row(row(n)), Flow::Continue);
        }
        assert_eq!(sink.rows, 10_000);
    }
}
