use std::collections::{BTreeMap, HashSet};
use std::time::SystemTime;

use crate::cleaner::core::category::{CleanerCategory, CleanerSection};
use crate::cleaner::core::item::{CleanableItem, CleanableItemId};
use crate::cleaner::core::progress::ScanProgress;
use crate::cleaner::core::report::{CategoryScanResult, CleanupReport, ScanCompleteness};
use crate::cleaner::core::risk::ItemCapability;
use crate::cleaner::core::scan_state::ScanState;
use crate::cleaner::core::selection::selected_by_default_ids;

/// Everything one category's own scan/selection/cleanup can be doing, kept
/// apart from every other category's — see the module doc on
/// [`CleanerState`] for why this is a map entry rather than a shared field.
#[derive(Default)]
pub struct CategoryState {
    scan_state: ScanState,
    progress: Option<ScanProgress>,
    /// The last completed scan's result. Deliberately *not* cleared by
    /// [`CleanerState::begin_scan`] — a Rescan keeps the previous result on
    /// screen (behind the scanning header) until the new one actually lands
    /// in [`CleanerState::finish_scan`], never mixing the two.
    result: Option<CategoryScanResult>,
    /// Set only on [`ScanState::Failed`] — the scanner call itself returned
    /// an error rather than a result (as opposed to
    /// [`crate::cleaner::core::report::ScanCompleteness::Partial`], which is
    /// a *successful* scan reporting that part of it was unreachable). Kept
    /// as the diagnostic detail behind a friendly summary, per req #18: the
    /// state itself (`Failed`) is what the UI leads with.
    error: Option<String>,
    selected_items: HashSet<CleanableItemId>,
    /// Whether a cleanup run is in flight for this category. Orthogonal to
    /// `scan_state`: cleaning never overwrites it, so a category can be
    /// `Completed` (the scan) and cleaning (the destructive step) at once,
    /// and the two are never confused with each other.
    cleaning: bool,
    cleanup_report: Option<CleanupReport>,
    started_at: Option<SystemTime>,
    finished_at: Option<SystemTime>,
    /// Bumped by every mutation that changes what [`Self::result`]'s items
    /// *are* — a finished scan replacing them, a cleanup or a "Keep"
    /// removing one. Deliberately **not** bumped by anything that merely
    /// changes how the scan is going (`begin_scan`, `update_progress`,
    /// `begin_cleaning`), because the items on screen are unchanged then:
    /// this is the identity `views::results_sync` compares to decide whether
    /// the results grid needs a fresh copy of the whole vector, and a
    /// rescan's progress ticks must not force one. Starts at 0 on a
    /// category that has never held a result.
    result_revision: u64,
    /// Bumped by every mutation of `selected_items`. Separate from
    /// [`Self::result_revision`] so ticking one checkbox on a 50,000-item
    /// result re-copies the selection alone and never the items.
    selection_revision: u64,
}

impl CategoryState {
    pub fn scan_state(&self) -> ScanState {
        self.scan_state
    }

    pub fn progress(&self) -> Option<&ScanProgress> {
        self.progress.as_ref()
    }

