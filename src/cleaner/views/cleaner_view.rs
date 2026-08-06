use std::sync::Arc;

use gpui::prelude::FluentBuilder as _;
use gpui::{StatefulInteractiveElement as _, *};
use gpui_component::WindowExt as _;
use gpui_component::button::{Button, ButtonVariant, ButtonVariants as _};
use gpui_component::dialog::DialogButtonProps;
use gpui_component::{ActiveTheme, Disableable as _, StyledExt as _, h_flex, v_flex};

use crate::cleaner::core::cancellation::CancellationToken;
use crate::cleaner::core::category::{CleanerCategory, CleanerSection};
use crate::cleaner::core::errors::{CleanupError, ScanError};
use crate::cleaner::core::item::{CleanableItem, CleanableItemId};
use crate::cleaner::core::permissions::{MacPermission, PermissionService, PermissionState};
use crate::cleaner::core::progress::{ProgressSink, ScanProgress};
use crate::cleaner::core::report::{
    CategoryScanResult, PartialScanReason, ScanCompleteness, ScanWarning,
};
use crate::cleaner::core::risk::{ItemCapability, RiskLevel};
use crate::cleaner::core::scan_context::ScanContext;
use crate::cleaner::core::scanner::CleanerScanner;
#[cfg(target_os = "macos")]
use crate::cleaner::macos::applications::review as uninstall_review;
#[cfg(target_os = "macos")]
use crate::cleaner::macos::{cleanup, permissions, platform};
use crate::cleaner::state::{CleanerState, CleanerStatus, default_scanners};
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
}

impl CleanerView {
    pub fn new(_window: &mut Window, _cx: &mut Context<Self>) -> Self {
        let mut view = Self {
            state: CleanerState::default(),
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
        };
        #[cfg(target_os = "macos")]
        view.refresh_permission_state(_cx);
        view
    }

