use std::sync::Arc;

use gpui::prelude::FluentBuilder as _;
use gpui::{StatefulInteractiveElement as _, *};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::{ActiveTheme, Disableable as _, StyledExt as _, h_flex, v_flex};

use crate::cleaner::core::cancellation::CancellationToken;
use crate::cleaner::core::category::{CleanerCategory, CleanerSection};
use crate::cleaner::core::errors::ScanError;
use crate::cleaner::core::progress::{ProgressSink, ScanProgress};
use crate::cleaner::core::scan_context::ScanContext;
use crate::cleaner::core::scanner::CleanerScanner;
use crate::cleaner::state::{CleanerState, CleanerStatus, default_scanners};
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
    scan_task: Option<Task<()>>,
    pump_task: Option<Task<()>>,
    progress_rx: Option<std::sync::mpsc::Receiver<ScanProgress>>,
    cancellation: Option<CancellationToken>,
}

impl CleanerView {
    pub fn new(_window: &mut Window, _cx: &mut Context<Self>) -> Self {
        Self {
            state: CleanerState::default(),
            scanners: default_scanners(),
            scan_task: None,
            pump_task: None,
            progress_rx: None,
            cancellation: None,
        }
    }

    fn supported_platform() -> bool {
        cfg!(target_os = "macos")
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
        let cancellation_for_scan = cancellation.clone();
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

                    for scanner in scanners {
                        if cancellation_for_scan.is_cancelled() {
                            cancelled = true;
                            break;
                        }
                        match scanner.scan(&context, &sink, &cancellation_for_scan) {
                            Ok(result) => results.push(result),
                            Err(ScanError::Cancelled) => {
                                cancelled = true;
                                break;
                            }
                            Err(_) => had_failures = true,
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

    fn section_categories(&self) -> Vec<CleanerCategory> {
        if self.state.section() == CleanerSection::SmartCare {
            CleanerCategory::ALL.to_vec()
        } else {
            CleanerCategory::categories_for(self.state.section()).collect()
        }
    }

    fn format_bytes(bytes: u64) -> String {
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
}

impl Render for CleanerView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let is_scanning = matches!(
            self.state.status(),
            CleanerStatus::Scanning | CleanerStatus::Cancelling
        );

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
                                                        .disabled(is_scanning)
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
                                    div()
                                        .text_color(cx.theme().muted_foreground)
                                        .child(t(Self::status_label(self.state.status()), cx)),
                                )
                                .when_some(self.state.progress(), |this, progress| {
                                    this.child(div().text_sm().child(format!(
                                        "{} · {} · {}",
                                        t(Str::CleanerStatusProgress, cx),
                                        progress.scanned_entries,
                                        progress.discovered_items
                                    )))
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
                                                container.children(result.items.iter().map(|item| {
                                                    h_flex()
                                                        .justify_between()
                                                        .w_full()
                                                        .py_1()
                                                        .child(item.display_name.clone())
                                                        .child(Self::format_bytes(
                                                            item.logical_size,
                                                        ))
                                                }))
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
