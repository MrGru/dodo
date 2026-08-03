//! One open object-detail surface and its current load.
//!
//! Only table data is paged. The next offset advances by the rows actually
//! kept, not by the nominal page size: if the existing byte budget fills after
//! 37 rows, row 38 is the first row requested next rather than row 101.

use crate::database::models::detail::{
    DATA_PAGE_SIZE, DdlSource, DetailField, DetailNotice, DetailRequest, DetailTab, DetailTarget,
};
use crate::database::models::error::DbError;
use crate::database::models::page::{PageBudget, PageBuffer};
use crate::database::models::value::ColumnMeta;
use crate::database::services::{DetailResult, Driver};
use crate::database::state::edit::PendingGrid;

#[derive(Clone, Debug)]
pub struct DetailGrid {
    pub columns: Vec<ColumnMeta>,
    /// `None` for table data, whose headings are database identifiers.
    pub fields: Option<Vec<DetailField>>,
    pub grid: PendingGrid,
    pub has_more: bool,
    pub capped_cells: usize,
    pub notice: Option<DetailNotice>,
}

#[derive(Clone, Debug)]
pub enum DetailLoad {
    Idle,
    Loading,
    Grid(DetailGrid),
    Ddl(String),
    Empty(Option<DetailNotice>),
    Unavailable,
    Failed(DbError),
}

pub struct DetailState {
    pub connection: u64,
    pub target: DetailTarget,
    pub ddl_source: DdlSource,
    pub tab: DetailTab,
    pub load: DetailLoad,
    offset: u64,
    previous_offsets: Vec<u64>,
}

impl DetailState {
    pub fn new(connection: u64, target: DetailTarget, ddl_source: DdlSource) -> Self {
        Self {
            connection,
            target,
            ddl_source,
            tab: DetailTab::Data,
            load: DetailLoad::Idle,
            offset: 0,
            previous_offsets: Vec::new(),
        }
    }

    pub fn visible_tabs(&self) -> impl Iterator<Item = DetailTab> + '_ {
        DetailTab::ALL.into_iter().filter(|tab| {
            tab.applies_to(self.target.kind)
                && (*tab != DetailTab::Ddl || self.ddl_source != DdlSource::None)
        })
    }

    pub fn select(&mut self, tab: DetailTab) -> bool {
        if tab == self.tab || !self.visible_tabs().any(|visible| visible == tab) {
            return false;
        }
        self.tab = tab;
        self.offset = 0;
        self.previous_offsets.clear();
        self.load = DetailLoad::Idle;
        true
    }

    pub fn begin(&mut self) -> DetailRequest {
        self.load = DetailLoad::Loading;
        self.request()
    }

    pub fn request(&self) -> DetailRequest {
        DetailRequest::new(self.target.clone(), self.tab, self.offset)
    }

    pub fn apply(&mut self, request: &DetailRequest, load: DetailLoad) -> bool {
        if &self.request() != request {
            return false;
        }
        self.load = load;
        true
    }

    pub fn can_previous(&self) -> bool {
        self.tab == DetailTab::Data && !self.previous_offsets.is_empty()
    }

    pub fn can_next(&self) -> bool {
        self.tab == DetailTab::Data
            && matches!(&self.load, DetailLoad::Grid(grid) if grid.has_more && !grid.grid.rows().is_empty())
    }

    pub fn previous(&mut self) -> bool {
        let Some(offset) = self.previous_offsets.pop() else {
            return false;
        };
        self.offset = offset;
        self.load = DetailLoad::Idle;
        true
    }

    pub fn next(&mut self) -> bool {
        let DetailLoad::Grid(grid) = &self.load else {
            return false;
        };
        if !grid.has_more || grid.grid.rows().is_empty() {
            return false;
        }
        let Some(next) = self.offset.checked_add(grid.grid.rows().len() as u64) else {
            return false;
        };
        self.previous_offsets.push(self.offset);
        self.offset = next;
        self.load = DetailLoad::Idle;
        true
    }

    pub fn page_number(&self) -> usize {
        self.previous_offsets.len() + 1
    }

    pub fn first_row_number(&self) -> u64 {
        self.offset + 1
    }
}