    fn supported_platform() -> bool {
        cfg!(target_os = "macos")
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

    fn set_section(&mut self, section: CleanerSection, cx: &mut Context<Self>) {
        self.state.set_section(section);
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

    fn toggle_selected(&mut self, id: CleanableItemId, cx: &mut Context<Self>) {
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

    fn reveal_in_finder(
        &mut self,
        path: std::path::PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        #[cfg(target_os = "macos")]
        {
            if let Err(error) = platform::reveal_in_finder(path.as_path()) {
                window.open_alert_dialog(cx, move |alert, _, cx| {
                    alert
                        .title(t(Str::CleanerStatusFailed, cx))
                        .description(error.clone())
                });
            }
        }
    }

    fn confirm_cleanup(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let selected = self.selected_items_for_active_category();
        if selected.is_empty() {
            return;
        }
        let count = selected.len();
        let size = Self::format_bytes(selected.iter().map(|item| item.logical_size).sum());
        let view = cx.entity();
        window.open_alert_dialog(cx, move |alert, _window, cx| {
            let confirm_view = view.clone();
            alert
                .title(t(Str::CleanerCleanupConfirmTitle, cx))
                .description(t(
                    Str::CleanerCleanupConfirmMessage {
                        count,
                        size: size.clone(),
                    },
                    cx,
                ))
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
        self.state.begin_cleaning();
        cx.notify();

        self.cleanup_task = Some(cx.spawn(async move |this, cx| {
            let report = cx
                .background_executor()
                .spawn(async move {
                    #[cfg(target_os = "macos")]
                    {
                        cleanup::cleanup_items(&items)
                    }
                    #[cfg(not(target_os = "macos"))]
                    {
                        let _ = items;
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

    fn begin_uninstall_review(
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

    fn section_categories(&self) -> Vec<CleanerCategory> {
        if self.state.section() == CleanerSection::SmartCare {
            CleanerCategory::ALL.to_vec()
        } else {
            CleanerCategory::categories_for(self.state.section()).collect()
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

    fn select_label(selected: bool) -> Str {
        if selected {
            Str::CleanerDeselectItem
        } else {
            Str::CleanerSelectItem
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

    fn risk_badge(risk: RiskLevel) -> &'static str {
        match risk {
            RiskLevel::SafeRecreatable => "safe",
            RiskLevel::ReviewRecommended => "review",
            RiskLevel::UserData => "user-data",
            RiskLevel::ApplicationMutation => "mutation",
            RiskLevel::Protected => "protected",
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
}

impl Render for CleanerView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
                                .children(CleanerSection::ALL.into_iter().map(|section| {
                                    Button::new(format!("cleaner-section-{section:?}"))
                                        .ghost()
                                        .w_full()
                                        .child(
                                            div()
                                                .w_full()
                                                .when(self.state.section() == section, |div| {
                                                    div.font_bold()
                                                })
                                                .child(t(Self::section_label(section), cx)),
                                        )
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.set_section(section, cx);
                                        }))
                                }))
                                .child(div().h(px(8.)))
                                .children(self.section_categories().into_iter().map(|category| {
                                    Button::new(format!("cleaner-category-{category:?}"))
                                        .ghost()
                                        .w_full()
                                        .child(
                                            div()
                                                .w_full()
                                                .pl_3()
                                                .when(self.state.category() == category, |div| {
                                                    div.font_bold()
                                                })
                                                .child(t(Self::category_label(category), cx)),
                                        )
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.set_category(category, cx);
                                        }))
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
                                        .child(t(Self::status_label(self.state.status()), cx)),
                                )
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
                                .when_some(self.state.progress(), |this, progress| {
                                    this.child(
                                        div()
                                            .text_sm()
                                            .child(format!(
                                                "{} · {} · {}{}",
                                                t(Str::CleanerStatusProgress, cx),
                                                progress.scanned_entries,
                                                progress.discovered_items,
                                                progress
                                                    .current_path
                                                    .as_ref()
                                                    .map(|path| format!(" · {}", path.display()))
                                                    .unwrap_or_default()
                                            )),
                                    )
                                })
                                .child(
                                    h_flex()
                                        .gap_4()
                                        .child(div().child(format!(
                                            "{}: {}",
                                            t(Str::CleanerEstimatedReclaimable, cx),
                                            Self::format_bytes(
                                                self.state.estimated_reclaimable_bytes()
                                            )
                                        )))
                                        .child(div().child(format!(
                                            "{}: {}",
                                            t(Str::CleanerEntriesScanned, cx),
                                            self.state.total_scanned_entries()
                                        ))),
                                )
                                .when_some(self.state.cleanup_report(), |this, report| {
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
                                    div()
                                        .id("cleaner-results-scroll")
                                        .flex_1()
                                        .min_h_0()
                                        .rounded(cx.theme().radius)
                                        .border_1()
                                        .border_color(cx.theme().border)
                                        .overflow_scroll()
                                        .p_2()
                                        .when_some(
                                            self.state.result_for(self.state.category()),
                                            |container, result| {
                                                container.child(
                                                    v_flex()
                                                        .gap_2()
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
                                                        .children(result.items.iter().map(|item| {
                                                            let path_text =
                                                                item.path.display().to_string();
                                                            let item_id = item.id;
                                                            let reveal_path = item.path.clone();
                                                            let selected = self.state.is_selected(item.id);
                                                            let can_cleanup = item
                                                                .capabilities
                                                                .contains(&ItemCapability::MoveToTrash);
                                                            let can_reveal = item
                                                                .capabilities
                                                                .contains(&ItemCapability::RevealInFinder);
                                                            let can_uninstall = item
                                                                .capabilities
                                                                .contains(&ItemCapability::UninstallApplication);
                                                            let uninstall_item = item.clone();
                                                            v_flex()
                                                                .gap_1()
                                                                .rounded(cx.theme().radius)
                                                                .border_1()
                                                                .border_color(cx.theme().border)
                                                                .p_2()
                                                                .child(
                                                                    h_flex()
                                                                        .justify_between()
                                                                        .items_center()
                                                                        .gap_2()
                                                                        .child(
                                                                            v_flex()
                                                                                .min_w_0()
                                                                                .child(
                                                                                    div()
                                                                                        .font_bold()
                                                                                        .child(
                                                                                            item.display_name
                                                                                                .clone(),
                                                                                        ),
                                                                                )
                                                                                .child(
                                                                                    div()
                                                                                        .text_sm()
                                                                                        .text_color(
                                                                                            cx.theme()
                                                                                                .muted_foreground,
                                                                                        )
                                                                                        .child(
                                                                                            Self::risk_badge(
                                                                                                item.risk,
                                                                                            ),
                                                                                        ),
                                                                                ),
                                                                        )
                                                                        .child(
                                                                            h_flex()
                                                                                .items_center()
                                                                                .gap_2()
                                                                                .when(can_cleanup, |row| {
                                                                                    row.child(
                                                                                        Button::new((
                                                                                            "cleaner-select-item",
                                                                                            item_id.0,
                                                                                        ))
                                                                                        .ghost()
                                                                                        .label(t(
                                                                                            Self::select_label(
                                                                                                selected,
                                                                                            ),
                                                                                            cx,
                                                                                        ))
                                                                                        .on_click(cx.listener(
                                                                                            move |this, _, _, cx| {
                                                                                                this.toggle_selected(
                                                                                                    item_id,
                                                                                                    cx,
                                                                                                )
                                                                                            },
                                                                                        )),
                                                                                    )
                                                                                })
                                                                                .child(
                                                                                    div().child(
                                                                                        Self::format_bytes(
                                                                                            item.logical_size,
                                                                                        ),
                                                                                    ),
                                                                                )
                                                                                .when(can_reveal, |row| {
                                                                                    row.child(
                                                                                        Button::new((
                                                                                            "cleaner-reveal",
                                                                                            item_id.0,
                                                                                        ))
                                                                                        .ghost()
                                                                                        .label(t(
                                                                                            Str::CleanerRevealInFinder,
                                                                                            cx,
                                                                                        ))
                                                                                        .on_click(cx.listener(
                                                                                            move |this, _, window, cx| {
                                                                                                this.reveal_in_finder(
                                                                                                    reveal_path.clone(),
                                                                                                    window,
                                                                                                    cx,
                                                                                                )
                                                                                            },
                                                                                        )),
                                                                                    )
                                                                                })
                                                                                .child(
                                                                                    Button::new((
                                                                                        "cleaner-copy-path",
                                                                                        item_id.0,
                                                                                    ))
                                                                                    .ghost()
                                                                                    .label(t(
                                                                                        Str::CleanerCopyPath,
                                                                                        cx,
                                                                                    ))
                                                                                    .on_click(cx.listener(
                                                                                        move |_, _, _, cx| {
                                                                                            cx.write_to_clipboard(
                                                                                                gpui::ClipboardItem::new_string(
                                                                                                    path_text
                                                                                                        .clone(),
                                                                                                ),
                                                                                            );
                                                                                        },
                                                                                    )),
                                                                                )
                                                                                .when(can_uninstall, |row| {
                                                                                    row.child(
                                                                                        Button::new((
                                                                                            "cleaner-begin-uninstall",
                                                                                            item_id.0,
                                                                                        ))
                                                                                        .ghost()
                                                                                        .label(t(
                                                                                            Str::CleanerBeginUninstallReview,
                                                                                            cx,
                                                                                        ))
                                                                                        .on_click(cx.listener(
                                                                                            move |this, _, window, cx| {
                                                                                                this.begin_uninstall_review(
                                                                                                    uninstall_item.clone(),
                                                                                                    window,
                                                                                                    cx,
                                                                                                );
                                                                                            },
                                                                                        )),
                                                                                    )
                                                                                }),
                                                                        ),
                                                                )
                                                                .child(
                                                                    div()
                                                                        .text_sm()
                                                                        .child(format!(
                                                                            "{}: {}",
                                                                            t(Str::CleanerPath, cx),
                                                                            item.path.display()
                                                                        )),
                                                                )
                                                                .child(
                                                                    div()
                                                                        .text_sm()
                                                                        .text_color(
                                                                            cx.theme()
                                                                                .muted_foreground,
                                                                        )
                                                                        .child(format!(
                                                                            "{}: {}",
                                                                            t(
                                                                                Str::CleanerExplanation,
                                                                                cx,
                                                                            ),
                                                                            item.explanation
                                                                        )),
                                                                )
                                                        })),
                                                )
                                            },
                                        )
                                        .when(
                                            self.state.result_for(self.state.category()).is_none(),
                                            |container| {
                                                container.child(
                                                    div()
                                                        .text_color(cx.theme().muted_foreground)
                                                        .child(t(Str::CleanerNoResultsYet, cx)),
                                                )
                                            },
                                        ),
                                ),
                        ),
                )
            })
    }
}
