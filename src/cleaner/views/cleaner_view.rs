use std::sync::Arc;

use gpui::prelude::FluentBuilder as _;
use gpui::*;
use gpui_component::WindowExt as _;
use gpui_component::button::{Button, ButtonCustomVariant, ButtonVariant, ButtonVariants as _};
use gpui_component::dialog::DialogButtonProps;
use gpui_component::progress::Progress;
use gpui_component::table::{DataTable, TableState};
use gpui_component::{ActiveTheme, Disableable as _, Icon, StyledExt as _, h_flex, v_flex};

use crate::app_icon::AppIcon;
use crate::cleaner::core::cancellation::CancellationToken;
use crate::cleaner::core::category::{CleanerCategory, CleanerSection};
use crate::cleaner::core::errors::{CleanupError, ScanError};
use crate::cleaner::core::ignore::{IgnoredItemsDocument, path_signature};
use crate::cleaner::core::item::{CleanableItem, CleanableItemId};
use crate::cleaner::core::permissions::{MacPermission, PermissionService, PermissionState};
use crate::cleaner::core::progress::{ProgressSink, ScanProgress};
#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
use crate::cleaner::core::report::CleanupReport;
use crate::cleaner::core::report::{
    CategoryScanResult, PartialScanReason, ScanCompleteness, ScanWarning,
};
use crate::cleaner::core::scan_context::ScanContext;
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
use crate::cleaner::state::{CleanerState, CleanerStatus, default_scanners};
use crate::cleaner::views::results_table::{ResultsTableDelegate, category_icon};
#[cfg(target_os = "macos")]
use crate::cleaner::views::uninstall_review_dialog;
use crate::i18n::{Str, t};

