use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::Arc;

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::WindowExt as _;
use gpui_component::button::{Button, ButtonCustomVariant, ButtonVariant, ButtonVariants as _};
use gpui_component::dialog::DialogButtonProps;
use gpui_component::progress::Progress;
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
#[cfg(target_os = "macos")]
use crate::cleaner::core::permissions::{MacPermission, PermissionService, PermissionState};
use crate::cleaner::core::progress::{ProgressSink, ScanProgress};
#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
use crate::cleaner::core::report::CleanupReport;
use crate::cleaner::core::report::{CategoryScanResult, PartialScanReason, ScanCompleteness};
use crate::cleaner::core::risk::SelectionPolicy;
use crate::cleaner::core::scan_context::ScanContext;
use crate::cleaner::core::scan_state::ScanState;
use crate::cleaner::core::scanner::CleanerScanner;
#[cfg(target_os = "macos")]
use crate::cleaner::macos::applications::review as uninstall_review;
#[cfg(target_os = "macos")]
use crate::cleaner::macos::scanners::docker_cache;
#[cfg(target_os = "macos")]
use crate::cleaner::macos::{cleanup, permissions, platform};
use crate::cleaner::services::ignore_store::{
    DiskOrphanIgnoreStore, OrphanIgnoreStore, OrphanIgnoreStoreError,
};
use crate::cleaner::state::{CategoryState, CleanerState, default_scanners};
use crate::cleaner::views::results_table::{ResultsTableDelegate, category_icon};
#[cfg(target_os = "macos")]
use crate::cleaner::views::uninstall_review_dialog;
use crate::i18n::{Str, t};

#[derive(Clone)]
struct ChannelProgressSink {
    tx: std::sync::mpsc::Sender<ScanProgress>,
}

impl ProgressSink for ChannelProgressSink {
    fn report(&self, progress: ScanProgress) {
        let _ = self.tx.send(progress);
    }
}

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
    progress_rxs: HashMap<CleanerCategory, std::sync::mpsc::Receiver<ScanProgress>>,
    /// A single timer loop that drains every live entry in `progress_rxs`
    /// each tick, rather than one timer per category — cheaper, and still
    /// fully independent per-category data (see `ensure_pump_running`).
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
            progress_rxs: HashMap::new(),
            pump_task: None,
            cleanup_tasks: HashMap::new(),
            #[cfg(target_os = "macos")]
            permission_check_tasks: HashMap::new(),
            ignore_store: Arc::new(DiskOrphanIgnoreStore::new()),
            ignored_paths: BTreeSet::new(),
            ignore_load_task: None,
            ignore_store_error: None,
            results_table,
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
    /// [`Self::results_table`]'s delegate. Called at the top of every
    /// `render`, not gated behind a dirty flag — see the original rationale
    /// preserved from round 1: `render` itself only runs when something
    /// already changed, and the copy is bounded by the active category's own
    /// item count.
    fn sync_results_table(&mut self, cx: &mut Context<Self>) {
        let category = self.state.selected_category();
        let category_state = self.state.category(category);
        let items = category_state
            .result()
            .map(|result| result.items.clone())
            .unwrap_or_default();
        let selected_ids: HashSet<CleanableItemId> =
            category_state.selected_ids().into_iter().collect();
        self.results_table.update(cx, |table, cx| {
            table.delegate_mut().set(items, selected_ids);
            table.refresh(cx);
        });
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
    /// category's `scan_tasks`/`cancellations`/`progress_rxs` entry — that
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
        let (tx, rx) = std::sync::mpsc::channel();
        self.cancellations.insert(category, cancellation);
        self.progress_rxs.insert(category, rx);
        self.state.begin_scan(category);
        cx.notify();
        self.ensure_pump_running(cx);

        let task = cx.spawn(async move |this, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move {
                    let context = ScanContext::new();
                    let sink = ChannelProgressSink { tx };
                    match scanner.scan(&context, &sink, &cancellation_for_scan) {
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
                this.progress_rxs.remove(&category);
                this.scan_tasks.remove(&category);
                cx.notify();
            });
        });
        self.scan_tasks.insert(category, task);
    }

    /// A single background loop that drains every category's progress
    /// channel each tick, rather than one timer per category. Stops itself
    /// once `progress_rxs` is empty, so it costs nothing while nothing is
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
                        let mut updated = false;
                        let categories: Vec<CleanerCategory> =
                            this.progress_rxs.keys().copied().collect();
                        for category in categories {
                            if let Some(rx) = this.progress_rxs.get(&category) {
                                while let Ok(progress) = rx.try_recv() {
                                    this.state.update_progress(category, progress);
                                    updated = true;
                                }
                            }
                        }
                        if updated {
                            cx.notify();
                        }
                        !this.progress_rxs.is_empty()
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
                    #[cfg(target_os = "macos")]
                    {
                        if is_docker {
                            docker_cache::prune_items(&items)
                        } else {
                            cleanup::cleanup_items(&items)
                        }
                    }
                    #[cfg(target_os = "windows")]
                    {
                        let _ = is_docker;
                        crate::cleaner::windows::cleanup::cleanup_items(&items)
                    }
                    #[cfg(target_os = "linux")]
                    {
                        let _ = is_docker;
                        crate::cleaner::linux::cleanup::cleanup_items(&items)
                    }
                    #[cfg(not(any(
                        target_os = "macos",
                        target_os = "windows",
                        target_os = "linux"
                    )))]
                    {
                        let _ = (is_docker, items);
                        CleanupReport {
                            successes: Vec::new(),
                            failures: Vec::new(),
                            estimated_reclaimed_bytes: 0,
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
    /// `Spinner` while scanning/cancelling, a small check/warning/error glyph
    /// once there is an outcome to show, nothing for a category that has
    /// never been scanned or whose scan was cancelled outright.
    fn render_scan_state_glyph(&self, category: CleanerCategory, cx: &App) -> AnyElement {
        match self.state.category(category).scan_state() {
            ScanState::Scanning | ScanState::Cancelling => {
                Spinner::new().xsmall().into_any_element()
            }
            ScanState::Completed => Icon::new(AppIcon::CircleCheck)
                .size_3()
                .text_color(cx.theme().success.opacity(0.7))
                .into_any_element(),
            ScanState::CompletedWithWarnings | ScanState::PartiallyCompleted => {
                Icon::new(AppIcon::AlertTriangle)
                    .size_3()
                    .text_color(cx.theme().warning)
                    .into_any_element()
            }
            ScanState::Failed => Icon::new(AppIcon::CircleX)
                .size_3()
                .text_color(cx.theme().danger)
                .into_any_element(),
            ScanState::NotScanned | ScanState::Cancelled => div().into_any_element(),
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
                    .child(Spinner::new().xsmall())
                    .child(div().font_bold().child(t(
                        if cancelling {
                            Str::CleanerStatusCancelling
                        } else {
                            Str::CleanerStatusScanning
                        },
                        cx,
                    ))),
            )
            .child(Progress::new("cleaner-scan-progress").loading(true))
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
    fn render_results_area(&self, category: CleanerCategory, cx: &mut Context<Self>) -> AnyElement {
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
