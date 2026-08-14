use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::WindowExt as _;
use gpui_component::button::{Button, ButtonCustomVariant, ButtonVariant, ButtonVariants as _};
use gpui_component::dialog::DialogButtonProps;
use gpui_component::spinner::Spinner;
use gpui_component::table::{DataTable, TableState};
use gpui_component::{
    ActiveTheme, Disableable as _, Icon, Sizable as _, StyledExt as _, h_flex, v_flex,
};

use crate::app_icon::AppIcon;
use crate::cleaner::core::cancellation::CancellationToken;
use crate::cleaner::core::category::{CleanerCategory, CleanerSection};
use crate::cleaner::core::errors::{CleanupError, ScanError};
use crate::cleaner::core::ignore::{IgnoredItemsDocument, path_signature};
use crate::cleaner::core::item::{CleanableItem, CleanableItemId};
#[cfg(target_os = "linux")]
use crate::cleaner::core::item::{InstalledAppAction, ItemMetadata};
#[cfg(target_os = "macos")]
use crate::cleaner::core::permissions::{MacPermission, PermissionService, PermissionState};
use crate::cleaner::core::progress::LatestProgress;
#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
use crate::cleaner::core::report::CleanupReport;
use crate::cleaner::core::report::{CategoryScanResult, PartialScanReason, ScanCompleteness};
use crate::cleaner::core::risk::SelectionPolicy;
use crate::cleaner::core::scan_context::ScanContext;
use crate::cleaner::core::scan_state::{ScanIndicator, ScanState};
use crate::cleaner::core::scanner::CleanerScanner;
use crate::cleaner::docker_cache;
#[cfg(target_os = "macos")]
use crate::cleaner::macos::applications::review as uninstall_review;
#[cfg(target_os = "macos")]
use crate::cleaner::macos::{cleanup, permissions, platform};
use crate::cleaner::services::ignore_store::{
    DiskOrphanIgnoreStore, OrphanIgnoreStore, OrphanIgnoreStoreError,
};
use crate::cleaner::state::{CategoryState, CleanerState, default_scanners};
use crate::cleaner::views::results_sync::{ResultsSync, ResultsSyncKey, ResultsSyncPlan};
use crate::cleaner::views::results_table::{ResultsTableDelegate, category_icon};
#[cfg(target_os = "macos")]
use crate::cleaner::views::uninstall_review_dialog;
use crate::i18n::{Str, t};

pub struct CleanerView {
    /// Navigation (which category/sections are showing) and every category's
    /// own scan/selection/cleanup state — see the module doc on
    /// [`crate::cleaner::state::CleanerState`] for why those stay three
    /// independent axes rather than derived from one another.
    state: CleanerState,
    scanners: Vec<Arc<dyn CleanerScanner>>,
    #[cfg(target_os = "macos")]
    permission_service: Arc<dyn PermissionService>,
    /// One entry per category currently scanning — inserting a second
    /// category's entry never touches the first's. See `start_scan`/
    /// `run_scan`.
    scan_tasks: HashMap<CleanerCategory, Task<()>>,
    cancellations: HashMap<CleanerCategory, CancellationToken>,
    /// One capacity-one, latest-wins slot per scanning category — see
    /// [`LatestProgress`] for why an intermediate update may be dropped and
    /// why a scan's own outcome never travels this way.
    progress_slots: HashMap<CleanerCategory, Arc<LatestProgress>>,
    /// A single timer loop that takes each live slot's latest update each
    /// tick, rather than one timer per category — cheaper, and still fully
    /// independent per-category data (see `ensure_pump_running`).
    pump_task: Option<Task<()>>,
    cleanup_tasks: HashMap<CleanerCategory, Task<()>>,
    /// The Full Disk Access probe kicked off *at Scan-click time* for a
    /// category that needs it (req #16/#17) — never eagerly on `new`, and
    /// never a permanent panel. Empty on every platform but macOS.
    #[cfg(target_os = "macos")]
    permission_check_tasks: HashMap<CleanerCategory, Task<()>>,
    /// Where the orphan-detection "keep" list lives; see
    /// `crate::cleaner::services::ignore_store`. Not `#[cfg(target_os = "macos")]`
    /// like `permission_service` — the store itself is plain JSON I/O with no
    /// macOS API calls, only the `OrphanedFiles` items that ever carry
    /// `ItemCapability::MarkAsKept` are macOS-only.
    ignore_store: Arc<dyn OrphanIgnoreStore>,
    /// The loaded keep list, kept in memory so "Keep" does not need a
    /// load-modify-persist round trip through disk for every click.
    ignored_paths: BTreeSet<String>,
    ignore_load_task: Option<Task<()>>,
    /// What went wrong reading or writing `cleaner-ignored-items.json`, if
    /// anything. `None` in the ordinary case, including a first run with no
    /// file yet.
    ignore_store_error: Option<OrphanIgnoreStoreError>,
    /// The virtualized results grid for the active category. See
    /// `results_table`'s module doc for why it holds a `WeakEntity` back to
    /// this view rather than the other way around.
    results_table: Entity<TableState<ResultsTableDelegate>>,
    /// What [`Self::results_table`]'s delegate was last filled from, so a
    /// frame that changed nothing does not re-copy the whole result into it.
    /// See `views::results_sync` for the measurements behind that.
    results_sync: ResultsSync,
    /// Which categories' warnings block is expanded to its raw diagnostic
    /// detail. UI-only — not domain state, same footing as
    /// `CleanerState::expanded_sections`.
    expanded_warnings: HashSet<CleanerCategory>,
}