/// The pure decision behind the sidebar's single-open accordion: clicking
/// `clicked` collapses everything when it was already the open one, else it
/// becomes the only open one. Pulled out of `CleanerView::toggle_section` so
/// this can be tested directly — a real click cannot be driven in this
/// project's test setup, which has no way to host a `Root` (see
/// `gpui-component-recipes`), so the GPUI wrapper around this has no test of
/// its own and this is the whole coverage for the behaviour.
fn next_expanded_section(
    current: Option<CleanerSection>,
    clicked: CleanerSection,
) -> Option<CleanerSection> {
    if current == Some(clicked) {
        None
    } else {
        Some(clicked)
    }
}

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
    state: CleanerState,
    scanners: Vec<Arc<dyn CleanerScanner>>,
    #[cfg(target_os = "macos")]
    permission_service: Arc<dyn PermissionService>,
    permission_state: PermissionState,
    permission_task: Option<Task<()>>,
    scan_task: Option<Task<()>>,
    cleanup_task: Option<Task<()>>,
    pump_task: Option<Task<()>>,
    progress_rx: Option<std::sync::mpsc::Receiver<ScanProgress>>,
    cancellation: Option<CancellationToken>,
    /// Where the orphan-detection "keep" list lives; see
    /// `crate::cleaner::services::ignore_store`. Not `#[cfg(target_os = "macos")]`
    /// like `permission_service` — the store itself is plain JSON I/O with no
    /// macOS API calls, only the `OrphanedFiles` items that ever carry
    /// `ItemCapability::MarkAsKept` are macOS-only.
    ignore_store: Arc<dyn OrphanIgnoreStore>,
    /// The loaded keep list, kept in memory so "Keep" does not need a
    /// load-modify-persist round trip through disk for every click.
    ignored_paths: std::collections::BTreeSet<String>,
    ignore_load_task: Option<Task<()>>,
    /// What went wrong reading or writing `cleaner-ignored-items.json`, if
    /// anything. `None` in the ordinary case, including a first run with no
    /// file yet.
    ignore_store_error: Option<OrphanIgnoreStoreError>,
    /// The virtualized results grid for the active category. See
    /// `results_table`'s module doc for why it holds a `WeakEntity` back to
    /// this view rather than the other way around.
    results_table: Entity<TableState<ResultsTableDelegate>>,
    /// Which sidebar section's categories are currently showing, if any.
    /// Purely a sidebar-accordion display concern — not `CleanerState`'s
    /// `section`, which is "the section scanned when Smart Care runs" and
    /// must keep its own value even while every section is collapsed.
    expanded_section: Option<CleanerSection>,
    /// Which categories `self.state.status()` actually describes: the scan's
    /// targets (one category outside Smart Care, all of them inside it) or,
    /// while cleaning, whichever categories the items being cleaned belong
    /// to. `CleanerState::status` is a single field shared by every category
    /// — without this, switching to a category outside that set (e.g. one
    /// Smart Care hasn't reached yet, or one a single-category scan never
    /// touched) would still read "Scanning"/"Completed" left over from
    /// whatever run last touched the status field. Read only through
    /// [`Self::displayed_status`].
    active_run_categories: Vec<CleanerCategory>,
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
        let state = CleanerState::default();
        let expanded_section = Some(state.section());
        let mut view = Self {
            state,
            scanners: default_scanners(),
            #[cfg(target_os = "macos")]
            permission_service: permissions::default_service(),
            permission_state: PermissionState::Unknown,
            permission_task: None,
            scan_task: None,
            cleanup_task: None,
            pump_task: None,
            progress_rx: None,
            cancellation: None,
            ignore_store: Arc::new(DiskOrphanIgnoreStore::new()),
            ignored_paths: std::collections::BTreeSet::new(),
            ignore_load_task: None,
            ignore_store_error: None,
            results_table,
            expanded_section,
            active_run_categories: Vec::new(),
        };
        #[cfg(target_os = "macos")]
        view.refresh_permission_state(cx);
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
    /// `render`, not gated behind a dirty flag: `render` itself only runs
    /// when something already changed (a `cx.notify()` fired), and the copy
    /// is a `Vec`/`HashSet` clone bounded by the active category's own item
    /// count — cheap next to the GPUI element tree `DataTable` builds only
    /// for the rows actually on screen. A dirty flag would risk a stale
    /// table on the one call site someone forgets to set it.
    fn sync_results_table(&mut self, cx: &mut Context<Self>) {
        let category = self.state.category();
        let items = self
            .state
            .result_for(category)
            .map(|result| result.items.clone())
            .unwrap_or_default();
        let selected_ids: std::collections::HashSet<CleanableItemId> =
            self.state.selected_ids_for(category).into_iter().collect();
        self.results_table.update(cx, |table, cx| {
            table.delegate_mut().set(items, selected_ids);
            table.refresh(cx);
        });
    }

    fn category_permission_requirement(category: CleanerCategory) -> Option<MacPermission> {
        match category {
            CleanerCategory::MailFiles | CleanerCategory::OrphanedFiles => {
                Some(MacPermission::FullDiskAccess)
            }
            _ => None,
        }
    }

    #[cfg(target_os = "macos")]
    fn refresh_permission_state(&mut self, cx: &mut Context<Self>) {
        if self.permission_task.is_some() {
            return;
        }
        self.permission_state = PermissionState::Checking;
        let service = self.permission_service.clone();
        self.permission_task = Some(cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { service.check_full_disk_access() })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.permission_state = match result {
                    Ok(state) => state,
                    Err(_) => PermissionState::Unknown,
                };
                this.permission_task = None;
                cx.notify();
            });
        }));
    }

    #[cfg(target_os = "macos")]
    fn open_full_disk_access_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Err(error) = self.permission_service.open_full_disk_access_settings() {
            window.open_alert_dialog(cx, move |alert, _, cx| {
                alert
                    .title(t(Str::CleanerStatusFailed, cx))
                    .description(format!("{error:?}"))
            });
        }
    }

    #[cfg(target_os = "macos")]
    fn reveal_application_bundle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Err(error) = self.permission_service.reveal_application_bundle() {
            window.open_alert_dialog(cx, move |alert, _, cx| {
                alert
                    .title(t(Str::CleanerStatusFailed, cx))
                    .description(format!("{error:?}"))
            });
        }
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

    /// A section header's click handler: standard single-open accordion
    /// behaviour. Clicking the already-expanded section collapses it;
    /// clicking any other section collapses whatever was open and expands
    /// that one instead. Deliberately not the same thing as "which section
    /// is active for scanning" — collapsing a section must not lose the
    /// user's place in the main pane, so `self.expanded_section` is its own
    /// field rather than being read off `self.state.section()`.
    fn toggle_section(&mut self, section: CleanerSection, cx: &mut Context<Self>) {
        self.expanded_section = next_expanded_section(self.expanded_section, section);
        if self.expanded_section == Some(section) {
            self.state.set_section(section);
        }
        cx.notify();
    }

    fn set_category(&mut self, category: CleanerCategory, cx: &mut Context<Self>) {
        self.state.set_category(category);
        cx.notify();
    }

    fn start_scan(&mut self, cx: &mut Context<Self>) {
        if !Self::supported_platform() || self.state.status() == CleanerStatus::Scanning {
            return;
        }

        let cancellation = CancellationToken::new();
        let (tx, rx) = std::sync::mpsc::channel();
        let scanners = self.scanners.clone();
        let targets = self.scan_targets();
        let cancellation_for_scan = cancellation.clone();
        #[cfg(target_os = "macos")]
        let permission_state = self.permission_state;
        self.cancellation = Some(cancellation);
        self.progress_rx = Some(rx);
        self.active_run_categories.clone_from(&targets);
        self.state.begin_scan();
        cx.notify();

        self.pump_progress(cx);

        self.scan_task = Some(cx.spawn(async move |this, cx| {
            let scan_result = cx
                .background_executor()
                .spawn(async move {
                    let mut had_failures = false;
                    let mut cancelled = false;
                    let context = ScanContext::new();
                    let sink = ChannelProgressSink { tx };
                    let mut results = Vec::new();

                    for category in targets {
                        if cancellation_for_scan.is_cancelled() {
                            cancelled = true;
                            break;
                        }
                        let Some(scanner) = scanners
                            .iter()
                            .find(|scanner| scanner.category() == category)
                            .cloned()
                        else {
                            results.push(Self::pending_result(category));
                            continue;
                        };
                        #[cfg(target_os = "macos")]
                        if scanner
                            .required_permissions()
                            .contains(&MacPermission::FullDiskAccess)
                            && permission_state != PermissionState::Granted
                        {
                            results.push(CategoryScanResult {
                                category,
                                items: Vec::new(),
                                scanned_entries: 0,
                                estimated_reclaimable_bytes: 0,
                                warnings: vec![ScanWarning {
                                    message:
                                        "Full Disk Access is required before this category can scan protected Mail or container data."
                                            .into(),
                                }],
                                completeness: ScanCompleteness::Partial {
                                    skipped_roots: Vec::new(),
                                    reason: PartialScanReason::PermissionDenied,
                                },
                            });
                            continue;
                        }
                        match scanner.scan(&context, &sink, &cancellation_for_scan) {
                            Ok(result) => results.push(result),
                            Err(ScanError::Cancelled) => {
                                cancelled = true;
                                results.push(CategoryScanResult {
                                    category,
                                    items: Vec::new(),
                                    scanned_entries: 0,
                                    estimated_reclaimable_bytes: 0,
                                    warnings: vec![ScanWarning {
                                        message: "Scan cancelled.".to_string(),
                                    }],
                                    completeness: ScanCompleteness::Partial {
                                        skipped_roots: Vec::new(),
                                        reason: PartialScanReason::Cancelled,
                                    },
                                });
                                break;
                            }
                            Err(error) => {
                                had_failures = true;
                                results.push(CategoryScanResult {
                                    category,
                                    items: Vec::new(),
                                    scanned_entries: 0,
                                    estimated_reclaimable_bytes: 0,
                                    warnings: vec![ScanWarning {
                                        message: format!("{error:?}"),
                                    }],
                                    completeness: ScanCompleteness::Partial {
                                        skipped_roots: Vec::new(),
                                        reason: PartialScanReason::RootUnavailable,
                                    },
                                });
                            }
                        }
                    }

                    (results, cancelled, had_failures)
                })
                .await;

            let _ = this.update(cx, |this, cx| {
                let (results, cancelled, had_failures) = scan_result;
                for result in results {
                    this.state.push_result(result);
                }
                this.state.finish_scan(cancelled, had_failures);
                this.cancellation = None;
                this.scan_task = None;
                cx.notify();
            });
        }));
    }

    fn pump_progress(&mut self, cx: &mut Context<Self>) {
        self.pump_task = Some(cx.spawn(async move |this, cx| {
            loop {
                let done = this
                    .update(cx, |this, cx| {
                        let mut updated = false;
                        if let Some(rx) = this.progress_rx.as_ref() {
                            while let Ok(progress) = rx.try_recv() {
                                this.state.update_progress(progress);
                                updated = true;
                            }
                        }
                        if updated {
                            cx.notify();
                        }

                        this.scan_task.is_none()
                            && matches!(
                                this.state.status(),
                                CleanerStatus::Completed
                                    | CleanerStatus::PartiallyCompleted
                                    | CleanerStatus::CompletedWithFailures
                                    | CleanerStatus::Failed
                            )
                    })
                    .unwrap_or(true);

                if done {
                    break;
                }
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(120))
                    .await;
            }
        }));
    }

    fn cancel_scan(&mut self, cx: &mut Context<Self>) {
        if let Some(cancellation) = self.cancellation.as_ref() {
            cancellation.cancel();
            self.state.begin_cancelling();
            cx.notify();
        }
    }

    pub(super) fn toggle_selected(&mut self, id: CleanableItemId, cx: &mut Context<Self>) {
        self.state.toggle_selected(id);
        cx.notify();
    }

    fn select_safe_items(&mut self, cx: &mut Context<Self>) {
        self.state.select_safe_items_for(self.state.category());
        cx.notify();
    }

    fn clear_selection(&mut self, cx: &mut Context<Self>) {
        self.state.clear_selection_for(self.state.category());
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
        let is_docker = self.state.category() == CleanerCategory::DockerCache;
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

    fn run_cleanup(&mut self, items: Vec<CleanableItem>, cx: &mut Context<Self>) {
        if self.cleanup_task.is_some() || items.is_empty() {
            return;
        }
        self.active_run_categories = items
            .iter()
            .map(|item| item.category)
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        self.state.begin_cleaning();
        cx.notify();

        let is_docker = items
            .iter()
            .all(|item| item.category == CleanerCategory::DockerCache);

        self.cleanup_task = Some(cx.spawn(async move |this, cx| {
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
                this.state.finish_cleaning(report);
                this.cleanup_task = None;
                cx.notify();
            });
        }));
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
                .result_for(CleanerCategory::InstalledApps)
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

    fn categories_for_section(section: CleanerSection) -> Vec<CleanerCategory> {
        if section == CleanerSection::SmartCare {
            CleanerCategory::ALL.to_vec()
        } else {
            CleanerCategory::categories_for(section).collect()
        }
    }

    fn selected_items_for_active_category(&self) -> Vec<CleanableItem> {
        let selected_ids = self.state.selected_ids_for(self.state.category());
        let Some(result) = self.state.result_for(self.state.category()) else {
            return Vec::new();
        };
        result
            .items
            .iter()
            .filter(|item| selected_ids.contains(&item.id))
            .cloned()
            .collect()
    }

    fn scan_targets(&self) -> Vec<CleanerCategory> {
        if self.state.section() == CleanerSection::SmartCare {
            CleanerCategory::ALL.to_vec()
        } else {
            vec![self.state.category()]
        }
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

    /// `self.state.status()` filtered through [`Self::active_run_categories`]:
    /// `Idle` for a category the current or last run never touched, the real
    /// status otherwise. See the field doc for why this indirection exists.
    fn displayed_status(&self) -> CleanerStatus {
        if self.active_run_categories.contains(&self.state.category()) {
            self.state.status()
        } else {
            CleanerStatus::Idle
        }
    }

    fn status_label(status: CleanerStatus) -> Str {
        match status {
            CleanerStatus::Idle => Str::CleanerStatusIdle,
            CleanerStatus::CheckingPermissions => Str::CleanerStatusCheckingPermissions,
            CleanerStatus::Scanning => Str::CleanerStatusScanning,
            CleanerStatus::Cancelling => Str::CleanerStatusCancelling,
            CleanerStatus::PartiallyCompleted => Str::CleanerStatusPartial,
            CleanerStatus::Completed => Str::CleanerStatusCompleted,
            CleanerStatus::Cleaning => Str::CleanerStatusCleaning,
            CleanerStatus::CompletedWithFailures => Str::CleanerStatusCompletedWithFailures,
            CleanerStatus::Failed => Str::CleanerStatusFailed,
        }
    }

    fn section_label(section: CleanerSection) -> Str {
        match section {
            CleanerSection::SmartCare => Str::CleanerSectionSmartCare,
            CleanerSection::Cleanup => Str::CleanerSectionCleanup,
            CleanerSection::Applications => Str::CleanerSectionApplications,
            CleanerSection::Advanced => Str::CleanerSectionAdvanced,
        }
    }

    fn section_icon(section: CleanerSection) -> AppIcon {
        match section {
            CleanerSection::SmartCare => AppIcon::ChartPie,
            CleanerSection::Cleanup => AppIcon::Trash,
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
            warnings: vec![ScanWarning {
                message: "This category is planned but not implemented yet.".to_string(),
            }],
            completeness: ScanCompleteness::Partial {
                skipped_roots: Vec::new(),
                reason: PartialScanReason::UnsupportedEnvironment,
            },
        }
    }

    fn permission_state_label(state: PermissionState) -> Str {
        match state {
            PermissionState::Unknown => Str::CleanerPermissionUnknown,
            PermissionState::Checking => Str::CleanerPermissionChecking,
            PermissionState::Granted => Str::CleanerPermissionGranted,
            PermissionState::Denied => Str::CleanerPermissionDenied,
            PermissionState::Restricted => Str::CleanerPermissionRestricted,
            PermissionState::RequiresRestart => Str::CleanerPermissionRequiresRestart,
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
    /// as "more selected" on hover rather than losing its highlight.
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

    /// One accordion group in the sidebar: the section's own header row,
    /// followed by its categories only while `self.expanded_section` names
    /// this section — see [`Self::toggle_section`] for why that is tracked
    /// separately from `self.state.section()`.
    fn render_section_group(
        &self,
        section: CleanerSection,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let expanded = self.expanded_section == Some(section);
        let accent_color = if expanded {
            cx.theme().primary
        } else {
            cx.theme().muted_foreground
        };
        let mut rows = vec![
            Button::new(format!("cleaner-section-{section:?}"))
                .custom(Self::sidebar_row_variant(cx, expanded))
                .w_full()
                .when(expanded, |btn| {
                    btn.border_l_2().border_color(cx.theme().primary)
                })
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
                    this.toggle_section(section, cx);
                }))
                .into_any_element(),
        ];

        if expanded {
            rows.extend(
                Self::categories_for_section(section)
                    .into_iter()
                    .map(|category| self.render_category_row(category, cx)),
            );
        }

        rows
    }

    fn render_category_row(&self, category: CleanerCategory, cx: &mut Context<Self>) -> AnyElement {
        let active = self.state.category() == category;
        let accent_color = if active {
            cx.theme().primary
        } else {
            cx.theme().muted_foreground
        };
        Button::new(format!("cleaner-category-{category:?}"))
            .custom(Self::sidebar_row_variant(cx, active))
            .w_full()
            .when(active, |btn| {
                btn.border_l_2().border_color(cx.theme().primary)
            })
            .child(
                h_flex()
                    .w_full()
                    .items_center()
                    .gap_2()
                    .pl_6()
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
            .on_click(cx.listener(move |this, _, _, cx| {
                this.set_category(category, cx);
            }))
            .into_any_element()
    }
}

