//! Whether the results grid needs a fresh copy of the active category's data
//! — and how much of it.
//!
//! [`super::CleanerView::sync_results_table`] copies the active category's
//! items and selection into the [`super::results_table::ResultsTableDelegate`]
//! at the top of every `render`. That copy is a deep clone of every
//! [`CleanableItem`](crate::core::item::CleanableItem) in the
//! result — display name, path, explanation, capability list and, for an
//! application row, its icon — so it is O(items) in time, and it was being
//! paid on every frame. When this module was written the icon made it
//! O(bytes) too; `core::icon` has since put that payload behind a shared
//! handle, so a copy now costs a reference count rather than the icon. Both
//! halves are needed and both are tested below: the plan stops the copy
//! happening per frame, the handle stops the one copy duplicating megabytes.
//!
//! Every frame is far more frames than it sounds. GPUI caches a view's
//! element tree, but a dirty view marks its whole ancestor path dirty and an
//! ancestor re-render sets `Window::refreshing`, which makes every descendant
//! view re-render too. The results table is a *child* view of `CleanerView`,
//! so scrolling it, hovering a row, or resizing a column re-renders
//! `CleanerView` — and a 120 ms scan-progress tick does the same while a
//! rescan keeps the previous result on screen. Measured on this machine over
//! a standalone `opt-level = 3` fixture of the same struct shape, because
//! dodo's own release profile is fat-LTO and was not rebuilt to time one
//! clone: 20,000 plain items cost 6.7 ms per frame to re-copy, and 2,000
//! application items carrying 64 KiB icons cost 21.8 ms — the whole 60 Hz
//! frame budget, spent copying data that had not changed. Twenty items cost
//! 5 µs, which is why the small lists nobody complained about felt fine.
//! (Those 64 KiB were an underestimate of what an icon then was: the real
//! payload measured 70.5 MiB per application. `core::icon` has the numbers.)
//!
//! The fix is not to copy less data — every row stays visible, every total
//! and warning stays exact — but to copy it only when it is actually
//! different. [`CategoryState`] stamps two revisions (see its field docs) and
//! this module is the pure decision over them: same category and same
//! revisions means the delegate already holds precisely this data, and the
//! cheapest correct thing to do is nothing at all. In dodo's own (debug)
//! test build, an unchanged frame over 20,000 items went from 14.0 ms to
//! 254 ns; what is left is a three-field comparison. None of this has been
//! confirmed against a running dodo on the captain's machine — see the
//! commit message for what is owed.
//!
//! Row rendering was already virtualized (`results_table`'s own module doc),
//! and that is exactly why the clone was the thing to find: the grid only
//! ever *drew* the visible rows, but it was handed all 20,000 of them again
//! every frame to draw twenty from.
//!
//! Deliberately kept free of GPUI so the whole decision table is a plain
//! unit test.

use crate::core::category::CleanerCategory;
use crate::state::CategoryState;

/// What the results delegate's current copy was made from. Two keys being
/// equal is the promise that re-copying would produce byte-identical data.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ResultsSyncKey {
    category: CleanerCategory,
    result_revision: u64,
    selection_revision: u64,
}

impl ResultsSyncKey {
    pub fn of(category: CleanerCategory, state: &CategoryState) -> Self {
        Self {
            category,
            result_revision: state.result_revision(),
            selection_revision: state.selection_revision(),
        }
    }
}

/// How much of the delegate a frame has to rewrite.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ResultsSyncPlan {
    /// Nothing changed since the last sync: leave the delegate alone. This
    /// is the ordinary frame — scrolling, hovering, a progress tick.
    UpToDate,
    /// The same result, a different selection: rewrite the selected-id set
    /// and leave the items (and their icon payloads) untouched.
    SelectionOnly,
    /// A different category, or a different result for this one: rewrite
    /// both, and refresh the columns, since the column set itself depends on
    /// the category.
    Everything,
}

/// The last-synced key. Owned by [`super::CleanerView`], one per view.
#[derive(Default)]
pub struct ResultsSync {
    last: Option<ResultsSyncKey>,
}