impl CleanerView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        // `cx.entity()` here is `Entity<CleanerView>` — the entity `new` is
        // building — grabbed *before* the inner `cx.new` call so the closure
        // that builds `results_table` (whose own `cx` is `Context<TableState<_>>`,
        // not this one) can hand `ResultsTableDelegate` a way back to it.
        let this = cx.entity().downgrade();
        let results_table = cx.new(|cx| {
            TableState::new(ResultsTableDelegate::new(this), window, cx).col_selectable(false)
        });
        let mut view = Self {
            state: CleanerState::default(),
            scanners: default_scanners(),
            #[cfg(target_os = "macos")]
            permission_service: permissions::default_service(),
            scan_tasks: HashMap::new(),
            cancellations: HashMap::new(),
            progress_slots: HashMap::new(),
            pump_task: None,
            cleanup_tasks: HashMap::new(),
            #[cfg(target_os = "macos")]
            permission_check_tasks: HashMap::new(),
            ignore_store: Arc::new(DiskOrphanIgnoreStore::new()),
            ignored_paths: BTreeSet::new(),
            ignore_load_task: None,
            ignore_store_error: None,
            results_table,
            results_sync: ResultsSync::default(),
            expanded_warnings: HashSet::new(),
        };
        view.load_ignored_paths(cx);
        view
    }

    fn supported_platform() -> bool {
        cfg!(any(
            target_os = "macos",
            target_os = "windows",
            target_os = "linux"
        ))
    }

    /// Copies the active category's current items and selection into
    /// [`Self::results_table`]'s delegate — but only the part of it that
    /// actually changed since the last frame.
    ///
    /// Round 1's version copied everything at the top of every `render`, on
    /// the reasoning that `render` only runs when something already changed.
    /// That reasoning does not hold: the results table is a *child view*, so
    /// its own scrolling and hovering re-render this view, as does any
    /// ancestor's redraw, as does a scan-progress tick every 120 ms while a
    /// rescan keeps the previous result on screen. None of those change a
    /// single row, and the copy is a deep clone of every item including its
    /// icon payload. `views::results_sync` carries the measurements and the
    /// decision table; here we only carry out what it decides.
    fn sync_results_table(&mut self, cx: &mut Context<Self>) {
        let category = self.state.selected_category();
        let category_state = self.state.category(category);
        let key = ResultsSyncKey::of(category, category_state);
        match self.results_sync.plan(key) {
            ResultsSyncPlan::UpToDate => {}
            ResultsSyncPlan::SelectionOnly => {
                let selected_ids: HashSet<CleanableItemId> =
                    category_state.selected_ids().into_iter().collect();
                self.results_table.update(cx, |table, _| {
                    table.delegate_mut().set_selection(selected_ids);
                });
            }
            ResultsSyncPlan::Everything => {
                let items = category_state
                    .result()
                    .map(|result| result.items.clone())
                    .unwrap_or_default();
                let selected_ids: HashSet<CleanableItemId> =
                    category_state.selected_ids().into_iter().collect();
                self.results_table.update(cx, |table, table_cx| {
                    table.delegate_mut().set(category, items, selected_ids);
                    table.refresh(table_cx);
                });
            }
        }
    }

    fn is_category_busy(&self, category: CleanerCategory) -> bool {
        let state = self.state.category(category);
        state.scan_state().is_active() || state.cleaning()
    }

    #[cfg(target_os = "macos")]
    fn is_permission_check_pending(&self, category: CleanerCategory) -> bool {
        self.permission_check_tasks.contains_key(&category)
    }

    #[cfg(not(target_os = "macos"))]
    fn is_permission_check_pending(&self, _category: CleanerCategory) -> bool {
        false
    }

    /// Loads `cleaner-ignored-items.json` on the background executor. Called
    /// once from [`Self::new`]; a failure leaves `ignored_paths` empty and
    /// records the error for the banner rather than blocking the rest of the
    /// panel — the same "fail closed, keep going" shape every other store in
    /// dodo uses.
    fn load_ignored_paths(&mut self, cx: &mut Context<Self>) {
        let store = self.ignore_store.clone();
        self.ignore_load_task = Some(cx.spawn(async move |this, cx| {
            let loaded = cx
                .background_executor()
                .spawn(async move { store.load() })
                .await;
            let _ = this.update(cx, |this, cx| {
                match loaded {
                    Ok(document) => {
                        this.ignored_paths = document.ignored_paths;
                        this.ignore_store_error = None;
                    }
                    Err(error) => this.ignore_store_error = Some(error),
                }
                this.ignore_load_task = None;
                cx.notify();
            });
        }));
    }

    /// "Keep": marks an orphan-detection candidate as reviewed and excluded
    /// from future scans (Phase 10). Removes the item from view immediately
    /// — [`CleanerState::remove_item`] — rather than waiting on the save,
    /// and persists the updated keep list on the background executor
    /// afterwards. A path already in the list (should not normally happen,
    /// since the item would already be gone from view) is a no-op: nothing
    /// new to persist.
    pub(super) fn mark_kept(&mut self, item: CleanableItem, cx: &mut Context<Self>) {
        if !self
            .ignored_paths
            .insert(path_signature(item.path.as_path()))
        {
            return;
        }
        self.state.remove_item(item.category, item.id);
        cx.notify();

        let document = IgnoredItemsDocument {
            ignored_paths: self.ignored_paths.clone(),
            ..IgnoredItemsDocument::default()
        };
        let store = self.ignore_store.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { store.persist(&document) })
                .await;
            if let Err(error) = result {
                let _ = this.update(cx, |this, cx| {
                    this.ignore_store_error = Some(error);
                    cx.notify();
                });
            }
        })
        .detach();
    }

    fn set_selected_category(&mut self, category: CleanerCategory, cx: &mut Context<Self>) {
        self.state.set_selected_category(category);
        cx.notify();
    }

    /// Starts (or restarts) one category's scan. Never touches any other
    /// category's `scan_tasks`/`cancellations`/`progress_slots` entry — that
    /// is what lets two categories scan at once (req #5/#23). A category
    /// requiring Full Disk Access gets a contextual, click-time-only check
    /// (req #16/#17) rather than a permanently rendered panel.
    fn start_scan(
        &mut self,
        category: CleanerCategory,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !Self::supported_platform() || self.is_category_busy(category) {
            return;
        }
        if self.is_permission_check_pending(category) {
            return;
        }

        let Some(scanner) = self
            .scanners
            .iter()
            .find(|scanner| scanner.category() == category)
            .cloned()
        else {
            self.state
                .finish_scan(category, Some(Self::pending_result(category)), false, None);
            cx.notify();
            return;
        };

        #[cfg(target_os = "macos")]
        {
            if scanner
                .required_permissions()
                .contains(&MacPermission::FullDiskAccess)
            {
                self.check_permission_then_scan(category, scanner, window, cx);
                return;
            }
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = &window;
        }
        self.run_scan(category, scanner, cx);
    }

    /// Probes Full Disk Access on the background executor, then either
    /// scans (Granted/Restricted/Unknown — the scanner's own per-root
    /// handling already degrades gracefully, per req #16's last paragraph)
    /// or shows a focused prompt (only on a clear Denied).
    #[cfg(target_os = "macos")]
    fn check_permission_then_scan(
        &mut self,
        category: CleanerCategory,
        scanner: Arc<dyn CleanerScanner>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let service = self.permission_service.clone();
        self.permission_check_tasks.insert(
            category,
            cx.spawn_in(window, async move |this, cx| {
                let result = cx
                    .background_executor()
                    .spawn(async move { service.check_full_disk_access() })
                    .await;
                let _ = this.update_in(cx, |this, window, cx| {
                    this.permission_check_tasks.remove(&category);
                    if matches!(result, Ok(PermissionState::Denied)) {
                        this.show_permission_prompt(window, cx);
                    } else {
                        this.run_scan(category, scanner, cx);
                    }
                });
            }),
        );
    }

    /// The contextual Full Disk Access prompt (req #16): a transient dialog
    /// raised only when a scan actually needs the permission and it is
    /// actually denied — never a panel occupying the main content area.
    #[cfg(target_os = "macos")]
    fn show_permission_prompt(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let service = self.permission_service.clone();
        window.open_alert_dialog(cx, move |alert, _, cx| {
            let service = service.clone();
            alert
                .title(t(Str::CleanerPermissionTitle, cx))
                .description(t(Str::CleanerPermissionExplanation, cx))
                .button_props(
                    DialogButtonProps::default()
                        .ok_text(t(Str::CleanerPermissionOpenSettings, cx))
                        .cancel_text(t(Str::CleanerPermissionNotNow, cx))
                        .show_cancel(true),
                )
                .on_ok(move |_, _, _cx| {
                    let _ = service.open_full_disk_access_settings();
                    true
                })
        });
    }

    fn run_scan(
        &mut self,
        category: CleanerCategory,
        scanner: Arc<dyn CleanerScanner>,
        cx: &mut Context<Self>,
    ) {
        let cancellation = CancellationToken::new();
        let cancellation_for_scan = cancellation.clone();
        let sink = Arc::new(LatestProgress::new());
        let sink_for_scan = sink.clone();
        self.cancellations.insert(category, cancellation);
        self.progress_slots.insert(category, sink);
        self.state.begin_scan(category);
        cx.notify();
        self.ensure_pump_running(cx);

        let task = cx.spawn(async move |this, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move {
                    let context = ScanContext::new();
                    match scanner.scan(&context, sink_for_scan.as_ref(), &cancellation_for_scan) {
                        Ok(result) => (Some(result), false, None),
                        Err(ScanError::Cancelled) => (None, true, None),
                        Err(error) => (None, false, Some(format!("{error:?}"))),
                    }
                })
                .await;

            let _ = this.update(cx, |this, cx| {
                let (result, cancelled, error) = outcome;
                this.state.finish_scan(category, result, cancelled, error);
                this.cancellations.remove(&category);
                this.progress_slots.remove(&category);
                this.scan_tasks.remove(&category);
                cx.notify();
            });
        });
        self.scan_tasks.insert(category, task);
    }

    /// A single background loop that takes each category's latest progress
    /// each tick, rather than one timer per category. Stops itself once
    /// `progress_slots` is empty, so it costs nothing while nothing is
    /// scanning, and one category finishing early never stops another's
    /// updates from still being pumped.
    fn ensure_pump_running(&mut self, cx: &mut Context<Self>) {
        if self.pump_task.is_some() {
            return;
        }
        self.pump_task = Some(cx.spawn(async move |this, cx| {
            loop {
                let has_active = this
                    .update(cx, |this, cx| {
                        if apply_latest_progress(&mut this.state, &this.progress_slots) {
                            cx.notify();
                        }
                        !this.progress_slots.is_empty()
                    })
                    .unwrap_or(false);

                if !has_active {
                    break;
                }
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(120))
                    .await;
            }
            let _ = this.update(cx, |this, _cx| {
                this.pump_task = None;
            });
        }));
    }

    fn cancel_scan(&mut self, category: CleanerCategory, cx: &mut Context<Self>) {
        if let Some(cancellation) = self.cancellations.get(&category) {
            cancellation.cancel();
            self.state.begin_cancelling(category);
            cx.notify();
        }
    }

    pub(super) fn toggle_selected(&mut self, id: CleanableItemId, cx: &mut Context<Self>) {
        self.state
            .toggle_selected(self.state.selected_category(), id);
        cx.notify();
    }

    /// The results table's header checkbox, unchecked or indeterminate:
    /// selects every selectable row in the active category. See
    /// `CleanerState::select_all` for the `MoveToTrash` gate.
    pub(super) fn select_all_visible(&mut self, cx: &mut Context<Self>) {
        self.state.select_all(self.state.selected_category());
        cx.notify();
    }

    /// The results table's header checkbox, fully checked: clears the whole
    /// selection for the active category.
    pub(super) fn deselect_all(&mut self, cx: &mut Context<Self>) {
        self.state.clear_selection(self.state.selected_category());
        cx.notify();
    }

    fn select_safe_items(&mut self, category: CleanerCategory, cx: &mut Context<Self>) {
        self.state.select_safe_items(category);
        cx.notify();
    }

    pub(super) fn reveal_in_finder(
        &mut self,
        path: std::path::PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        #[cfg(target_os = "macos")]
        let result = platform::reveal_in_finder(path.as_path());
        #[cfg(target_os = "windows")]
        let result = crate::cleaner::windows::platform::reveal_in_explorer(path.as_path());
        #[cfg(target_os = "linux")]
        let result = crate::cleaner::linux::platform::reveal_in_file_manager(path.as_path());
        #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
        let result: Result<(), String> = Ok(());

        if let Err(error) = result {
            window.open_alert_dialog(cx, move |alert, _, cx| {
                alert
                    .title(t(Str::CleanerStatusFailed, cx))
                    .description(error.clone())
            });
        }
    }

    fn confirm_empty_trash(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(result) = self.state.category(CleanerCategory::TrashBins).result() else {
            return;
        };
        let items = result.items.clone();
        if items.is_empty() {
            return;
        }
        let count = result.scanned_entries;
        let size = Self::format_bytes(result.estimated_reclaimable_bytes);
        let view = cx.entity();
        window.open_alert_dialog(cx, move |alert, _, cx| {
            let confirm_view = view.clone();
            let items = items.clone();
            alert
                .title(t(Str::CleanerEmptyTrashConfirmTitle, cx))
                .description(t(
                    Str::CleanerEmptyTrashConfirmMessage {
                        count,
                        size: size.clone(),
                    },
                    cx,
                ))
                .button_props(
                    DialogButtonProps::default()
                        .ok_text(t(Str::CleanerEmptyTrash, cx))
                        .ok_variant(ButtonVariant::Danger)
                        .cancel_text(t(Str::CleanerCancelScan, cx))
                        .show_cancel(true),
                )
                .on_ok(move |_, _, cx| {
                    confirm_view.update(cx, |this, cx| this.run_empty_trash(items.clone(), cx));
                    true
                })
        });
    }

    fn confirm_cleanup(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let selected = self.selected_items_for_active_category();
        if selected.is_empty() {
            return;
        }
        let is_docker = self.state.selected_category() == CleanerCategory::DockerCache;
        let count = selected.len();
        let size = Self::format_bytes(selected.iter().map(|item| item.logical_size).sum());
        let view = cx.entity();
        window.open_alert_dialog(cx, move |alert, _window, cx| {
            let confirm_view = view.clone();
            let (title, description) = if is_docker {
                (
                    t(Str::CleanerDockerCleanupConfirmTitle, cx),
                    t(
                        Str::CleanerDockerCleanupConfirmMessage {
                            count,
                            size: size.clone(),
                        },
                        cx,
                    ),
                )
            } else {
                (
                    t(Str::CleanerCleanupConfirmTitle, cx),
                    t(
                        Str::CleanerCleanupConfirmMessage {
                            count,
                            size: size.clone(),
                        },
                        cx,
                    ),
                )
            };
            alert
                .title(title)
                .description(description)
                .button_props(
                    DialogButtonProps::default()
                        .ok_text(t(Str::CleanerCleanSelected, cx))
                        .ok_variant(ButtonVariant::Danger)
                        .cancel_text(t(Str::CleanerCancelScan, cx))
                        .show_cancel(true),
                )
                .on_ok(move |_, _, cx| {
                    confirm_view.update(cx, |this, cx| this.start_cleanup(cx));
                    true
                })
        });
    }

    fn start_cleanup(&mut self, cx: &mut Context<Self>) {
        let items = self.selected_items_for_active_category();
        self.run_cleanup(items, cx);
    }

    /// Runs the shared Trash-move pipeline over an explicit item list rather
    /// than the active category's selection. `pub(super)` so the uninstall
    /// review dialog (`views::uninstall_review_dialog`) can hand it the app
    /// bundle plus whichever leftover candidates the user checked, without
    /// those items ever needing to pass through `CleanerState`'s normal
    /// per-category selection first.
    pub(super) fn start_uninstall_cleanup(
        &mut self,
        items: Vec<CleanableItem>,
        cx: &mut Context<Self>,
    ) {
        self.run_cleanup(items, cx);
    }

    /// Cleans exactly the category the items belong to (every caller hands
    /// this a single-category list) and never touches any other category's
    /// `cleaning`/`cleanup_report` — a cleanup running for one category never
    /// blocks scanning or viewing another.
    fn run_empty_trash(&mut self, items: Vec<CleanableItem>, cx: &mut Context<Self>) {
        if self.cleanup_tasks.contains_key(&CleanerCategory::TrashBins) {
            return;
        }
        self.state.begin_cleaning(CleanerCategory::TrashBins);
        cx.notify();
        let task = cx.spawn(async move |this, cx| {
            let report = cx
                .background_executor()
                .spawn(async move {
                    #[cfg(target_os = "macos")]
                    {
                        cleanup::empty_trash_items(&items)
                    }
                    #[cfg(not(target_os = "macos"))]
                    {
                        let _ = items;
                        crate::cleaner::core::report::CleanupReport {
                            successes: Vec::new(),
                            failures: Vec::new(),
                            estimated_reclaimed_bytes: 0,
                        }
                    }
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.state
                    .finish_cleaning(CleanerCategory::TrashBins, report);
                this.cleanup_tasks.remove(&CleanerCategory::TrashBins);
                cx.notify();
            });
        });
        self.cleanup_tasks.insert(CleanerCategory::TrashBins, task);
    }

    fn run_cleanup(&mut self, items: Vec<CleanableItem>, cx: &mut Context<Self>) {
        let Some(category) = items.first().map(|item| item.category) else {
            return;
        };
        if self.cleanup_tasks.contains_key(&category) {
            return;
        }
        self.state.begin_cleaning(category);
        cx.notify();

        let is_docker = category == CleanerCategory::DockerCache;

        let task = cx.spawn(async move |this, cx| {
            let report = cx
                .background_executor()
                .spawn(async move {
                    if is_docker {
                        docker_cache::prune_items(&items)
                    } else {
                        #[cfg(target_os = "macos")]
                        {
                            cleanup::cleanup_items(&items)
                        }
                        #[cfg(target_os = "windows")]
                        {
                            crate::cleaner::windows::cleanup::cleanup_items(&items)
                        }
                        #[cfg(target_os = "linux")]
                        {
                            crate::cleaner::linux::cleanup::cleanup_items(&items)
                        }
                        #[cfg(not(any(
                            target_os = "macos",
                            target_os = "windows",
                            target_os = "linux"
                        )))]
                        {
                            let _ = items;
                            CleanupReport {
                                successes: Vec::new(),
                                failures: Vec::new(),
                                estimated_reclaimed_bytes: 0,
                            }
                        }
                    }
                })
                .await;

            let _ = this.update(cx, |this, cx| {
                this.state.finish_cleaning(category, report);
                this.cleanup_tasks.remove(&category);
                cx.notify();
            });
        });
        self.cleanup_tasks.insert(category, task);
    }

    pub(super) fn begin_uninstall_review(
        &mut self,
        item: CleanableItem,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        #[cfg(target_os = "macos")]
        {
            let other_apps = self
                .state
                .category(CleanerCategory::InstalledApps)
                .result()
                .map(|result| {
                    result
                        .items
                        .iter()
                        .filter(|other| other.id != item.id)
                        .filter_map(uninstall_review::identity_for)
                        .collect()
                })
                .unwrap_or_default();
            let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
            uninstall_review_dialog::open(cx.entity(), item, other_apps, home, window, cx);
        }
        #[cfg(target_os = "windows")]
        {
            // The selected row deliberately does not become a command
            // argument. Windows owns app selection and vendor invocation.
            let _ = item;
            if let Err(error) = crate::cleaner::windows::platform::open_installed_apps_settings() {
                window.open_alert_dialog(cx, move |alert, _, cx| {
                    alert
                        .title(t(Str::CleanerStatusFailed, cx))
                        .description(error.clone())
                });
            }
        }
        #[cfg(target_os = "linux")]
        {
            let ItemMetadata::InstalledApp(metadata) = &item.metadata else {
                return;
            };
            let Some(action) = metadata.action.as_ref() else {
                return;
            };
            let ok_text = match action {
                InstalledAppAction::FlatpakUser { .. } => Str::CleanerUninstallApplication,
                InstalledAppAction::AppImage => Str::CleanerUninstallMoveToTrash,
            };
            let title = Str::CleanerUninstallReviewTitle {
                name: item.display_name.clone(),
            };
            let view = cx.entity();
            window.open_alert_dialog(cx, move |alert, _, cx| {
                let confirm_view = view.clone();
                let item = item.clone();
                alert
                    .title(t(title.clone(), cx))
                    .button_props(
                        DialogButtonProps::default()
                            .ok_text(t(ok_text.clone(), cx))
                            .ok_variant(ButtonVariant::Danger)
                            .cancel_text(t(Str::CleanerCancelScan, cx))
                            .show_cancel(true),
                    )
                    .on_ok(move |_, _, cx| {
                        confirm_view
                            .update(cx, |this, cx| this.run_cleanup(vec![item.clone()], cx));
                        true
                    })
            });
        }
    }

    fn selected_items_for_active_category(&self) -> Vec<CleanableItem> {
        self.state
            .category(self.state.selected_category())
            .selected_items()
    }

    /// `pub(super)` rather than private: the uninstall review dialog
    /// (`views::uninstall_review_dialog`, a sibling module) renders sizes the
    /// same way this panel does and should not grow a second formatter.
    pub(super) fn format_bytes(bytes: u64) -> String {
        const KIB: u64 = 1024;
        const MIB: u64 = 1024 * KIB;
        const GIB: u64 = 1024 * MIB;
        if bytes >= GIB {
            format!("{:.1} GiB", bytes as f64 / GIB as f64)
        } else if bytes >= MIB {
            format!("{:.1} MiB", bytes as f64 / MIB as f64)
        } else if bytes >= KIB {
            format!("{:.1} KiB", bytes as f64 / KIB as f64)
        } else {
            format!("{bytes} B")
        }
    }

    /// "· 2.4s" beside the completed summary's stat chips (req #8's "scan
    /// duration if useful") — `None` when either timestamp is missing (a
    /// category that has never finished a scan) or the clock went backwards
    /// (never trusted over a zero/negative duration).
    fn scan_duration_label(state: &CategoryState) -> Option<String> {
        let started = state.started_at()?;
        let finished = state.finished_at()?;
        let duration = finished.duration_since(started).ok()?;
        Some(format!("· {:.1}s", duration.as_secs_f64()))
    }

    fn section_label(section: CleanerSection) -> Str {
        match section {
            CleanerSection::Cleanup => Str::CleanerSectionCleanup,
            CleanerSection::Applications => Str::CleanerSectionApplications,
            CleanerSection::Advanced => Str::CleanerSectionAdvanced,
        }
    }

    fn section_icon(section: CleanerSection) -> AppIcon {
        match section {
            CleanerSection::Cleanup => AppIcon::BrushCleaning,
            CleanerSection::Applications => AppIcon::LayoutDashboard,
            CleanerSection::Advanced => AppIcon::Sliders,
        }
    }

    fn category_label(category: CleanerCategory) -> Str {
        match category {
            CleanerCategory::SystemJunk => Str::CleanerCategorySystemJunk,
            CleanerCategory::UserCache => Str::CleanerCategoryUserCache,
            CleanerCategory::MailFiles => Str::CleanerCategoryMailFiles,
            CleanerCategory::TrashBins => Str::CleanerCategoryTrashBins,
            CleanerCategory::LargeOldFiles => Str::CleanerCategoryLargeOldFiles,
            CleanerCategory::InstalledApps => Str::CleanerCategoryInstalledApps,
            CleanerCategory::OrphanedFiles => Str::CleanerCategoryOrphanedFiles,
            CleanerCategory::AiApps => Str::CleanerCategoryAiApps,
            CleanerCategory::XcodeJunk => Str::CleanerCategoryXcodeJunk,
            CleanerCategory::HomebrewCache => Str::CleanerCategoryHomebrewCache,
            CleanerCategory::NodeToolingCache => Str::CleanerCategoryNodeToolingCache,
            CleanerCategory::DockerCache => Str::CleanerCategoryDockerCache,
            CleanerCategory::UniversalBinaries => Str::CleanerCategoryUniversalBinaries,
            CleanerCategory::LanguageFiles => Str::CleanerCategoryLanguageFiles,
        }
    }

    fn completeness_label(completeness: &ScanCompleteness) -> Option<Str> {
        match completeness {
            ScanCompleteness::Complete => None,
            ScanCompleteness::Partial {
                reason: PartialScanReason::PermissionDenied,
                ..
            } => Some(Str::CleanerPartialPermissionDenied),
            ScanCompleteness::Partial {
                reason: PartialScanReason::RootUnavailable,
                ..
            } => Some(Str::CleanerPartialRootUnavailable),
            ScanCompleteness::Partial {
                reason: PartialScanReason::Cancelled,
                ..
            } => Some(Str::CleanerPartialCancelled),
            ScanCompleteness::Partial {
                reason: PartialScanReason::UnsupportedEnvironment,
                ..
            } => Some(Str::CleanerPartialUnsupported),
        }
    }

    fn pending_result(category: CleanerCategory) -> CategoryScanResult {
        CategoryScanResult {
            category,
            items: Vec::new(),
            scanned_entries: 0,
            estimated_reclaimable_bytes: 0,
            warnings: vec![crate::cleaner::core::report::ScanWarning {
                message: "This category is planned but not implemented yet.".to_string(),
            }],
            completeness: ScanCompleteness::Partial {
                skipped_roots: Vec::new(),
                reason: PartialScanReason::UnsupportedEnvironment,
            },
        }
    }

    fn cleanup_error_text(error: &CleanupError) -> String {
        match error {
            CleanupError::Safety(error) => format!("{error:?}"),
            CleanupError::Trash(message) => message.clone(),
            CleanupError::PermissionRequired(permission) => format!("{permission:?}"),
            CleanupError::ExternalOperationFailed { operation, message } => {
                format!("{operation}: {message}")
            }
        }
    }

    /// The colour scheme for one sidebar row (section header or category).
    /// `active` gets a tinted rest background on top of the same
    /// hover/press tint everything else gets, so a selected row still reads
    /// as "more selected" on hover rather than losing its highlight. No
    /// left border here — the accent colour, background tint and bold text
    /// alone carry "selected" (req #3).
    fn sidebar_row_variant(cx: &App, active: bool) -> ButtonCustomVariant {
        let base = ButtonCustomVariant::new(cx)
            .hover(cx.theme().primary.opacity(0.08))
            .active(cx.theme().primary_active.opacity(0.16));
        if active {
            base.color(cx.theme().primary.opacity(0.12))
        } else {
            base
        }
    }

    /// The compact per-category indicator in the sidebar (req #4): a
    /// `Spinner` while scanning or cancelling, a small check/warning/error
    /// glyph once there is an outcome to show, nothing for a category that
    /// has never been scanned or whose scan was cancelled outright.
    ///
    /// The mapping is [`ScanState::indicator`], not a `match` here: this
    /// function's previous version said all of the above in its doc comment
    /// and drew an empty `div()` for both in-flight states, which is the
    /// "no spinner on the tab being scanned" report. Reading a tested pure
    /// function is what keeps the claim and the pixels in step.
    fn render_scan_state_glyph(&self, category: CleanerCategory, cx: &App) -> AnyElement {
        match self.state.category(category).scan_state().indicator() {
            ScanIndicator::InProgress => Spinner::new()
                .xsmall()
                .color(cx.theme().primary)
                .into_any_element(),
            ScanIndicator::Success => Icon::new(AppIcon::CircleCheck)
                .size_3()
                .text_color(cx.theme().success.opacity(0.7))
                .into_any_element(),
            ScanIndicator::Warning => Icon::new(AppIcon::AlertTriangle)
                .size_3()
                .text_color(cx.theme().warning)
                .into_any_element(),
            ScanIndicator::Error => Icon::new(AppIcon::CircleX)
                .size_3()
                .text_color(cx.theme().danger)
                .into_any_element(),
            ScanIndicator::Idle => div().into_any_element(),
        }
    }

    /// One tree group in the sidebar: the section's own header row, then —
    /// independently of every other section — its categories only while
    /// `CleanerState::is_section_expanded` says this section is expanded
    /// (req #2: each section owns its own expanded/collapsed bit).
    fn render_section_group(
        &self,
        section: CleanerSection,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let expanded = self.state.is_section_expanded(section);
        let accent_color = if expanded {
            cx.theme().primary
        } else {
            cx.theme().muted_foreground
        };
        let mut rows = vec![
            Button::new(format!("cleaner-section-{section:?}"))
                .custom(Self::sidebar_row_variant(cx, expanded))
                .w_full()
                .child(
                    h_flex()
                        .w_full()
                        .items_center()
                        .justify_between()
                        .gap_2()
                        .child(
                            h_flex()
                                .min_w_0()
                                .items_center()
                                .gap_2()
                                .child(
                                    Icon::new(Self::section_icon(section))
                                        .size_4()
                                        .flex_shrink_0()
                                        .text_color(accent_color),
                                )
                                .child(
                                    div()
                                        .min_w_0()
                                        .truncate()
                                        .text_color(accent_color)
                                        .when(expanded, |div| div.font_bold())
                                        .child(t(Self::section_label(section), cx)),
                                ),
                        )
                        .child(
                            Icon::new(if expanded {
                                AppIcon::ChevronDown
                            } else {
                                AppIcon::ChevronRight
                            })
                            .size_3()
                            .flex_shrink_0()
                            .text_color(accent_color),
                        ),
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.state.toggle_section_expanded(section);
                    cx.notify();
                }))
                .into_any_element(),
        ];

        if expanded {
            rows.extend(
                CleanerCategory::categories_for(section)
                    .map(|category| self.render_category_row(category, cx)),
            );
        }

        rows
    }

    fn render_category_row(&self, category: CleanerCategory, cx: &mut Context<Self>) -> AnyElement {
        let active = self.state.selected_category() == category;
        let accent_color = if active {
            cx.theme().primary
        } else {
            cx.theme().muted_foreground
        };
        Button::new(format!("cleaner-category-{category:?}"))
            .custom(Self::sidebar_row_variant(cx, active))
            .w_full()
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .pl_6()
                    .child(
                        h_flex()
                            .min_w_0()
                            .items_center()
                            .gap_2()
                            .child(
                                Icon::new(category_icon(category))
                                    .size_4()
                                    .flex_shrink_0()
                                    .text_color(accent_color),
                            )
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .text_color(accent_color)
                                    .when(active, |div| div.font_bold())
                                    .child(t(Self::category_label(category), cx)),
                            ),
                    )
                    .child(self.render_scan_state_glyph(category, cx)),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                this.set_selected_category(category, cx);
            }))
            .into_any_element()
    }

    /// Layer 2/3 of the main pane (req #25): which of the pre-scan empty
    /// state, the scanning panel or the completed summary + results is
    /// shown is entirely a function of `ScanState` — never inferred from a
    /// warning string or an `Option` combination.
    fn render_category_body(
        &self,
        category: CleanerCategory,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let state = self.state.category(category);
        match state.scan_state() {
            ScanState::NotScanned => self.render_empty_state(category, cx),
            _ => self.render_scanned_body(category, state, cx),
        }
    }

    /// The pre-scan empty state (req #6): no result table, no zeroed
    /// counters, no disabled Clean button — just what this category is and
    /// a single `Scan` action.
    fn render_empty_state(&self, category: CleanerCategory, cx: &mut Context<Self>) -> AnyElement {
        v_flex()
            .flex_1()
            .min_h_0()
            .items_center()
            .justify_center()
            .gap_3()
            .child(
                Icon::new(category_icon(category))
                    .size_8()
                    .text_color(cx.theme().muted_foreground),
            )
            .child(
                div()
                    .font_bold()
                    .text_lg()
                    .child(t(Self::category_label(category), cx)),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .max_w(px(360.))
                    .text_center()
                    .child(t(Str::CleanerScanDescription, cx)),
            )
            .child(
                Button::new("cleaner-scan")
                    .primary()
                    .label(t(Str::CleanerScan, cx))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.start_scan(category, window, cx)
                    })),
            )
            .into_any_element()
    }

    fn render_scanned_body(
        &self,
        category: CleanerCategory,
        state: &CategoryState,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let top = match state.scan_state() {
            ScanState::Scanning | ScanState::Cancelling => {
                self.render_scanning_panel(category, state, cx)
            }
            _ => self.render_summary_panel(category, state, cx),
        };

        v_flex()
            .flex_1()
            .min_h_0()
            .gap_3()
            .child(top)
            .when(state.cleaning(), |container| {
                container.child(
                    h_flex()
                        .items_center()
                        .gap_2()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(Spinner::new().xsmall())
                        .child(t(Str::CleanerStatusCleaning, cx)),
                )
            })
            .when_some(state.error(), |container, message| {
                container.child(self.render_error_block(category, message, cx))
            })
            .when_some(state.result(), |container, result| {
                let mut container = container;
                if let Some(label) = Self::completeness_label(&result.completeness) {
                    container = container.child(
                        div()
                            .rounded(cx.theme().radius)
                            .border_1()
                            .border_color(cx.theme().border)
                            .bg(cx.theme().warning.opacity(0.08))
                            .px_3()
                            .py_2()
                            .text_sm()
                            .child(t(label, cx)),
                    );
                }
                if !result.warnings.is_empty() {
                    container = container.child(self.render_warnings_block(category, result, cx));
                }
                container
            })
            .when_some(state.cleanup_report(), |container, report| {
                container.child(self.render_cleanup_report_block(report, cx))
            })
            .when(state.result().is_some(), |container| {
                container.child(self.render_results_area(category, cx))
            })
            // A first scan has no previous result to keep on screen, so
            // without this the whole area below the scanning header was
            // blank — the second half of "no in-progress indicator on the
            // tab being scanned". A rescan skips it: the rows it is about to
            // replace are better than a placeholder.
            .when(
                state.result().is_none()
                    && state.scan_state().indicator() == ScanIndicator::InProgress,
                |container| container.child(Self::render_scanning_placeholder(state, cx)),
            )
            .into_any_element()
    }

    /// What fills the results area while a category is being scanned for the
    /// first time. Deliberately the same spinner and the same two words as
    /// the header above it, rather than a second vocabulary for the same
    /// state.
    fn render_scanning_placeholder(state: &CategoryState, cx: &App) -> AnyElement {
        v_flex()
            .flex_1()
            .min_h_0()
            .items_center()
            .justify_center()
            .gap_3()
            .child(Spinner::new().large().color(cx.theme().primary))
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(t(
                        if state.scan_state() == ScanState::Cancelling {
                            Str::CleanerStatusCancelling
                        } else {
                            Str::CleanerStatusScanning
                        },
                        cx,
                    )),
            )
            .into_any_element()
    }

    /// The scanning header (req #7): an indeterminate progress indicator —
    /// none of dodo's scanners know their total work ahead of time, so a
    /// fake percentage is never shown — plus a running entries/bytes count
    /// and a Cancel action that only exists while a scan can still be
    /// cancelled.
    fn render_scanning_panel(
        &self,
        category: CleanerCategory,
        state: &CategoryState,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let cancelling = state.scan_state() == ScanState::Cancelling;
        v_flex()
            .gap_2()
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(cx.theme().border)
            .p_3()
            .child(
                h_flex()
                    .items_center()
                    .gap_2()
                    // The indeterminate indicator the doc comment above
                    // promises. Before this it was a bold word alone, so the
                    // only moving thing on a scanning pane was a counter that
                    // stands still whenever a scanner is inside one long
                    // directory.
                    .child(Spinner::new().xsmall().color(cx.theme().primary))
                    .child(div().font_bold().child(t(
                        if cancelling {
                            Str::CleanerStatusCancelling
                        } else {
                            Str::CleanerStatusScanning
                        },
                        cx,
                    ))),
            )
            .when_some(state.progress(), |this, progress| {
                this.child(
                    h_flex()
                        .gap_3()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(t(
                            Str::CleanerEntriesScannedCount(progress.scanned_entries),
                            cx,
                        ))
                        .child(t(
                            Str::CleanerBytesDiscovered(Self::format_bytes(
                                progress.discovered_bytes,
                            )),
                            cx,
                        )),
                )
            })
            .child(
                h_flex().justify_end().child(
                    Button::new("cleaner-cancel")
                        .ghost()
                        .disabled(cancelling)
                        .label(t(Str::CleanerCancelScan, cx))
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.cancel_scan(category, cx)),
                        ),
                ),
            )
            .into_any_element()
    }

    /// The completed/partial/cancelled/failed summary header (req #8): a
    /// state glyph and label, compact stat chips when there is a result to
    /// summarize, and a Rescan action — never selection controls, which
    /// belong to the results table (req #9).
    fn render_summary_panel(
        &self,
        category: CleanerCategory,
        state: &CategoryState,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (icon, icon_color, label) = match state.scan_state() {
            ScanState::Completed => (
                AppIcon::CircleCheck,
                cx.theme().success,
                Str::CleanerStatusCompleted,
            ),
            ScanState::CompletedWithWarnings => (
                AppIcon::AlertTriangle,
                cx.theme().warning,
                Str::CleanerStatusCompletedWithWarnings,
            ),
            ScanState::PartiallyCompleted => (
                AppIcon::AlertTriangle,
                cx.theme().warning,
                Str::CleanerStatusPartial,
            ),
            ScanState::Cancelled => (
                AppIcon::CircleX,
                cx.theme().muted_foreground,
                Str::CleanerStatusCancelled,
            ),
            ScanState::Failed
            | ScanState::NotScanned
            | ScanState::Scanning
            | ScanState::Cancelling => (
                AppIcon::CircleX,
                cx.theme().danger,
                Str::CleanerStatusFailed,
            ),
        };
        let result = state.result();
        let safe_count = result
            .map(|result| {
                result
                    .items
                    .iter()
                    .filter(|item| {
                        matches!(item.selection_policy, SelectionPolicy::SelectedByDefault)
                    })
                    .count()
            })
            .unwrap_or(0);
        let items_count = result.map(|result| result.items.len()).unwrap_or(0);
        let reclaimable = result
            .map(|result| result.estimated_reclaimable_bytes)
            .unwrap_or(0);
        let warnings_count = result.map(|result| result.warnings.len()).unwrap_or(0);
        let is_busy = self.is_category_busy(category) || self.is_permission_check_pending(category);

        v_flex()
            .gap_2()
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(cx.theme().border)
            .p_3()
            .child(
                h_flex()
                    .items_center()
                    .justify_between()
                    .child(
                        h_flex()
                            .items_center()
                            .gap_2()
                            .child(Icon::new(icon).size_4().text_color(icon_color))
                            .child(div().font_bold().child(t(label, cx))),
                    )
                    .child(
                        Button::new("cleaner-rescan")
                            .ghost()
                            .disabled(is_busy)
                            .label(t(Str::CleanerRescan, cx))
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.start_scan(category, window, cx)
                            })),
                    ),
            )
            .when(result.is_some(), |this| {
                this.child(
                    h_flex()
                        .gap_4()
                        .text_sm()
                        .child(t(
                            Str::CleanerReclaimableAmount(Self::format_bytes(reclaimable)),
                            cx,
                        ))
                        .child(t(Str::CleanerItemsFound(items_count), cx))
                        .child(t(Str::CleanerSafeItemsCount(safe_count), cx))
                        .when(warnings_count > 0, |row| {
                            row.child(t(Str::CleanerWarningCount(warnings_count), cx))
                        })
                        .when_some(Self::scan_duration_label(state), |row, duration| {
                            row.child(
                                div()
                                    .text_color(cx.theme().muted_foreground)
                                    .child(duration),
                            )
                        }),
                )
            })
            .into_any_element()
    }

    /// The friendly failure summary (req #18) for [`ScanState::Failed`]: the
    /// state itself is what leads, and the raw `format!("{error:?}")` detail
    /// (`CategoryState::error`) stays available but collapsed rather than
    /// shown directly.
    fn render_error_block(
        &self,
        category: CleanerCategory,
        message: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let expanded = self.expanded_warnings.contains(&category);
        v_flex()
            .gap_1()
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().danger.opacity(0.08))
            .p_2()
            .child(
                h_flex()
                    .items_center()
                    .justify_between()
                    .child(
                        h_flex()
                            .items_center()
                            .gap_2()
                            .child(
                                Icon::new(AppIcon::CircleX)
                                    .size_4()
                                    .text_color(cx.theme().danger),
                            )
                            .child(div().text_sm().child(t(Str::CleanerStatusFailed, cx))),
                    )
                    .child(
                        Button::new("cleaner-error-toggle")
                            .ghost()
                            .xsmall()
                            .label(t(
                                if expanded {
                                    Str::CleanerScanWarningsHideDetails
                                } else {
                                    Str::CleanerScanWarningsShowDetails
                                },
                                cx,
                            ))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if !this.expanded_warnings.remove(&category) {
                                    this.expanded_warnings.insert(category);
                                }
                                cx.notify();
                            })),
                    ),
            )
            .when(expanded, |this| {
                this.child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(message.to_string()),
                )
            })
            .into_any_element()
    }

    /// The friendly warnings summary (req #18): a count and a collapsed-by-
    /// -default expander, never a raw `format!("{error:?}")` string leading
    /// the display — the diagnostic detail is still there, one click away.
    fn render_warnings_block(
        &self,
        category: CleanerCategory,
        result: &CategoryScanResult,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let expanded = self.expanded_warnings.contains(&category);
        v_flex()
            .gap_1()
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(cx.theme().border)
            .bg(cx.theme().warning.opacity(0.08))
            .p_2()
            .child(
                h_flex()
                    .items_center()
                    .justify_between()
                    .child(
                        h_flex()
                            .items_center()
                            .gap_2()
                            .child(
                                Icon::new(AppIcon::AlertTriangle)
                                    .size_4()
                                    .text_color(cx.theme().warning),
                            )
                            .child(div().text_sm().child(t(
                                Str::CleanerScanWarningsSummary(result.warnings.len()),
                                cx,
                            ))),
                    )
                    .child(
                        Button::new("cleaner-warnings-toggle")
                            .ghost()
                            .xsmall()
                            .label(t(
                                if expanded {
                                    Str::CleanerScanWarningsHideDetails
                                } else {
                                    Str::CleanerScanWarningsShowDetails
                                },
                                cx,
                            ))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if !this.expanded_warnings.remove(&category) {
                                    this.expanded_warnings.insert(category);
                                }
                                cx.notify();
                            })),
                    ),
            )
            .when(expanded, |this| {
                this.child(
                    div()
                        .font_bold()
                        .text_sm()
                        .child(t(Str::CleanerWarnings, cx)),
                )
                .children(result.warnings.iter().map(|warning| {
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(warning.message.clone())
                }))
            })
            .into_any_element()
    }

    fn render_cleanup_report_block(
        &self,
        report: &crate::cleaner::core::report::CleanupReport,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        v_flex()
            .gap_1()
            .rounded(cx.theme().radius)
            .border_1()
            .border_color(cx.theme().border)
            .p_2()
            .child(div().font_bold().child(t(Str::CleanerCleanupReport, cx)))
            .child(div().text_sm().child(t(
                Str::CleanerCleanupSuccessCount(report.successes.len()),
                cx,
            )))
            .child(div().text_sm().child(t(
                Str::CleanerCleanupFailureCount(report.failures.len()),
                cx,
            )))
            .when(!report.failures.is_empty(), |list| {
                list.children(report.failures.iter().map(|failure| {
                    div().text_sm().text_color(cx.theme().danger).child(format!(
                        "{}: {}",
                        failure.path.display(),
                        Self::cleanup_error_text(&failure.error)
                    ))
                }))
            })
            .into_any_element()
    }

    /// Layer 3 (req #25/#9-#13): the selection toolbar lives directly above
    /// the table it acts on, and the table body is the only thing that
    /// scrolls — the toolbar above it stays put, standing in for a sticky
    /// action bar without any extra scroll tracking (req #12).
    fn render_trash_results_area(&self, cx: &mut Context<Self>) -> AnyElement {
        let state = self.state.category(CleanerCategory::TrashBins);
        let result = state.result();
        let count = result.map_or(0, |result| result.scanned_entries);
        let size = result.map_or(0, |result| result.estimated_reclaimable_bytes);
        let busy = self.is_category_busy(CleanerCategory::TrashBins);
        v_flex()
            .flex_1()
            .min_h_0()
            .items_center()
            .justify_center()
            .gap_3()
            .child(
                div()
                    .text_lg()
                    .font_bold()
                    .child(t(Str::CleanerItemsFound(count as usize), cx)),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(Self::format_bytes(size)),
            )
            .child(
                Button::new("cleaner-empty-trash")
                    .danger()
                    .disabled(busy || count == 0)
                    .label(t(Str::CleanerEmptyTrash, cx))
                    .on_click(
                        cx.listener(|this, _, window, cx| this.confirm_empty_trash(window, cx)),
                    ),
            )
            .into_any_element()
    }

    fn render_results_area(&self, category: CleanerCategory, cx: &mut Context<Self>) -> AnyElement {
        if category == CleanerCategory::TrashBins {
            return self.render_trash_results_area(cx);
        }
        let category_state = self.state.category(category);
        let selected_count = category_state.selected_count();
        let selected_bytes = category_state.selected_reclaimable_bytes();
        let is_busy = self.is_category_busy(category);

        v_flex()
            .flex_1()
            .min_h_0()
            .gap_2()
            .child(
                h_flex()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .child(
                        h_flex()
                            .items_center()
                            .gap_3()
                            .child(
                                Button::new("cleaner-select-safe")
                                    .ghost()
                                    .disabled(is_busy)
                                    .label(t(Str::CleanerSelectSafeItems, cx))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.select_safe_items(category, cx)
                                    })),
                            )
                            .when(selected_count > 0, |row| {
                                row.child(
                                    div()
                                        .text_sm()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(t(
                                            Str::CleanerSelectedSummary {
                                                count: selected_count,
                                                size: Self::format_bytes(selected_bytes),
                                            },
                                            cx,
                                        )),
                                )
                            }),
                    )
                    .child(
                        Button::new("cleaner-clean-selected")
                            .when(selected_count > 0, |btn| btn.danger())
                            .when(selected_count == 0, |btn| btn.ghost())
                            .disabled(is_busy || selected_count == 0)
                            .label(if selected_count > 0 {
                                t(
                                    Str::CleanerCleanCount {
                                        count: selected_count,
                                        size: Self::format_bytes(selected_bytes),
                                    },
                                    cx,
                                )
                            } else {
                                t(Str::CleanerCleanSelected, cx)
                            })
                            .on_click(
                                cx.listener(|this, _, window, cx| this.confirm_cleanup(window, cx)),
                            ),
                    ),
            )
            .child(
                div().flex_1().min_h_0().child(
                    DataTable::new(&self.results_table)
                        .stripe(true)
                        .bordered(true),
                ),
            )
            .into_any_element()
    }
}