impl Render for CleanerView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_results_table(cx);
        let is_scanning = matches!(
            self.state.status(),
            CleanerStatus::Scanning | CleanerStatus::Cancelling
        );
        let is_cleaning = self.state.status() == CleanerStatus::Cleaning;
        let is_busy = is_scanning || is_cleaning || self.cleanup_task.is_some();
        let selected_count = self.state.selected_count_for(self.state.category());

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
                            v_flex()
                                .w(px(240.))
                                .h_full()
                                .gap_2()
                                .child(
                                    div()
                                        .font_bold()
                                        .text_sm()
                                        .child(t(Str::CleanerSidebarTitle, cx)),
                                )
                                .children(CleanerSection::ALL.into_iter().flat_map(|section| {
                                    self.render_section_group(section, cx)
                                })),
                        )
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w_0()
                                .h_full()
                                .gap_3()
                                .child(
                                    h_flex()
                                        .justify_between()
                                        .items_center()
                                        .child(div().font_bold().child(t(
                                            Self::category_label(self.state.category()),
                                            cx,
                                        )))
                                        .child(
                                            h_flex()
                                                .gap_2()
                                                .child(
                                                    Button::new("cleaner-scan")
                                                        .primary()
                                                        .disabled(is_busy)
                                                        .label(t(Str::CleanerScan, cx))
                                                        .on_click(cx.listener(|this, _, _, cx| {
                                                            this.start_scan(cx)
                                                        })),
                                                )
                                                .child(
                                                    Button::new("cleaner-cancel")
                                                        .ghost()
                                                        .disabled(!is_scanning)
                                                        .label(t(Str::CleanerCancelScan, cx))
                                                        .on_click(cx.listener(|this, _, _, cx| {
                                                            this.cancel_scan(cx)
                                                        })),
                                                ),
                                        ),
                                )
                                .child(
                                    h_flex()
                                        .items_center()
                                        .gap_2()
                                        .child(
                                            Button::new("cleaner-select-safe")
                                                .ghost()
                                                .disabled(is_busy)
                                                .label(t(Str::CleanerSelectSafeItems, cx))
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.select_safe_items(cx)
                                                })),
                                        )
                                        .child(
                                            Button::new("cleaner-clear-selection")
                                                .ghost()
                                                .disabled(is_busy || selected_count == 0)
                                                .label(t(Str::CleanerClearSelection, cx))
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.clear_selection(cx)
                                                })),
                                        )
                                        .child(
                                            Button::new("cleaner-clean-selected")
                                                .danger()
                                                .disabled(is_busy || selected_count == 0)
                                                .label(t(Str::CleanerCleanSelected, cx))
                                                .on_click(cx.listener(|this, _, window, cx| {
                                                    this.confirm_cleanup(window, cx)
                                                })),
                                        )
                                        .child(
                                            div()
                                                .text_sm()
                                                .text_color(cx.theme().muted_foreground)
                                                .child(t(
                                                    Str::CleanerSelectedCount(selected_count),
                                                    cx,
                                                )),
                                        ),
                                )
                                .child(
                                    div()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(t(Self::status_label(self.displayed_status()), cx)),
                                )
                                .when_some(self.ignore_store_error.as_ref(), |this, error| {
                                    this.child(
                                        div()
                                            .text_sm()
                                            .text_color(cx.theme().danger)
                                            .child(t(error.message(), cx)),
                                    )
                                })
                                .when(
                                    Self::category_permission_requirement(self.state.category())
                                        == Some(MacPermission::FullDiskAccess),
                                    |this| {
                                        this.child(
                                            v_flex()
                                                .gap_1()
                                                .rounded(cx.theme().radius)
                                                .border_1()
                                                .border_color(cx.theme().border)
                                                .bg(cx.theme().warning.opacity(0.08))
                                                .p_2()
                                                .child(
                                                    div()
                                                        .font_bold()
                                                        .child(t(
                                                            Str::CleanerPermissionTitle,
                                                            cx,
                                                        )),
                                                )
                                                .child(
                                                    div()
                                                        .text_sm()
                                                        .child(t(
                                                            Str::CleanerPermissionExplanation,
                                                            cx,
                                                        )),
                                                )
                                                .child(
                                                    div()
                                                        .text_sm()
                                                        .text_color(
                                                            cx.theme().muted_foreground,
                                                        )
                                                        .child(t(
                                                            Self::permission_state_label(
                                                                self.permission_state,
                                                            ),
                                                            cx,
                                                        )),
                                                )
                                                .child(
                                                    h_flex()
                                                        .gap_2()
                                                        .child(
                                                            Button::new(
                                                                "cleaner-permission-recheck",
                                                            )
                                                            .ghost()
                                                            .label(t(
                                                                Str::CleanerPermissionRecheck,
                                                                cx,
                                                            ))
                                                            .on_click(cx.listener(
                                                                |this, _, _, cx| {
                                                                    #[cfg(target_os = "macos")]
                                                                    this.refresh_permission_state(cx);
                                                                },
                                                            )),
                                                        )
                                                        .child(
                                                            Button::new(
                                                                "cleaner-permission-settings",
                                                            )
                                                            .ghost()
                                                            .label(t(
                                                                Str::CleanerPermissionOpenSettings,
                                                                cx,
                                                            ))
                                                            .on_click(cx.listener(
                                                                |this, _, window, cx| {
                                                                    #[cfg(target_os = "macos")]
                                                                    this.open_full_disk_access_settings(
                                                                        window, cx,
                                                                    );
                                                                },
                                                            )),
                                                        )
                                                        .child(
                                                            Button::new(
                                                                "cleaner-permission-reveal-app",
                                                            )
                                                            .ghost()
                                                            .label(t(
                                                                Str::CleanerPermissionRevealApp,
                                                                cx,
                                                            ))
                                                            .on_click(cx.listener(
                                                                |this, _, window, cx| {
                                                                    #[cfg(target_os = "macos")]
                                                                    this.reveal_application_bundle(
                                                                        window, cx,
                                                                    );
                                                                },
                                                            )),
                                                        ),
                                                ),
                                        )
                                    },
                                )
                                .when(
                                    is_scanning
                                        && self
                                            .active_run_categories
                                            .contains(&self.state.category()),
                                    |this| {
                                    // Only this category's own scan may draw here: a scan
                                    // running for a different category (started before the
                                    // user navigated away) must not bleed its loading bar or
                                    // progress into whichever category happens to be on screen.
                                    let own_progress = self
                                        .state
                                        .progress()
                                        .filter(|progress| progress.category == self.state.category());
                                    this.child(
                                        v_flex()
                                            .gap_1()
                                            .child(Progress::new("cleaner-scan-progress").loading(true))
                                            .when_some(own_progress, |this, progress| {
                                                this.child(
                                                    h_flex()
                                                        .items_center()
                                                        .gap_2()
                                                        .text_sm()
                                                        .child(format!(
                                                            "{} · {} · {}",
                                                            t(Str::CleanerStatusProgress, cx),
                                                            progress.scanned_entries,
                                                            progress.discovered_items,
                                                        ))
                                                        .child(
                                                            // Fixed height + horizontal scroll
                                                            // instead of letting a long path
                                                            // reflow (and jump) the row.
                                                            div()
                                                                .id("cleaner-scan-progress-path")
                                                                .flex_1()
                                                                .min_w_0()
                                                                .h(px(20.))
                                                                .overflow_x_scroll()
                                                                .whitespace_nowrap()
                                                                .text_color(
                                                                    cx.theme().muted_foreground,
                                                                )
                                                                .child(
                                                                    progress
                                                                        .current_path
                                                                        .as_ref()
                                                                        .map(|path| {
                                                                            path.display().to_string()
                                                                        })
                                                                        .unwrap_or_default(),
                                                                ),
                                                        ),
                                                )
                                            }),
                                    )
                                })
                                .child({
                                    // `CleanerState::{estimated_reclaimable_bytes,
                                    // total_scanned_entries}` are running sums across every
                                    // category scanned in this run (all of them, once Smart
                                    // Care is done) — showing those here would put User
                                    // Cache's numbers on the System Junk page just because
                                    // both were part of the same run. This category's own
                                    // result is what belongs on its own page.
                                    let own_result = self.state.result_for(self.state.category());
                                    h_flex()
                                        .gap_4()
                                        .child(div().child(format!(
                                            "{}: {}",
                                            t(Str::CleanerEstimatedReclaimable, cx),
                                            Self::format_bytes(
                                                own_result
                                                    .map(|result| result.estimated_reclaimable_bytes)
                                                    .unwrap_or(0)
                                            )
                                        )))
                                        .child(div().child(format!(
                                            "{}: {}",
                                            t(Str::CleanerEntriesScanned, cx),
                                            own_result
                                                .map(|result| result.scanned_entries)
                                                .unwrap_or(0)
                                        )))
                                })
                                .when_some(
                                    self.state.cleanup_report().filter(|_| {
                                        self.active_run_categories.contains(&self.state.category())
                                    }),
                                    |this, report| {
                                    this.child(
                                        v_flex()
                                            .gap_1()
                                            .rounded(cx.theme().radius)
                                            .border_1()
                                            .border_color(cx.theme().border)
                                            .p_2()
                                            .child(
                                                div()
                                                    .font_bold()
                                                    .child(t(Str::CleanerCleanupReport, cx)),
                                            )
                                            .child(div().text_sm().child(t(
                                                Str::CleanerCleanupSuccessCount(
                                                    report.successes.len(),
                                                ),
                                                cx,
                                            )))
                                            .child(div().text_sm().child(t(
                                                Str::CleanerCleanupFailureCount(
                                                    report.failures.len(),
                                                ),
                                                cx,
                                            )))
                                            .when(!report.failures.is_empty(), |list| {
                                                list.children(report.failures.iter().map(
                                                    |failure| {
                                                        div()
                                                            .text_sm()
                                                            .text_color(cx.theme().danger)
                                                            .child(format!(
                                                                "{}: {}",
                                                                failure.path.display(),
                                                                Self::cleanup_error_text(
                                                                    &failure.error
                                                                )
                                                            ))
                                                    },
                                                ))
                                            }),
                                    )
                                })
                                .child(
                                    v_flex()
                                        .flex_1()
                                        .min_h_0()
                                        .gap_2()
                                        .when_some(
                                            self.state.result_for(self.state.category()),
                                            |container, result| {
                                                container
                                                    .when_some(
                                                        Self::completeness_label(
                                                            &result.completeness,
                                                        ),
                                                        |list, label| {
                                                            list.child(
                                                                div()
                                                                    .rounded(cx.theme().radius)
                                                                    .border_1()
                                                                    .border_color(
                                                                        cx.theme().border,
                                                                    )
                                                                    .bg(
                                                                        cx.theme()
                                                                            .warning
                                                                            .opacity(0.08),
                                                                    )
                                                                    .px_3()
                                                                    .py_2()
                                                                    .child(t(label, cx)),
                                                            )
                                                        },
                                                    )
                                                    .when(!result.warnings.is_empty(), |list| {
                                                        list.child(
                                                            v_flex()
                                                                .gap_1()
                                                                .child(
                                                                    div()
                                                                        .font_bold()
                                                                        .child(t(
                                                                            Str::CleanerWarnings,
                                                                            cx,
                                                                        )),
                                                                )
                                                                .children(
                                                                    result.warnings.iter().map(
                                                                        |warning| {
                                                                            div()
                                                                                .text_sm()
                                                                                .text_color(
                                                                                    cx.theme()
                                                                                        .muted_foreground,
                                                                                )
                                                                                .child(
                                                                                    warning
                                                                                        .message
                                                                                        .clone(),
                                                                                )
                                                                        },
                                                                    ),
                                                                ),
                                                        )
                                                    })
                                            },
                                        )
                                        .child(
                                            div().flex_1().min_h_0().child(
                                                DataTable::new(&self.results_table)
                                                    .stripe(true)
                                                    .bordered(true),
                                            ),
                                        ),
                                ),
                        ),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::next_expanded_section;
    use crate::cleaner::core::category::CleanerSection;

    #[test]
    fn clicking_a_closed_section_opens_only_that_one() {
        assert_eq!(
            next_expanded_section(None, CleanerSection::Cleanup),
            Some(CleanerSection::Cleanup)
        );
        assert_eq!(
            next_expanded_section(Some(CleanerSection::SmartCare), CleanerSection::Cleanup),
            Some(CleanerSection::Cleanup),
            "opening a different section must replace whatever was open, not add to it"
        );
    }

    #[test]
    fn clicking_the_already_open_section_closes_it() {
        assert_eq!(
            next_expanded_section(Some(CleanerSection::Advanced), CleanerSection::Advanced),
            None
        );
    }

    #[test]
    fn a_second_click_on_the_same_section_after_it_reopens_elsewhere_still_toggles() {
        // The exact sequence the bug report described: open Cleanup, open
        // Advanced (Cleanup implicitly closes), then click Advanced again —
        // it must close, not require a third, different section first.
        let mut expanded = next_expanded_section(None, CleanerSection::Cleanup);
        expanded = next_expanded_section(expanded, CleanerSection::Advanced);
        assert_eq!(expanded, Some(CleanerSection::Advanced));
        expanded = next_expanded_section(expanded, CleanerSection::Advanced);
        assert_eq!(expanded, None);
    }
}