/// Loads one detail section. Blocking by contract; the view runs this on the
/// background executor.
pub fn load(driver: &dyn Driver, request: &DetailRequest) -> DetailLoad {
    if !request.tab.applies_to(request.target.kind) {
        return DetailLoad::Unavailable;
    }

    let budget = if request.tab == DetailTab::Data {
        PageBudget {
            max_rows: DATA_PAGE_SIZE as usize,
            ..PageBudget::default()
        }
    } else {
        PageBudget::default()
    };
    let mut sink = PageBuffer::new(budget);

    match driver.detail(request, &mut sink) {
        Err(error) => DetailLoad::Failed(error),
        Ok(DetailResult::Unavailable) => DetailLoad::Unavailable,
        Ok(DetailResult::Ddl(sql)) if sql.trim().is_empty() => DetailLoad::Unavailable,
        Ok(DetailResult::Ddl(sql)) => DetailLoad::Ddl(sql),
        Ok(DetailResult::Rows {
            fields,
            truncated,
            notice,
        }) => {
            let (columns, rows, sink_truncated, capped_cells) = sink.into_parts();
            if rows.is_empty() && request.tab != DetailTab::Data {
                DetailLoad::Empty(notice)
            } else {
                let editability = driver.editability(&columns);
                DetailLoad::Grid(DetailGrid {
                    columns,
                    fields,
                    grid: PendingGrid::new(rows, editability),
                    has_more: truncated || sink_truncated,
                    capped_cells,
                    notice,
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{DetailLoad, DetailState, load};
    use crate::database::models::catalog::{NodeId, NodeKind};
    use crate::database::models::detail::{DdlSource, DetailTab, DetailTarget};
    use crate::database::models::error::DbError;
    use crate::database::services::fake::FakeDriver;
    use crate::database::state::edit::PendingGrid;

    fn state() -> DetailState {
        DetailState::new(
            7,
            DetailTarget::new(NodeId::new("table:users"), NodeKind::Table, "users"),
            DdlSource::Server,
        )
    }

    #[test]
    fn paging_advances_by_rows_kept_and_never_skips_after_a_short_page() {
        let mut state = state();
        state.load = DetailLoad::Grid(super::DetailGrid {
            columns: Vec::new(),
            fields: None,
            grid: PendingGrid::new(
                vec![Vec::new(); 37],
                crate::database::models::identity::Editability::ReadOnly(
                    crate::database::models::identity::ReadOnlyReason::NoColumns,
                ),
            ),
            has_more: true,
            capped_cells: 0,
            notice: None,
        });

        assert!(state.next());
        assert_eq!(state.request().offset, 37);
        assert_eq!(state.page_number(), 2);
        assert!(state.previous());
        assert_eq!(state.request().offset, 0);
        assert!(!state.previous());
    }

    #[test]
    fn paging_stops_at_the_boundaries() {
        let mut state = state();
        assert!(!state.can_previous());
        assert!(!state.next());

        state.load = DetailLoad::Grid(super::DetailGrid {
            columns: Vec::new(),
            fields: None,
            grid: PendingGrid::new(
                vec![Vec::new()],
                crate::database::models::identity::Editability::ReadOnly(
                    crate::database::models::identity::ReadOnlyReason::NoColumns,
                ),
            ),
            has_more: false,
            capped_cells: 0,
            notice: None,
        });
        assert!(!state.can_next());
        assert!(!state.next());
    }

    #[test]
    fn a_stale_background_answer_is_ignored() {
        let mut state = state();
        let request = state.begin();
        assert!(state.select(DetailTab::Columns));
        assert!(!state.apply(&request, DetailLoad::Empty(None)));
        assert!(matches!(state.load, DetailLoad::Idle));
    }

    #[test]
    fn fake_driver_covers_ready_empty_unavailable_and_error_states() {
        let ready = FakeDriver::sql();
        let request = state().request();
        assert!(matches!(load(&ready, &request), DetailLoad::Grid(_)));

        let empty = FakeDriver::sql().with_rows(0);
        assert!(matches!(load(&empty, &request), DetailLoad::Grid(_)));

        let unavailable = FakeDriver::key_value();
        assert!(matches!(
            load(&unavailable, &request),
            DetailLoad::Unavailable
        ));

        let failed = FakeDriver::sql().failing(DbError::server("denied"));
        assert!(matches!(load(&failed, &request), DetailLoad::Failed(_)));
    }
}