impl Render for CleanerView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_results_table(cx);
        let category = self.state.selected_category();

        v_flex()
            .size_full()
            .when(!Self::supported_platform(), |this| {
                this.child(
                    v_flex()
                        .size_full()
                        .justify_center()
                        .items_center()
                        .gap_2()
                        .child(div().font_bold().child(t(Str::CleanerTitle, cx)))
                        .child(
                            div()
                                .text_color(cx.theme().muted_foreground)
                                .child(t(Str::CleanerUnsupportedPlatform, cx)),
                        ),
                )
            })
            .when(Self::supported_platform(), |this| {
                this.child(
                    h_flex()
                        .size_full()
                        .gap_4()
                        .child(
                            v_flex().w(px(240.)).h_full().gap_1().children(
                                CleanerSection::ALL
                                    .into_iter()
                                    .flat_map(|section| self.render_section_group(section, cx)),
                            ),
                        )
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w_0()
                                .h_full()
                                .gap_3()
                                .child(
                                    h_flex()
                                        .items_center()
                                        .gap_2()
                                        .child(Icon::new(category_icon(category)).size_4())
                                        .child(
                                            div()
                                                .font_bold()
                                                .child(t(Self::category_label(category), cx)),
                                        ),
                                )
                                .when_some(self.ignore_store_error.as_ref(), |this, error| {
                                    this.child(
                                        div()
                                            .text_sm()
                                            .text_color(cx.theme().danger)
                                            .child(t(error.message(), cx)),
                                    )
                                })
                                .child(self.render_category_body(category, cx)),
                        ),
                )
            })
    }
}