impl ResultsSync {
    /// Decides what this frame owes the delegate and records the key as
    /// synced. The caller must carry out whatever it returns — nothing else
    /// re-reads the key, so a skipped `Everything` would leave stale rows on
    /// screen until the next change.
    pub fn plan(&mut self, key: ResultsSyncKey) -> ResultsSyncPlan {
        let plan = match self.last {
            Some(last) if last == key => ResultsSyncPlan::UpToDate,
            Some(last)
                if last.category == key.category && last.result_revision == key.result_revision =>
            {
                ResultsSyncPlan::SelectionOnly
            }
            _ => ResultsSyncPlan::Everything,
        };
        self.last = Some(key);
        plan
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::{ResultsSync, ResultsSyncKey, ResultsSyncPlan};
    use crate::core::category::CleanerCategory;
    use crate::core::icon::IconRaster;
    use crate::core::item::{ApplicationMetadata, CleanableItem, CleanableItemId, ItemMetadata};
    use crate::core::progress::{ScanPhase, ScanProgress};
    use crate::core::report::{
        CategoryScanResult, CleanupItemSuccess, CleanupReport, ScanCompleteness,
    };
    use crate::core::risk::{ItemCapability, RiskLevel, SelectionPolicy};
    use crate::state::CleanerState;

    /// Big enough that a per-frame re-copy is the difference between a
    /// smooth panel and a stuttering one, small enough to stay a fast unit
    /// test. Paths are deliberately long and every string is unique: the
    /// cost this guards against is per-item heap traffic, so a fixture of
    /// 20,000 copies of one short shared path would not be the same test.
    const LARGE: u64 = 20_000;
    const SMALL: u64 = 20;
    /// The worst case measured over this machine's 98 real application
    /// bundles at `IconRaster::EDGE_PIXELS` — see `core::icon`.
    const REALISTIC_ICON_BYTES: usize = 7_745;

    fn icon_of(item: &CleanableItem) -> Option<&IconRaster> {
        match &item.metadata {
            ItemMetadata::Application(metadata) => metadata.icon.as_ref(),
            _ => None,
        }
    }

    fn item(ix: u64, category: CleanerCategory, icon_bytes: usize) -> CleanableItem {
        CleanableItem {
            id: CleanableItemId(ix),
            category,
            group: Some(format!("group-{}", ix % 64)),
            display_name: format!("very-long-display-name-for-item-number-{ix}.bundle"),
            path: format!(
                "/Users/captain/Library/Application Support/com.example.vendor{ix}/Caches/\
                 derived-data/a-rather-long-project-name-{ix}/Build/Intermediates.noindex/\
                 ArchiveIntermediates/target-{ix}/Objects-normal/arm64/module-{ix}.o"
            )
            .into(),
            logical_size: 4096 * (ix % 977 + 1),
            allocated_size: Some(8192),
            modified_at: None,
            last_accessed_at: None,
            risk: RiskLevel::SafeRecreatable,
            selection_policy: if ix.is_multiple_of(3) {
                SelectionPolicy::SelectedByDefault
            } else {
                SelectionPolicy::NotSelectedByDefault
            },
            capabilities: vec![ItemCapability::MoveToTrash, ItemCapability::CopyPath],
            explanation: format!("Recreatable build output discovered under item {ix}."),
            warnings: Vec::new(),
            metadata: if icon_bytes == 0 {
                ItemMetadata::Generic
            } else {
                ItemMetadata::Application(ApplicationMetadata {
                    bundle_id: Some(format!("com.example.vendor{ix}")),
                    team_id: Some("ABCDE12345".to_string()),
                    version: Some("1.2.3".to_string()),
                    executable: Some(format!("vendor{ix}")),
                    icon: IconRaster::new(vec![7u8; icon_bytes]),
                })
            },
        }
    }

    fn result(category: CleanerCategory, count: u64, icon_bytes: usize) -> CategoryScanResult {
        let items: Vec<CleanableItem> = (0..count)
            .map(|ix| item(ix, category, icon_bytes))
            .collect();
        CategoryScanResult {
            category,
            estimated_reclaimable_bytes: items.iter().map(|item| item.logical_size).sum(),
            scanned_entries: count,
            items,
            warnings: Vec::new(),
            completeness: ScanCompleteness::Complete,
        }
    }

    /// Stands in for `ResultsTableDelegate`, which cannot be built without a
    /// window: it holds exactly the two things the real delegate holds and
    /// counts how much full-data work the plan actually asked for.
    #[derive(Default)]
    struct MirrorDelegate {
        category: Option<CleanerCategory>,
        items: Vec<CleanableItem>,
        selected_ids: HashSet<CleanableItemId>,
        item_copies: usize,
        selection_copies: usize,
    }

    impl MirrorDelegate {
        /// The whole of `CleanerView::sync_results_table`, minus the entity
        /// plumbing: plan, then copy exactly what the plan asked for.
        fn sync(&mut self, sync: &mut ResultsSync, state: &CleanerState) -> ResultsSyncPlan {
            let category = state.selected_category();
            let category_state = state.category(category);
            let plan = sync.plan(ResultsSyncKey::of(category, category_state));
            match plan {
                ResultsSyncPlan::UpToDate => {}
                ResultsSyncPlan::SelectionOnly => {
                    self.selected_ids = category_state.selected_ids().into_iter().collect();
                    self.selection_copies += 1;
                }
                ResultsSyncPlan::Everything => {
                    self.category = Some(category);
                    self.items = category_state
                        .result()
                        .map(|result| result.items.clone())
                        .unwrap_or_default();
                    self.selected_ids = category_state.selected_ids().into_iter().collect();
                    self.item_copies += 1;
                    self.selection_copies += 1;
                }
            }
            plan
        }

        fn assert_mirrors(&self, state: &CleanerState) {
            let category = state.selected_category();
            let category_state = state.category(category);
            assert_eq!(self.category, Some(category));
            let expected: &[CleanableItem] = category_state
                .result()
                .map_or(&[], |result| result.items.as_slice());
            assert_eq!(
                self.items.len(),
                expected.len(),
                "every cleanable entry must reach the grid — nothing hidden, capped or paged"
            );
            assert_eq!(self.items, expected, "the grid's rows must be the result's");
            let expected_selection: HashSet<CleanableItemId> =
                category_state.selected_ids().into_iter().collect();
            assert_eq!(self.selected_ids, expected_selection);
        }
    }

    fn scanned(category: CleanerCategory, count: u64, icon_bytes: usize) -> CleanerState {
        let mut state = CleanerState::default();
        state.set_selected_category(category);
        state.begin_scan(category);
        state.finish_scan(
            category,
            Some(result(category, count, icon_bytes)),
            false,
            None,
        );
        state
    }

    #[test]
    fn the_first_frame_copies_everything() {
        let state = scanned(CleanerCategory::UserCache, SMALL, 0);
        let mut sync = ResultsSync::default();
        let mut delegate = MirrorDelegate::default();

        assert_eq!(
            delegate.sync(&mut sync, &state),
            ResultsSyncPlan::Everything
        );
        delegate.assert_mirrors(&state);
    }

    #[test]
    fn redrawing_an_unchanged_large_result_copies_nothing() {
        let state = scanned(CleanerCategory::UserCache, LARGE, 0);
        let mut sync = ResultsSync::default();
        let mut delegate = MirrorDelegate::default();
        delegate.sync(&mut sync, &state);

        // Scrolling, hovering a row, an unrelated view's notify: 200 frames
        // over the same data. Before this plan every one of them deep-cloned
        // all 20,000 items.
        for _ in 0..200 {
            assert_eq!(delegate.sync(&mut sync, &state), ResultsSyncPlan::UpToDate);
        }

        assert_eq!(delegate.item_copies, 1);
        assert_eq!(delegate.selection_copies, 1);
        delegate.assert_mirrors(&state);
    }

    #[test]
    fn a_rescans_progress_ticks_never_recopy_the_retained_result() {
        let category = CleanerCategory::LargeOldFiles;
        let mut state = scanned(category, LARGE, 0);
        let mut sync = ResultsSync::default();
        let mut delegate = MirrorDelegate::default();
        delegate.sync(&mut sync, &state);

        // A rescan deliberately keeps the previous result on screen, so the
        // items behind the scanning header are the same items throughout.
        state.begin_scan(category);
        assert_eq!(delegate.sync(&mut sync, &state), ResultsSyncPlan::UpToDate);
        for tick in 0..50 {
            state.update_progress(
                category,
                ScanProgress {
                    category,
                    phase: ScanPhase::Traversing,
                    current_path: None,
                    scanned_entries: tick * 1000,
                    discovered_items: tick,
                    discovered_bytes: tick * 4096,
                },
            );
            assert_eq!(delegate.sync(&mut sync, &state), ResultsSyncPlan::UpToDate);
        }
        assert_eq!(delegate.item_copies, 1);

        // The new result landing is the one frame that owes a copy.
        state.finish_scan(category, Some(result(category, 5, 0)), false, None);
        assert_eq!(
            delegate.sync(&mut sync, &state),
            ResultsSyncPlan::Everything
        );
        assert_eq!(delegate.item_copies, 2);
        delegate.assert_mirrors(&state);
    }

    #[test]
    fn toggling_one_checkbox_recopies_the_selection_and_not_the_items() {
        let category = CleanerCategory::UserCache;
        let mut state = scanned(category, LARGE, 0);
        let mut sync = ResultsSync::default();
        let mut delegate = MirrorDelegate::default();
        delegate.sync(&mut sync, &state);

        state.toggle_selected(category, CleanableItemId(1));
        assert_eq!(
            delegate.sync(&mut sync, &state),
            ResultsSyncPlan::SelectionOnly
        );
        assert!(delegate.selected_ids.contains(&CleanableItemId(1)));
        assert_eq!(delegate.item_copies, 1, "the items did not change");
        delegate.assert_mirrors(&state);

        state.select_all(category);
        assert_eq!(
            delegate.sync(&mut sync, &state),
            ResultsSyncPlan::SelectionOnly
        );
        assert_eq!(delegate.selected_ids.len(), LARGE as usize);
        assert_eq!(delegate.item_copies, 1);
        delegate.assert_mirrors(&state);

        state.clear_selection(category);
        assert_eq!(
            delegate.sync(&mut sync, &state),
            ResultsSyncPlan::SelectionOnly
        );
        assert!(delegate.selected_ids.is_empty());
        assert_eq!(delegate.item_copies, 1);

        state.select_safe_items(category);
        assert_eq!(
            delegate.sync(&mut sync, &state),
            ResultsSyncPlan::SelectionOnly
        );
        assert_eq!(delegate.item_copies, 1);
        delegate.assert_mirrors(&state);
    }

    #[test]
    fn selection_totals_survive_the_skipped_frames() {
        let category = CleanerCategory::UserCache;
        let state = scanned(category, SMALL, 0);
        let mut sync = ResultsSync::default();
        let mut delegate = MirrorDelegate::default();
        delegate.sync(&mut sync, &state);

        let count = state.category(category).selected_count();
        let bytes = state.category(category).selected_reclaimable_bytes();
        assert!(count > 0 && bytes > 0);

        for _ in 0..20 {
            delegate.sync(&mut sync, &state);
        }

        assert_eq!(state.category(category).selected_count(), count);
        assert_eq!(state.category(category).selected_reclaimable_bytes(), bytes);
        assert_eq!(
            state.category(category).selected_items().len(),
            count,
            "cleanup must still receive exactly the ticked items"
        );
        delegate.assert_mirrors(&state);
    }

    #[test]
    fn switching_category_and_back_always_recopies() {
        let first = CleanerCategory::UserCache;
        let second = CleanerCategory::SystemJunk;
        let mut state = CleanerState::default();
        state.finish_scan(first, Some(result(first, SMALL, 0)), false, None);
        state.finish_scan(second, Some(result(second, SMALL + 3, 0)), false, None);
        state.set_selected_category(first);

        let mut sync = ResultsSync::default();
        let mut delegate = MirrorDelegate::default();
        delegate.sync(&mut sync, &state);
        delegate.assert_mirrors(&state);

        // Two categories can hold the same revisions — they both start at
        // one finished scan — so the category has to be part of the key.
        state.set_selected_category(second);
        assert_eq!(
            delegate.sync(&mut sync, &state),
            ResultsSyncPlan::Everything
        );
        delegate.assert_mirrors(&state);
        assert_eq!(delegate.items.len(), SMALL as usize + 3);

        state.set_selected_category(first);
        assert_eq!(
            delegate.sync(&mut sync, &state),
            ResultsSyncPlan::Everything
        );
        delegate.assert_mirrors(&state);
        assert_eq!(delegate.items.len(), SMALL as usize);
    }

    #[test]
    fn keeping_an_item_recopies_the_shortened_result() {
        let category = CleanerCategory::OrphanedFiles;
        let mut state = scanned(category, SMALL, 0);
        let mut sync = ResultsSync::default();
        let mut delegate = MirrorDelegate::default();
        delegate.sync(&mut sync, &state);

        state.remove_item(category, CleanableItemId(3));
        assert_eq!(
            delegate.sync(&mut sync, &state),
            ResultsSyncPlan::Everything
        );
        assert_eq!(delegate.items.len(), SMALL as usize - 1);
        assert!(
            !delegate
                .items
                .iter()
                .any(|item| item.id == CleanableItemId(3))
        );
        delegate.assert_mirrors(&state);
    }

    #[test]
    fn a_finished_cleanup_recopies_the_shortened_result() {
        let category = CleanerCategory::UserCache;
        let mut state = scanned(category, SMALL, 0);
        let mut sync = ResultsSync::default();
        let mut delegate = MirrorDelegate::default();
        delegate.sync(&mut sync, &state);

        state.begin_cleaning(category);
        assert_eq!(
            delegate.sync(&mut sync, &state),
            ResultsSyncPlan::UpToDate,
            "starting a cleanup changes no row"
        );

        state.finish_cleaning(
            category,
            CleanupReport {
                successes: vec![CleanupItemSuccess {
                    id: CleanableItemId(0),
                    path: "/tmp/item-0".into(),
                    trashed_path: None,
                    logical_size: 4096,
                }],
                failures: Vec::new(),
                estimated_reclaimed_bytes: 4096,
            },
        );
        assert_eq!(
            delegate.sync(&mut sync, &state),
            ResultsSyncPlan::Everything
        );
        assert_eq!(delegate.items.len(), SMALL as usize - 1);
        delegate.assert_mirrors(&state);
    }

    /// The icon payload used to be the reason a *small* result could be as
    /// expensive to re-copy as a huge one. Two things now bound it, and this
    /// pins both: the plan means the copy happens once rather than per
    /// frame, and `IconRaster` means that one copy shares the bytes instead
    /// of duplicating them.
    #[test]
    fn icon_payloads_are_copied_once_not_once_per_frame() {
        let category = CleanerCategory::InstalledApps;
        let state = scanned(category, 400, REALISTIC_ICON_BYTES);
        let mut sync = ResultsSync::default();
        let mut delegate = MirrorDelegate::default();

        for _ in 0..100 {
            delegate.sync(&mut sync, &state);
        }

        assert_eq!(delegate.item_copies, 1);
        let carried: usize = delegate
            .items
            .iter()
            .filter_map(icon_of)
            .map(IconRaster::len)
            .sum();
        assert_eq!(
            carried,
            400 * REALISTIC_ICON_BYTES,
            "every icon still reaches the grid; it is just not re-copied per frame"
        );
        delegate.assert_mirrors(&state);
    }

    /// The memory half of the captain's report, stated where it actually
    /// bit: the grid's copy of an Installed Apps result must not be a second
    /// copy of its icons. Before `IconRaster`, `result.items.clone()` deep-
    /// copied a `Vec<u8>` per application — and that `Vec` held the whole
    /// `NSImage` TIFF representation, measured at 73,949,448 bytes each.
    #[test]
    fn the_grids_copy_shares_the_scans_icon_bytes_rather_than_duplicating_them() {
        const APPS: u64 = 400;
        let category = CleanerCategory::InstalledApps;
        let state = scanned(category, APPS, REALISTIC_ICON_BYTES);
        let mut sync = ResultsSync::default();
        let mut delegate = MirrorDelegate::default();
        delegate.sync(&mut sync, &state);

        let scanned_items = state
            .category(category)
            .result()
            .expect("a finished scan")
            .items
            .as_slice();
        assert_eq!(delegate.items.len(), scanned_items.len());

        let mut shared = 0;
        for (mirrored, original) in delegate.items.iter().zip(scanned_items) {
            let (Some(mirrored), Some(original)) = (icon_of(mirrored), icon_of(original)) else {
                panic!("every application row in this fixture carries an icon");
            };
            assert!(
                std::ptr::eq(mirrored.as_bytes(), original.as_bytes()),
                "the grid must point at the scan's icon bytes, not a copy of them"
            );
            shared += 1;
        }
        assert_eq!(shared, APPS as usize);

        // And the bound the type enforces, over the whole result: whatever
        // the icons are, this many applications cannot retain more than this.
        let total: usize = scanned_items
            .iter()
            .filter_map(icon_of)
            .map(IconRaster::len)
            .sum();
        assert!(total <= APPS as usize * IconRaster::MAX_BYTES);
    }

    #[test]
    fn a_failed_or_cancelled_scan_leaves_the_previous_rows_alone() {
        let category = CleanerCategory::UserCache;
        let mut state = scanned(category, SMALL, 0);
        let mut sync = ResultsSync::default();
        let mut delegate = MirrorDelegate::default();
        delegate.sync(&mut sync, &state);

        state.begin_scan(category);
        state.finish_scan(category, None, true, None);
        assert_eq!(
            delegate.sync(&mut sync, &state),
            ResultsSyncPlan::UpToDate,
            "a cancelled scan replaces no item, so the grid keeps what it has"
        );
        delegate.assert_mirrors(&state);
        assert_eq!(delegate.item_copies, 1);
    }
}