    pub fn result(&self) -> Option<&CategoryScanResult> {
        self.result.as_ref()
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn cleaning(&self) -> bool {
        self.cleaning
    }

    pub fn cleanup_report(&self) -> Option<&CleanupReport> {
        self.cleanup_report.as_ref()
    }

    pub fn started_at(&self) -> Option<SystemTime> {
        self.started_at
    }

    pub fn finished_at(&self) -> Option<SystemTime> {
        self.finished_at
    }

    /// See the field docs: these two are the whole reason the results grid
    /// can tell "the same result, drawn again" from "a different result".
    pub fn result_revision(&self) -> u64 {
        self.result_revision
    }

    pub fn selection_revision(&self) -> u64 {
        self.selection_revision
    }

    pub fn selected_count(&self) -> usize {
        self.result
            .as_ref()
            .map(|result| {
                result
                    .items
                    .iter()
                    .filter(|item| self.selected_items.contains(&item.id))
                    .count()
            })
            .unwrap_or(0)
    }

    pub fn selected_reclaimable_bytes(&self) -> u64 {
        self.result
            .as_ref()
            .map(|result| {
                result
                    .items
                    .iter()
                    .filter(|item| self.selected_items.contains(&item.id))
                    .map(|item| item.logical_size)
                    .sum()
            })
            .unwrap_or(0)
    }

    pub fn selected_ids(&self) -> Vec<CleanableItemId> {
        self.result
            .as_ref()
            .map(|result| {
                result
                    .items
                    .iter()
                    .filter(|item| self.selected_items.contains(&item.id))
                    .map(|item| item.id)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// The full items behind [`Self::selected_ids`], for handing straight to
    /// the cleanup pipeline (`macos::cleanup::cleanup_items` and friends).
    pub fn selected_items(&self) -> Vec<CleanableItem> {
        self.result
            .as_ref()
            .map(|result| {
                result
                    .items
                    .iter()
                    .filter(|item| self.selected_items.contains(&item.id))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    }
}

/// Every category's own scan/selection state, plus the sidebar's navigation
/// state — kept as three independent axes on purpose (the ticket's own
/// architectural rule): which category is on screen
/// ([`Self::selected_category`]), which sections are expanded
/// ([`Self::expanded_sections`]), and what each category's own scan/selection
/// is doing ([`Self::categories`]). None of the three is derived from
/// another: selecting a category never touches expansion, expanding a
/// section never touches any category's scan, and switching category never
/// touches — or even reads — any other category's [`CategoryState`].
pub struct CleanerState {
    selected_category: CleanerCategory,
    expanded_sections: HashSet<CleanerSection>,
    categories: BTreeMap<CleanerCategory, CategoryState>,
}

impl Default for CleanerState {
    fn default() -> Self {
        Self {
            selected_category: CleanerCategory::ALL[0],
            expanded_sections: CleanerSection::ALL.into_iter().collect(),
            categories: CleanerCategory::ALL
                .into_iter()
                .map(|category| (category, CategoryState::default()))
                .collect(),
        }
    }
}

impl CleanerState {
    pub fn selected_category(&self) -> CleanerCategory {
        self.selected_category
    }

    pub fn set_selected_category(&mut self, category: CleanerCategory) {
        self.selected_category = category;
    }

    pub fn is_section_expanded(&self, section: CleanerSection) -> bool {
        self.expanded_sections.contains(&section)
    }

    pub fn toggle_section_expanded(&mut self, section: CleanerSection) {
        if !self.expanded_sections.remove(&section) {
            self.expanded_sections.insert(section);
        }
    }

    pub fn category(&self, category: CleanerCategory) -> &CategoryState {
        // Every `CleanerCategory::ALL` entry is seeded in `default()` and
        // nothing ever removes one, so this is always present.
        self.categories
            .get(&category)
            .expect("every CleanerCategory has a seeded CategoryState")
    }

    fn category_mut(&mut self, category: CleanerCategory) -> &mut CategoryState {
        self.categories
            .get_mut(&category)
            .expect("every CleanerCategory has a seeded CategoryState")
    }

    pub fn begin_scan(&mut self, category: CleanerCategory) {
        let state = self.category_mut(category);
        state.scan_state = ScanState::Scanning;
        state.progress = None;
        state.cleanup_report = None;
        state.started_at = Some(SystemTime::now());
        state.finished_at = None;
        // `result` and `selected_items` are deliberately untouched — see the
        // field doc on `CategoryState::result`.
    }

    pub fn begin_cancelling(&mut self, category: CleanerCategory) {
        let state = self.category_mut(category);
        if state.scan_state == ScanState::Scanning {
            state.scan_state = ScanState::Cancelling;
        }
    }

    pub fn update_progress(&mut self, category: CleanerCategory, progress: ScanProgress) {
        self.category_mut(category).progress = Some(progress);
    }

    /// Ends this category's scan. `result` is `None` when the scan was
    /// cancelled or errored before producing one; otherwise it replaces
    /// whatever the previous scan left and the selection resets to that
    /// result's own defaults — never a mix of old selection and new items.
    pub fn finish_scan(
        &mut self,
        category: CleanerCategory,
        result: Option<CategoryScanResult>,
        cancelled: bool,
        error: Option<String>,
    ) {
        let had_error = error.is_some();
        let state = self.category_mut(category);
        state.progress = None;
        state.finished_at = Some(SystemTime::now());
        state.error = error;
        match result {
            Some(result) => {
                let partial = matches!(result.completeness, ScanCompleteness::Partial { .. });
                let has_warnings = !result.warnings.is_empty();
                state.scan_state =
                    ScanState::from_outcome(cancelled, had_error, partial, has_warnings);
                state.selected_items = selected_by_default_ids(&result.items);
                state.result = Some(result);
                state.result_revision += 1;
                state.selection_revision += 1;
            }
            None => {
                state.scan_state = ScanState::from_outcome(cancelled, had_error, false, false);
            }
        }
    }

    pub fn toggle_selected(&mut self, category: CleanerCategory, id: CleanableItemId) {
        let state = self.category_mut(category);
        if !state.selected_items.remove(&id) {
            state.selected_items.insert(id);
        }
        state.selection_revision += 1;
    }

    pub fn select_safe_items(&mut self, category: CleanerCategory) {
        let state = self.category_mut(category);
        if let Some(result) = state.result.as_ref() {
            state
                .selected_items
                .extend(selected_by_default_ids(&result.items));
        }
        state.selection_revision += 1;
    }

    pub fn clear_selection(&mut self, category: CleanerCategory) {
        let state = self.category_mut(category);
        state.selected_items.clear();
        state.selection_revision += 1;
    }

    /// Every row the header checkbox is allowed to select in bulk: items
    /// carrying [`ItemCapability::MoveToTrash`], the same capability the
    /// per-row checkbox already gates on (`views::results_table`). A row
    /// with no such capability (read-only informational rows) never gets
    /// bulk-selected, matching what the per-row checkbox already refuses.
    pub fn select_all(&mut self, category: CleanerCategory) {
        let state = self.category_mut(category);
        if let Some(result) = state.result.as_ref() {
            state.selected_items = result
                .items
                .iter()
                .filter(|item| item.capabilities.contains(&ItemCapability::MoveToTrash))
                .map(|item| item.id)
                .collect();
        }
        state.selection_revision += 1;
    }

    pub fn begin_cleaning(&mut self, category: CleanerCategory) {
        let state = self.category_mut(category);
        state.cleaning = true;
        state.cleanup_report = None;
    }

    pub fn finish_cleaning(&mut self, category: CleanerCategory, report: CleanupReport) {
        let state = self.category_mut(category);
        state.cleaning = false;
        if !report.successes.is_empty() {
            state.result_revision += 1;
            state.selection_revision += 1;
        }
        for success in &report.successes {
            state.selected_items.remove(&success.id);
            if let Some(result) = state.result.as_mut() {
                result.items.retain(|item| item.id != success.id);
            }
        }
        state.cleanup_report = Some(report);
    }

    /// Removes one item from a category's result and selection, without a
    /// rescan and without going through the cleanup pipeline. Used by "Keep"
    /// (`views::cleaner_view::CleanerView::mark_kept`): once the ignore list
    /// has recorded the path, the item disappears from view immediately
    /// rather than waiting for the next scan to leave it out.
    pub fn remove_item(&mut self, category: CleanerCategory, id: CleanableItemId) {
        let state = self.category_mut(category);
        if let Some(result) = state.result.as_mut() {
            result.items.retain(|item| item.id != id);
        }
        state.selected_items.remove(&id);
        state.result_revision += 1;
        state.selection_revision += 1;
    }
}

#[cfg(test)]
mod tests {
    use crate::cleaner::core::category::{CleanerCategory, CleanerSection};
    use crate::cleaner::core::item::{CleanableItem, CleanableItemId, ItemMetadata};
    use crate::cleaner::core::progress::{ScanPhase, ScanProgress};
    use crate::cleaner::core::report::{
        CategoryScanResult, CleanupItemSuccess, CleanupReport, PartialScanReason, ScanCompleteness,
    };
    use crate::cleaner::core::risk::{ItemCapability, RiskLevel, SelectionPolicy};
    use crate::cleaner::core::scan_state::ScanState;
    use crate::cleaner::state::CleanerState;

    fn sample_item(id: u64, category: CleanerCategory, risk: RiskLevel) -> CleanableItem {
        CleanableItem {
            id: CleanableItemId(id),
            category,
            group: None,
            display_name: format!("item-{id}"),
            path: format!("/tmp/item-{id}").into(),
            logical_size: 10,
            allocated_size: None,
            modified_at: None,
            last_accessed_at: None,
            risk,
            selection_policy: match risk {
                RiskLevel::SafeRecreatable => SelectionPolicy::SelectedByDefault,
                _ => SelectionPolicy::NotSelectedByDefault,
            },
            capabilities: vec![ItemCapability::MoveToTrash],
            explanation: String::new(),
            warnings: Vec::new(),
            metadata: ItemMetadata::Generic,
        }
    }

    fn complete_result(category: CleanerCategory, items: Vec<CleanableItem>) -> CategoryScanResult {
        CategoryScanResult {
            category,
            estimated_reclaimable_bytes: items.iter().map(|item| item.logical_size).sum(),
            scanned_entries: items.len() as u64,
            items,
            warnings: Vec::new(),
            completeness: ScanCompleteness::Complete,
        }
    }

    #[test]
    fn expanding_one_section_does_not_affect_another() {
        let mut state = CleanerState::default();
        // All three start expanded (the mockup's default), so collapse one
        // first to make the independence observable.
        state.toggle_section_expanded(CleanerSection::Cleanup);
        assert!(!state.is_section_expanded(CleanerSection::Cleanup));
        assert!(state.is_section_expanded(CleanerSection::Advanced));

        state.toggle_section_expanded(CleanerSection::Advanced);
        assert!(!state.is_section_expanded(CleanerSection::Advanced));
        assert!(
            !state.is_section_expanded(CleanerSection::Cleanup),
            "collapsing Advanced must not re-expand Cleanup"
        );
        assert!(state.is_section_expanded(CleanerSection::Applications));
    }

    #[test]
    fn selecting_a_category_does_not_alter_expansion_state() {
        let mut state = CleanerState::default();
        state.toggle_section_expanded(CleanerSection::Advanced);
        state.set_selected_category(CleanerCategory::DockerCache);

        assert_eq!(state.selected_category(), CleanerCategory::DockerCache);
        assert!(!state.is_section_expanded(CleanerSection::Advanced));
        assert!(state.is_section_expanded(CleanerSection::Cleanup));
    }

    #[test]
    fn switching_category_does_not_lose_scan_results() {
        let mut state = CleanerState::default();
        state.begin_scan(CleanerCategory::SystemJunk);
        state.finish_scan(
            CleanerCategory::SystemJunk,
            Some(complete_result(
                CleanerCategory::SystemJunk,
                vec![sample_item(
                    1,
                    CleanerCategory::SystemJunk,
                    RiskLevel::SafeRecreatable,
                )],
            )),
            false,
            None,
        );

        state.set_selected_category(CleanerCategory::DockerCache);
        state.set_selected_category(CleanerCategory::SystemJunk);

        assert!(
            state
                .category(CleanerCategory::SystemJunk)
                .result()
                .is_some()
        );
        assert_eq!(
            state.category(CleanerCategory::SystemJunk).scan_state(),
            ScanState::Completed
        );
    }

    #[test]
    fn two_categories_can_be_scanning_at_once() {
        let mut state = CleanerState::default();
        state.begin_scan(CleanerCategory::SystemJunk);
        state.begin_scan(CleanerCategory::DockerCache);

        assert_eq!(
            state.category(CleanerCategory::SystemJunk).scan_state(),
            ScanState::Scanning
        );
        assert_eq!(
            state.category(CleanerCategory::DockerCache).scan_state(),
            ScanState::Scanning
        );
    }

    #[test]
    fn cancelling_one_category_does_not_cancel_another() {
        let mut state = CleanerState::default();
        state.begin_scan(CleanerCategory::SystemJunk);
        state.begin_scan(CleanerCategory::DockerCache);

        state.begin_cancelling(CleanerCategory::SystemJunk);

        assert_eq!(
            state.category(CleanerCategory::SystemJunk).scan_state(),
            ScanState::Cancelling
        );
        assert_eq!(
            state.category(CleanerCategory::DockerCache).scan_state(),
            ScanState::Scanning,
            "cancelling System Junk must not touch Docker Cache's own scan"
        );
    }

    #[test]
    fn select_safe_never_selects_a_non_safe_item() {
        let mut state = CleanerState::default();
        let category = CleanerCategory::UserCache;
        let safe = sample_item(1, category, RiskLevel::SafeRecreatable);
        let risky = sample_item(2, category, RiskLevel::ReviewRecommended);
        state.finish_scan(
            category,
            Some(complete_result(category, vec![safe, risky])),
            false,
            None,
        );
        state.clear_selection(category);

        state.select_safe_items(category);

        let selected = state.category(category).selected_ids();
        assert_eq!(selected, vec![CleanableItemId(1)]);
    }

    #[test]
    fn selected_count_and_reclaimable_bytes_are_correct() {
        let mut state = CleanerState::default();
        let category = CleanerCategory::UserCache;
        state.finish_scan(
            category,
            Some(complete_result(
                category,
                vec![
                    sample_item(1, category, RiskLevel::SafeRecreatable),
                    sample_item(2, category, RiskLevel::SafeRecreatable),
                ],
            )),
            false,
            None,
        );

        assert_eq!(state.category(category).selected_count(), 2);
        assert_eq!(state.category(category).selected_reclaimable_bytes(), 20);
    }

    #[test]
    fn header_select_all_only_selects_items_with_move_to_trash() {
        let mut state = CleanerState::default();
        let category = CleanerCategory::UserCache;
        let mut read_only = sample_item(2, category, RiskLevel::SafeRecreatable);
        read_only.capabilities = vec![ItemCapability::CopyPath];
        state.finish_scan(
            category,
            Some(complete_result(
                category,
                vec![
                    sample_item(1, category, RiskLevel::SafeRecreatable),
                    read_only,
                ],
            )),
            false,
            None,
        );
        state.clear_selection(category);

        state.select_all(category);

        assert_eq!(
            state.category(category).selected_ids(),
            vec![CleanableItemId(1)]
        );
    }

    #[test]
    fn a_cancelled_scan_does_not_appear_completed() {
        let mut state = CleanerState::default();
        let category = CleanerCategory::SystemJunk;
        state.begin_scan(category);
        state.begin_cancelling(category);
        state.finish_scan(category, None, true, None);

        assert_eq!(state.category(category).scan_state(), ScanState::Cancelled);
    }

    #[test]
    fn a_failed_scan_does_not_appear_completed() {
        let mut state = CleanerState::default();
        let category = CleanerCategory::SystemJunk;
        state.begin_scan(category);
        state.finish_scan(category, None, false, Some("boom".to_string()));

        assert_eq!(state.category(category).scan_state(), ScanState::Failed);
    }

    #[test]
    fn a_fresh_scan_replaces_stale_results_rather_than_mixing_them() {
        let mut state = CleanerState::default();
        let category = CleanerCategory::SystemJunk;
        state.finish_scan(
            category,
            Some(complete_result(
                category,
                vec![sample_item(1, category, RiskLevel::SafeRecreatable)],
            )),
            false,
            None,
        );

        // A rescan starts: the old result must stay put until the new one
        // actually lands (req #20) — not disappear mid-scan.
        state.begin_scan(category);
        assert!(state.category(category).result().is_some());

        state.finish_scan(
            category,
            Some(complete_result(
                category,
                vec![sample_item(2, category, RiskLevel::SafeRecreatable)],
            )),
            false,
            None,
        );

        let result = state.category(category).result().expect("has a result");
        assert_eq!(result.items.len(), 1);
        assert_eq!(result.items[0].id, CleanableItemId(2));
    }

    #[test]
    fn permission_denied_scans_are_partially_completed() {
        let mut state = CleanerState::default();
        let category = CleanerCategory::MailFiles;
        state.finish_scan(
            category,
            Some(CategoryScanResult {
                category,
                items: Vec::new(),
                scanned_entries: 0,
                estimated_reclaimable_bytes: 0,
                warnings: Vec::new(),
                completeness: ScanCompleteness::Partial {
                    skipped_roots: vec!["/tmp/mail".into()],
                    reason: PartialScanReason::PermissionDenied,
                },
            }),
            false,
            None,
        );

        assert_eq!(
            state.category(category).scan_state(),
            ScanState::PartiallyCompleted
        );
    }

    #[test]
    fn remove_item_hides_it_immediately_without_a_rescan() {
        let mut state = CleanerState::default();
        let category = CleanerCategory::OrphanedFiles;
        state.finish_scan(
            category,
            Some(complete_result(
                category,
                vec![sample_item(7, category, RiskLevel::ReviewRecommended)],
            )),
            false,
            None,
        );
        assert_eq!(state.category(category).selected_count(), 0);

        state.remove_item(category, CleanableItemId(7));

        assert!(
            state
                .category(category)
                .result()
                .expect("category still has a result")
                .items
                .is_empty(),
            "the kept item must be gone from the visible results"
        );
    }

    /// The two revisions are what `views::results_sync` compares to decide
    /// whether the results grid needs a fresh deep copy of the whole result.
    /// Every assertion below is really a statement about that: what must
    /// force a re-copy, and — the expensive half — what must not.
    #[test]
    fn a_scan_in_flight_never_bumps_the_result_revision() {
        let mut state = CleanerState::default();
        let category = CleanerCategory::UserCache;
        state.finish_scan(
            category,
            Some(complete_result(
                category,
                vec![sample_item(1, category, RiskLevel::SafeRecreatable)],
            )),
            false,
            None,
        );
        let after_scan = state.category(category).result_revision();

        // A rescan keeps the previous result on screen, so nothing the scan
        // does before it lands changes a single row.
        state.begin_scan(category);
        state.update_progress(
            category,
            ScanProgress {
                category,
                phase: ScanPhase::Traversing,
                current_path: None,
                scanned_entries: 5_000,
                discovered_items: 12,
                discovered_bytes: 4096,
            },
        );
        state.begin_cancelling(category);
        state.begin_cleaning(category);

        assert_eq!(state.category(category).result_revision(), after_scan);
    }

    #[test]
    fn a_landed_result_bumps_both_revisions_and_a_barren_one_bumps_neither() {
        let mut state = CleanerState::default();
        let category = CleanerCategory::SystemJunk;
        let before = (
            state.category(category).result_revision(),
            state.category(category).selection_revision(),
        );

        state.finish_scan(
            category,
            Some(complete_result(
                category,
                vec![sample_item(1, category, RiskLevel::SafeRecreatable)],
            )),
            false,
            None,
        );
        let landed = (
            state.category(category).result_revision(),
            state.category(category).selection_revision(),
        );
        assert!(landed.0 > before.0 && landed.1 > before.1);

        // Cancelled and failed scans replace no item and reset no selection.
        state.finish_scan(category, None, true, None);
        state.finish_scan(category, None, false, Some("boom".to_string()));
        assert_eq!(
            (
                state.category(category).result_revision(),
                state.category(category).selection_revision()
            ),
            landed
        );
    }

    #[test]
    fn selection_changes_bump_only_the_selection_revision() {
        let mut state = CleanerState::default();
        let category = CleanerCategory::UserCache;
        state.finish_scan(
            category,
            Some(complete_result(
                category,
                vec![
                    sample_item(1, category, RiskLevel::SafeRecreatable),
                    sample_item(2, category, RiskLevel::ReviewRecommended),
                ],
            )),
            false,
            None,
        );
        let result_revision = state.category(category).result_revision();
        let mut selection_revision = state.category(category).selection_revision();

        for change in [
            CleanerState::clear_selection as fn(&mut CleanerState, CleanerCategory),
            CleanerState::select_all,
            CleanerState::select_safe_items,
        ] {
            change(&mut state, category);
            assert!(
                state.category(category).selection_revision() > selection_revision,
                "every selection change must be visible to the results grid"
            );
            selection_revision = state.category(category).selection_revision();
            assert_eq!(
                state.category(category).result_revision(),
                result_revision,
                "ticking rows must never force a re-copy of the items"
            );
        }

        state.toggle_selected(category, CleanableItemId(2));
        assert!(state.category(category).selection_revision() > selection_revision);
        assert_eq!(state.category(category).result_revision(), result_revision);
    }

    #[test]
    fn removing_an_item_bumps_the_result_revision() {
        let mut state = CleanerState::default();
        let category = CleanerCategory::OrphanedFiles;
        state.finish_scan(
            category,
            Some(complete_result(
                category,
                vec![sample_item(7, category, RiskLevel::ReviewRecommended)],
            )),
            false,
            None,
        );
        let before = state.category(category).result_revision();

        state.remove_item(category, CleanableItemId(7));

        assert!(state.category(category).result_revision() > before);
    }

    #[test]
    fn a_cleanup_that_removed_rows_bumps_the_result_revision() {
        let mut state = CleanerState::default();
        let category = CleanerCategory::UserCache;
        state.finish_scan(
            category,
            Some(complete_result(
                category,
                vec![sample_item(1, category, RiskLevel::SafeRecreatable)],
            )),
            false,
            None,
        );
        let before = state.category(category).result_revision();

        state.begin_cleaning(category);
        assert_eq!(
            state.category(category).result_revision(),
            before,
            "starting a cleanup removes no row"
        );

        state.finish_cleaning(
            category,
            CleanupReport {
                successes: vec![CleanupItemSuccess {
                    id: CleanableItemId(1),
                    path: "/tmp/item-1".into(),
                    trashed_path: None,
                    logical_size: 10,
                }],
                failures: Vec::new(),
                estimated_reclaimed_bytes: 10,
            },
        );

        assert!(state.category(category).result_revision() > before);
        assert!(
            state
                .category(category)
                .result()
                .expect("result kept")
                .items
                .is_empty()
        );
    }

    #[test]
    fn one_categorys_revisions_are_not_anothers() {
        let mut state = CleanerState::default();
        let scanned = CleanerCategory::UserCache;
        let untouched = CleanerCategory::DockerCache;
        state.finish_scan(
            scanned,
            Some(complete_result(
                scanned,
                vec![sample_item(1, scanned, RiskLevel::SafeRecreatable)],
            )),
            false,
            None,
        );
        state.toggle_selected(scanned, CleanableItemId(1));

        assert_eq!(state.category(untouched).result_revision(), 0);
        assert_eq!(state.category(untouched).selection_revision(), 0);
    }

    #[test]
    fn progress_updates_are_scoped_to_their_own_category() {
        let mut state = CleanerState::default();
        state.begin_scan(CleanerCategory::SystemJunk);
        state.update_progress(
            CleanerCategory::SystemJunk,
            ScanProgress {
                category: CleanerCategory::SystemJunk,
                phase: ScanPhase::Traversing,
                current_path: None,
                scanned_entries: 10,
                discovered_items: 2,
                discovered_bytes: 2048,
            },
        );

        assert!(
            state
                .category(CleanerCategory::SystemJunk)
                .progress()
                .is_some()
        );
        assert!(
            state
                .category(CleanerCategory::DockerCache)
                .progress()
                .is_none()
        );
    }
}