/// One pump tick: applies **at most one** update per category and returns
/// whether anything changed. This is the bound on catch-up — a pump that was
/// starved for a second does the same amount of work as one that ticked on
/// time, because a scanner that reported a thousand times in between left one
/// update behind, not a thousand (see [`LatestProgress`]).
///
/// A free function rather than a method so it can be driven straight from a
/// plain [`CleanerState`] in tests, with no window and no frame.
fn apply_latest_progress(
    state: &mut CleanerState,
    slots: &HashMap<CleanerCategory, Arc<LatestProgress>>,
) -> bool {
    let mut updated = false;
    for (category, slot) in slots {
        if let Some(progress) = slot.take() {
            state.update_progress(*category, progress);
            updated = true;
        }
    }
    updated
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::thread;

    use gpui::{AppContext as _, TestAppContext};
    use gpui_component::table::TableDelegate as _;

    use super::{CleanerView, apply_latest_progress};
    use crate::cleaner::core::category::CleanerCategory;
    use crate::cleaner::core::item::{CleanableItem, CleanableItemId, ItemMetadata};
    use crate::cleaner::core::progress::{LatestProgress, ProgressSink, ScanPhase, ScanProgress};
    use crate::cleaner::core::report::{CategoryScanResult, ScanCompleteness};
    use crate::cleaner::core::risk::{ItemCapability, RiskLevel, SelectionPolicy};
    use crate::cleaner::state::CleanerState;

    use std::collections::HashMap;

    fn progress(category: CleanerCategory, scanned_entries: u64) -> ScanProgress {
        ScanProgress {
            category,
            phase: ScanPhase::Traversing,
            current_path: None,
            scanned_entries,
            discovered_items: scanned_entries / 10,
            discovered_bytes: scanned_entries * 1024,
        }
    }

    /// The catch-up bound: a pump tick that arrives after a flood costs one
    /// apply per category, not one per report, and lands the newest value.
    #[test]
    fn a_pump_tick_applies_at_most_one_update_per_category() {
        let categories = [
            CleanerCategory::SystemJunk,
            CleanerCategory::UserCache,
            CleanerCategory::LargeOldFiles,
        ];
        let slots: HashMap<_, _> = categories
            .iter()
            .map(|category| (*category, Arc::new(LatestProgress::new())))
            .collect();

        for step in 1..=5_000u64 {
            for category in categories {
                slots[&category].report(progress(category, step));
            }
        }
        for category in categories {
            assert_eq!(slots[&category].pending(), 1);
        }

        let mut state = CleanerState::default();
        for category in categories {
            state.begin_scan(category);
        }
        assert!(apply_latest_progress(&mut state, &slots));
        for category in categories {
            assert_eq!(
                state.category(category).progress(),
                Some(&progress(category, 5_000)),
                "the tick must land the newest report, not an intermediate one"
            );
            assert_eq!(slots[&category].pending(), 0);
        }

        // A second tick with nothing new reports no change, so `cx.notify()`
        // is not called for a frame that would draw the same thing.
        assert!(!apply_latest_progress(&mut state, &slots));
    }

    /// One category's flood never delays or overwrites another's update —
    /// the per-category slot is what keeps the two independent.
    #[test]
    fn categories_do_not_share_a_slot() {
        let mut slots = HashMap::new();
        slots.insert(CleanerCategory::SystemJunk, Arc::new(LatestProgress::new()));
        slots.insert(CleanerCategory::UserCache, Arc::new(LatestProgress::new()));

        for step in 1..=100u64 {
            slots[&CleanerCategory::SystemJunk].report(progress(CleanerCategory::SystemJunk, step));
        }
        slots[&CleanerCategory::UserCache].report(progress(CleanerCategory::UserCache, 7));

        let mut state = CleanerState::default();
        state.begin_scan(CleanerCategory::SystemJunk);
        state.begin_scan(CleanerCategory::UserCache);
        assert!(apply_latest_progress(&mut state, &slots));

        assert_eq!(
            state.category(CleanerCategory::SystemJunk).progress(),
            Some(&progress(CleanerCategory::SystemJunk, 100))
        );
        assert_eq!(
            state.category(CleanerCategory::UserCache).progress(),
            Some(&progress(CleanerCategory::UserCache, 7))
        );
    }

    #[gpui::test]
    fn landing_rows_does_not_change_the_static_column_layout(cx: &mut TestAppContext) {
        cx.update(gpui_component::init);
        let window = cx.add_window(CleanerView::new);
        cx.run_until_parked();
        let (cleaner, table, before) = cx
            .read_window(&window, |cleaner, cx| {
                let table = cleaner.read(cx).results_table.clone();
                let state = table.read(cx);
                let before = (0..state.delegate().columns_count(cx))
                    .map(|ix| {
                        let column = state.delegate().column(ix, cx);
                        (column.key.to_string(), column.width)
                    })
                    .collect::<Vec<_>>();
                (cleaner, table, before)
            })
            .expect("test window stays open");
        let category = CleanerCategory::SystemJunk;

        cleaner.update(cx, |cleaner, cx| {
            cleaner.state.begin_scan(category);
            cleaner.state.finish_scan(
                category,
                Some(CategoryScanResult {
                    category,
                    estimated_reclaimable_bytes: 1,
                    scanned_entries: 1,
                    items: vec![CleanableItem {
                        id: CleanableItemId(1),
                        category,
                        group: None,
                        display_name: "cache".to_string(),
                        path: "/tmp/cache".into(),
                        logical_size: 1,
                        allocated_size: None,
                        modified_at: None,
                        last_accessed_at: None,
                        risk: RiskLevel::SafeRecreatable,
                        selection_policy: SelectionPolicy::SelectedByDefault,
                        capabilities: vec![
                            ItemCapability::MoveToTrash,
                            ItemCapability::RevealInFinder,
                            ItemCapability::CopyPath,
                            ItemCapability::MarkAsKept,
                            ItemCapability::UninstallApplication,
                        ],
                        explanation: String::new(),
                        warnings: Vec::new(),
                        metadata: ItemMetadata::Generic,
                    }],
                    warnings: Vec::new(),
                    completeness: ScanCompleteness::Complete,
                }),
                false,
                None,
            );
            cx.notify();
        });
        cx.run_until_parked();

        let (rows, after) = cx
            .read_window(&window, |_, cx| {
                let state = table.read(cx);
                let after = (0..state.delegate().columns_count(cx))
                    .map(|ix| {
                        let column = state.delegate().column(ix, cx);
                        (column.key.to_string(), column.width)
                    })
                    .collect::<Vec<_>>();
                (state.delegate().rows_count(cx), after)
            })
            .expect("test window stays open");
        assert_eq!(rows, 1);
        assert_eq!(after, before, "row data must not mutate table layout");
    }

    /// A producer flooding from its own thread while the pump ticks never
    /// leaves more than one update pending, and every report is accounted
    /// for: applied, coalesced away, or still pending — never queued.
    #[test]
    fn a_concurrent_flood_stays_bounded_while_the_pump_ticks() {
        const REPORTS: u64 = 50_000;
        let category = CleanerCategory::UserCache;
        let slot = Arc::new(LatestProgress::new());
        let producer_slot = slot.clone();
        let done = Arc::new(AtomicBool::new(false));
        let producer_done = done.clone();

        let producer = thread::spawn(move || {
            for step in 1..=REPORTS {
                producer_slot.report(progress(CleanerCategory::UserCache, step));
            }
            producer_done.store(true, Ordering::Release);
        });

        let mut slots = HashMap::new();
        slots.insert(category, slot.clone());
        let mut state = CleanerState::default();
        state.begin_scan(category);
        let mut applied = 0u64;
        loop {
            let finished = done.load(Ordering::Acquire);
            if apply_latest_progress(&mut state, &slots) {
                applied += 1;
            }
            assert!(
                slot.pending() <= 1,
                "the slot must never hold more than one pending update"
            );
            if finished {
                break;
            }
        }
        producer.join().expect("producer thread finishes");

        // Conservation: every report either reached the state, was coalesced
        // away by a newer one, or is the single one still pending. Nothing
        // was queued, so catch-up never grew with the flood.
        assert_eq!(applied + slot.coalesced() + slot.pending() as u64, REPORTS);
        assert!(
            applied < REPORTS,
            "a flood this size must have coalesced something"
        );
    }
}
