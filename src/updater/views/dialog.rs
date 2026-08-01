//! The update dialog: the only place a user sees the updater, and the only
//! place that starts one.
//!
//! # It is a `window.open_dialog`, and the body is an entity
//!
//! Both for the reasons `docker::views::detail`'s module doc sets out and
//! `AGENTS.md` repeats. A hand-rolled `div().absolute().inset_0()` scrim cannot
//! be modal — it covers only its positioned ancestor and swallows nothing — and
//! a dialog layer does **not** repaint on the page's `cx.notify()`, so the body
//! has to be an [`Entity`] whose own `cx.notify()` paints it. That matters more
//! here than it did there: this body repaints many times a second while a
//! download runs.
//!
//! The card's width is **stated**, never `w_full`: a percentage width resolves
//! to `auto` inside the dialog's wrappers and content-sizes the body to its
//! widest child. Everything inside it is bounded to that width and that height
//! for one blunt reason: what overflows the card lands on the dialog's
//! **overlay**, so a button pushed past the edge is not just clipped — clicking
//! it dismisses the dialog. See [`UpdateDialog::render_footer`].
//!
//! # There is only ever one
//!
//! Two things open it — the sidebar button and a background check that found
//! something — and they used to stack. [`claim`] gives them one slot between
//! them, released by every close path.
//!
//! # It drives IO, and performs none
//!
//! Every button hands work to
//! [`services::pipeline`](crate::updater::services::pipeline) on the background
//! executor and gets [`UpdateEvent`]s back through a channel; the events go
//! into [`UpdaterMachine`], and this file renders whatever the machine then
//! says. **Nothing here reads a file or opens a socket** — not even to check
//! whether the downloaded archive exists.
//!
//! # Check silently, ask before downloading
//!
//! Decided with the captain. A background check that finds nothing is invisible;
//! one that finds something opens this dialog through [`open_with`]. Nothing is
//! ever downloaded until **Download and install** is pressed, and that is
//! structural rather than remembered — see `pipeline`'s module doc.

use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::Duration;

use gpui::prelude::FluentBuilder as _;
use gpui::{
    AnyElement, App, AppContext as _, Context, Entity, InteractiveElement as _, IntoElement,
    ParentElement as _, Render, SharedString, StatefulInteractiveElement as _, Styled as _, Task,
    Window, div, px,
};
use gpui_component::button::{Button, ButtonVariants as _};
use gpui_component::checkbox::Checkbox;
use gpui_component::progress::Progress;
use gpui_component::{ActiveTheme as _, Icon, StyledExt as _, WindowExt as _, h_flex, v_flex};

use crate::app_icon::AppIcon;
use crate::build_info::VERSION_INFO;
use crate::i18n::{Str, t};
use crate::updater::models::state::{InstallOutcome, UpdateEvent, UpdateInfo, UpdaterState};
use crate::updater::services::{Cancellation, log, pipeline};
use crate::updater::state::machine::{RetryFrom, UpdaterMachine};
use crate::updater::{Updater, UpdaterServices};

/// The card's preferred width, and the body's height.
///
/// Both *preferred*: [`card_size`] shrinks them for a small window, exactly as
/// `docker::views::detail` does — `Dialog` computes `left` from the width it is
/// given, so an over-wide card is pushed off both edges rather than clipped.
const PANEL_W: gpui::Pixels = px(560.);
const PANEL_H: gpui::Pixels = px(340.);
const PANEL_MARGIN: gpui::Pixels = px(24.);
/// `Dialog`'s own horizontal padding (`Edges::all(16)`), subtracted to get the
/// body's width from the card's.
const DIALOG_PADDING_X: gpui::Pixels = px(32.);

/// How often the UI task drains events the background job produced.
///
/// A poll rather than a wakeup because the job runs on the background executor
/// and may not touch the UI thread; 40 ms is under a frame at 24 fps, so the
/// progress bar moves smoothly, and it costs nothing when no job is running —
/// the loop only exists for the duration of one.
const PUMP_INTERVAL: Duration = Duration::from_millis(40);

/// Opens the dialog and starts a check straight away — the **Check for
/// updates** button in the sidebar footer.
pub fn open(window: &mut Window, cx: &mut App) {
    if !claim(window, cx) {
        return;
    }
    let services = Updater::services(cx);
    let config = Updater::config(cx);
    let view = cx.new(|cx| {
        let mut dialog = UpdateDialog::new(UpdaterMachine::new(), services, config);
        dialog.start_check(cx);
        dialog
    });
    present(view, window, cx);
}

/// Opens the dialog on an update a background check already found, so the check
/// is not repeated.
pub fn open_with(info: UpdateInfo, window: &mut Window, cx: &mut App) {
    if !claim(window, cx) {
        return;
    }
    let services = Updater::services(cx);
    let config = Updater::config(cx);
    let view = cx.new(|_| UpdateDialog::new(UpdaterMachine::holding(info), services, config));
    present(view, window, cx);
}

/// Whether an update dialog is on screen.
///
/// There are two ways in — the sidebar button and a background check that found
/// something — and nothing stopped them stacking, so a launch that found an
/// update while the user was pressing **Check for updates** put two identical
/// dialogs on top of each other. A global rather than a field because the thing
/// being tracked is not a property of any one dialog.
#[derive(Default)]
struct DialogOnScreen(bool);

impl gpui::Global for DialogOnScreen {}

fn set_on_screen(on_screen: bool, cx: &mut App) {
    cx.set_global(DialogOnScreen(on_screen));
}

/// What an open request should do, given what is believed and what is true.
///
/// Split out as a pure function so the recovery case has a test: the flag is set
/// by [`present`] and cleared by every close path, and if one of them were ever
/// missed the dialog could never be opened again. `window` is the authority on
/// what is actually on screen, so a flag that outlived its dialog is corrected
/// rather than believed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum OpenDecision {
    /// Nothing is showing; open one.
    Open,
    /// Ours is already showing; leave it alone.
    AlreadyShowing,
    /// The flag is stale — no dialog is on screen at all. Clear it and open.
    ClearStaleAndOpen,
}

fn decide_open(believed_on_screen: bool, any_dialog_on_screen: bool) -> OpenDecision {
    match (believed_on_screen, any_dialog_on_screen) {
        (false, _) => OpenDecision::Open,
        (true, true) => OpenDecision::AlreadyShowing,
        (true, false) => OpenDecision::ClearStaleAndOpen,
    }
}

/// Takes the single update-dialog slot, or reports that it is taken.
fn claim(window: &mut Window, cx: &mut App) -> bool {
    let believed = cx.try_global::<DialogOnScreen>().is_some_and(|d| d.0);
    match decide_open(believed, window.has_active_dialog(cx)) {
        OpenDecision::AlreadyShowing => false,
        OpenDecision::Open => true,
        OpenDecision::ClearStaleAndOpen => {
            set_on_screen(false, cx);
            true
        }
    }
}

/// Closes the dialog from one of its own buttons.
///
/// `window.close_dialog` does not run the `on_close` handler — that fires for
/// the close button, the overlay and escape — so releasing the slot has to
/// happen here too, or a dialog dismissed with **Later** would block every
/// later one.
fn close(window: &mut Window, cx: &mut App) {
    set_on_screen(false, cx);
    window.close_dialog(cx);
}

fn present(view: Entity<UpdateDialog>, window: &mut Window, cx: &mut App) {
    set_on_screen(true, cx);
    window.open_dialog(cx, move |dialog, window, cx| {
        let view = view.clone();
        let closing = view.clone();
        let (card_w, body_h) = card_size(window);
        // Clicking the backdrop must not abandon a running download or, worse,
        // an install: while work is in flight the only ways out are Cancel and
        // the close button, both of which go through `abandon`.
        let busy = view.read(cx).machine.state().is_busy();
        dialog
            .w(card_w)
            .overlay_closable(!busy)
            .title(t(Str::SoftwareUpdate, cx))
            // Closing the dialog abandons whatever it started. The background
            // job is not stopped by dropping the view's task — that only stops
            // the UI listening — so the flag has to be set explicitly.
            .on_close(move |_, _, cx| {
                closing.update(cx, |this, _| this.abandon());
                set_on_screen(false, cx);
            })
            // `content`, not `child`: plain children are wrapped in an
            // `overflow_y_scrollbar` box, which takes its width from its
            // content and collapses every `w_full` inside.
            .content(move |content, _, _| {
                content.child(
                    div()
                        .w(card_w - DIALOG_PADDING_X)
                        .h(body_h)
                        .child(view.clone()),
                )
            })
    });
}

fn card_size(window: &Window) -> (gpui::Pixels, gpui::Pixels) {
    card_size_for(window.viewport_size())
}

/// The card width and body height for one viewport: the preferred size, shrunk
/// to leave [`PANEL_MARGIN`] around the card in a window smaller than that.
///
/// Split out from [`card_size`] so it can be tested without a `Window`. That is
/// worth doing rather than eyeballing: `Dialog` centres the card by computing
/// `left` from the width it was given, so a card wider than the window is not
/// merely clipped — it is pushed off *both* edges, and the buttons go with it.
fn card_size_for(viewport: gpui::Size<gpui::Pixels>) -> (gpui::Pixels, gpui::Pixels) {
    (
        PANEL_W.min(viewport.width - PANEL_MARGIN * 2.),
        PANEL_H.min(viewport.height - PANEL_MARGIN * 4.),
    )
}

/// The dialog body.
pub struct UpdateDialog {
    machine: UpdaterMachine,
    services: UpdaterServices,
    config: crate::updater::models::config::UpdaterConfig,
    /// The UI-side pump. Dropping it stops the dialog listening; [`abandon`]
    /// is what stops the *work*.
    ///
    /// [`abandon`]: UpdateDialog::abandon
    task: Option<Task<()>>,
    cancel: Cancellation,
}

impl UpdateDialog {
    fn new(
        machine: UpdaterMachine,
        services: UpdaterServices,
        config: crate::updater::models::config::UpdaterConfig,
    ) -> Self {
        Self {
            machine,
            services,
            config,
            task: None,
            cancel: Cancellation::new(),
        }
    }

    /// Stops whatever is in flight and forgets the pump.
    fn abandon(&mut self) {
        self.cancel.cancel();
        self.task = None;
    }

    // ---- Starting work -------------------------------------------------------

    fn start_check(&mut self, cx: &mut Context<Self>) {
        if !self.machine.apply(UpdateEvent::CheckingStarted) {
            return;
        }
        cx.notify();

        let Ok(current) = pipeline::current_version() else {
            // `build_info`'s own tests rule this out; refusing to guess is
            // still better than unwrapping in a released binary.
            self.machine.apply(UpdateEvent::Error(
                crate::updater::models::state::UpdateError::Install(
                    VERSION_INFO.version.to_owned(),
                ),
            ));
            cx.notify();
            return;
        };

        let source = self.services.source.clone();
        let config = self.config.clone();
        self.cancel = Cancellation::new();

        self.task = Some(cx.spawn(async move |this, cx| {
            let (tx, rx) = channel::<UpdateEvent>();
            cx.background_executor()
                .spawn(async move {
                    let _ = pipeline::check(source.as_ref(), &config, &current, &|event| {
                        let _ = tx.send(event);
                    });
                })
                .detach();
            pump(rx, this, cx).await;
        }));
    }

    fn start_download(&mut self, cx: &mut Context<Self>) {
        let Some(info) = self.machine.state().info().cloned() else {
            return;
        };

        // Downloading a version is the clearest possible statement that it is
        // no longer skipped.
        if self.config.skipped_version.is_some() {
            self.config.clear_skip();
            self.persist_config(cx);
        }

        if !self.machine.apply(UpdateEvent::DownloadStarted) {
            return;
        }
        cx.notify();

        let services = self.services.clone();
        let cancel = Cancellation::new();
        self.cancel = cancel.clone();
        let staging = pipeline::staging_directory();

        self.task = Some(cx.spawn(async move |this, cx| {
            let (tx, rx) = channel::<UpdateEvent>();
            cx.background_executor()
                .spawn(async move {
                    let _ = pipeline::download_and_install(
                        services.downloader.as_ref(),
                        services.verifier.as_ref(),
                        services.installer.as_ref(),
                        &info,
                        &staging,
                        &cancel,
                        &|event| {
                            let _ = tx.send(event);
                        },
                    );
                })
                .detach();
            pump(rx, this, cx).await;
        }));
    }

    // ---- Buttons -------------------------------------------------------------

    fn cancel(&mut self, cx: &mut Context<Self>) {
        self.cancel.cancel();
        self.task = None;
        if self.machine.cancel() {
            cx.notify();
        }
    }

    fn retry(&mut self, cx: &mut Context<Self>) {
        match self.machine.retry() {
            Some(RetryFrom::Check) => self.start_check(cx),
            Some(RetryFrom::Download(_)) => self.start_download(cx),
            None => {}
        }
    }

    fn skip(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(version) = self.machine.skip() {
            self.config.skip(&version);
            self.persist_config(cx);
        }
        close(window, cx);
    }

    fn restart(&mut self, cx: &mut Context<Self>) {
        let installer = self.services.installer.clone();
        cx.spawn(async move |_, cx| {
            let started = cx
                .background_executor()
                .spawn(async move { installer.relaunch() })
                .await;
            match started {
                Ok(()) => {
                    cx.update(|cx| cx.quit());
                }
                // A relaunch that failed leaves the *installed* update in place;
                // the user restarts dodo themselves and gets it. Nothing is
                // broken, so this is a note rather than an error state.
                Err(error) => log::problem(&format!("could not relaunch: {error:?}")),
            }
        })
        .detach();
    }

    fn set_auto_check(&mut self, on: bool, cx: &mut Context<Self>) {
        self.config.auto_update = on;
        self.persist_config(cx);
        cx.notify();
    }

    /// Writes the settings to disk on the background executor, and updates the
    /// global so the periodic checker sees the change without a restart.
    fn persist_config(&self, cx: &mut Context<Self>) {
        let config = self.config.clone();
        let store = self.services.store.clone();
        cx.spawn(async move |_, cx| {
            let saved = cx
                .background_executor()
                .spawn({
                    let config = config.clone();
                    async move { store.persist(&config) }
                })
                .await;
            if let Err(error) = saved {
                log::problem(&format!("could not save updater.json: {error:?}"));
            }
            cx.update(|cx| Updater::set_config(config, cx));
        })
        .detach();
    }
}

/// Drains events onto the machine until the sender is dropped, which happens
/// when the background job returns.
///
/// A `try_recv` and a timer rather than a blocking `recv`, because this runs on
/// the UI task: blocking here would freeze the window. Returning early when the
/// dialog is gone is what makes closing it stop the pump.
async fn pump(
    rx: Receiver<UpdateEvent>,
    this: gpui::WeakEntity<UpdateDialog>,
    cx: &mut gpui::AsyncApp,
) {
    loop {
        match rx.try_recv() {
            Ok(event) => {
                let alive = this.update(cx, |this, cx| {
                    if this.machine.apply(event) {
                        cx.notify();
                    }
                });
                if alive.is_err() {
                    return;
                }
            }
            Err(TryRecvError::Empty) => cx.background_executor().timer(PUMP_INTERVAL).await,
            Err(TryRecvError::Disconnected) => return,
        }
    }
}

impl Render for UpdateDialog {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        v_flex()
            .size_full()
            // A flex item defaults to `min-width: auto`, so without this the
            // widest child — a release-note line, an error naming two digests —
            // would set the body's width and push the buttons off the card. The
            // same rule the API Explorer's columns carry.
            .min_w_0()
            .justify_between()
            .gap_3()
            // `overflow_hidden` is the guarantee, not the sizing: whatever the
            // body's own height arithmetic does, nothing it renders may paint
            // over the footer and take the buttons' clicks with it.
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .child(self.render_body(cx)),
            )
            .child(self.render_footer(cx))
    }
}

impl UpdateDialog {
    fn render_body(&self, cx: &Context<Self>) -> AnyElement {
        match self.machine.state() {
            UpdaterState::Idle | UpdaterState::Checking => {
                status(Str::UpdateChecking, self.current_version_line(cx), cx)
            }
            UpdaterState::Completed => {
                status(Str::UpdateUpToDate, self.current_version_line(cx), cx)
            }
            UpdaterState::UpdateAvailable(info) => self.render_available(info, cx),
            UpdaterState::Downloading { progress, .. } => v_flex()
                .gap_3()
                .child(div().text_sm().child(t(
                    Str::UpdateDownloadProgress {
                        done: format_size(progress.downloaded),
                        total: format_size(progress.total),
                        percent: progress.percent,
                    },
                    cx,
                )))
                .child(Progress::new("update-progress").value(f32::from(progress.percent)))
                .into_any_element(),
            // The gap between the bytes landing and verification starting is a
            // task hop; showing "verifying" for both is honest and avoids a
            // flicker through a third caption.
            UpdaterState::Downloaded { .. } | UpdaterState::Verifying { .. } => {
                status(Str::UpdateVerifying, None, cx)
            }
            UpdaterState::Installing { .. } => status(Str::UpdateInstalling, None, cx),
            UpdaterState::ReadyToRestart { info, outcome } => {
                self.render_installed(info, outcome, cx)
            }
            UpdaterState::Failed { .. } => v_flex()
                .gap_2()
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(
                            Icon::new(AppIcon::AlertTriangle)
                                .size(px(14.))
                                .text_color(cx.theme().danger),
                        )
                        .child(
                            div()
                                .font_bold()
                                .flex_shrink_0()
                                .whitespace_nowrap()
                                .child(t(Str::UpdateFailedHeadline, cx)),
                        ),
                )
                .children(self.machine.error().map(|error| {
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(t(error.message(), cx))
                }))
                .into_any_element(),
        }
    }

    fn render_available(&self, info: &UpdateInfo, cx: &Context<Self>) -> AnyElement {
        v_flex()
            .gap_2()
            .min_w_0()
            // The release notes are the only body that can be arbitrarily long,
            // and the scroll box below only bounds itself if this column is
            // bounded first: `h_full` ties it to the body's height so `flex_1`
            // there has a remainder to compute, rather than a column that grows
            // to whatever the release wrote.
            .h_full()
            .min_h_0()
            .child(div().font_bold().child(t(
                Str::UpdateAvailableHeadline(info.parsed.to_display()),
                cx,
            )))
            .child(
                h_flex()
                    .gap_3()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    // Two short fixed-length facts side by side: they must not
                    // wrap mid-phrase when the window is narrow.
                    .child(
                        div()
                            .flex_shrink_0()
                            .whitespace_nowrap()
                            .child(t(Str::UpdatePublished(info.published_at.clone()), cx)),
                    )
                    .child(
                        div()
                            .flex_shrink_0()
                            .whitespace_nowrap()
                            .child(t(Str::UpdateDownloadSize(format_size(info.file.size)), cx)),
                    ),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(t(Str::UpdateReleaseNotes, cx)),
            )
            .child(
                div()
                    .id("update-release-notes")
                    .flex_1()
                    .min_h_0()
                    .min_w_0()
                    .overflow_y_scroll()
                    .rounded(cx.theme().radius)
                    .border_1()
                    .border_color(cx.theme().border)
                    .p_2()
                    .text_sm()
                    .font_family(cx.theme().mono_font_family.clone())
                    // The release notes are the *release's* text, not dodo's:
                    // they arrive in the manifest and are shown verbatim, which
                    // is why they do not go through `Str`.
                    .child(SharedString::from(info.notes.clone())),
            )
            .into_any_element()
    }

    fn render_installed(
        &self,
        info: &UpdateInfo,
        outcome: &InstallOutcome,
        cx: &Context<Self>,
    ) -> AnyElement {
        let headline = div().font_bold().child(t(
            Str::UpdateInstalledHeadline(info.parsed.to_display()),
            cx,
        ));

        match outcome {
            InstallOutcome::Installed => v_flex()
                .gap_2()
                .child(headline)
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(t(self.current_version_str(), cx)),
                )
                .into_any_element(),
            // Not a failure: the archive is downloaded and verified, and this
            // says why it could not be put in place and where it is.
            InstallOutcome::Manual { reason, archive } => v_flex()
                .gap_2()
                .min_w_0()
                .child(div().font_bold().child(t(
                    Str::UpdateManualInstall(archive.display().to_string()),
                    cx,
                )))
                .child(
                    div()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child(t(reason.message(), cx)),
                )
                .into_any_element(),
        }
    }

    fn current_version_str(&self) -> Str {
        Str::UpdateCurrentVersion(VERSION_INFO.version.to_owned())
    }

    fn current_version_line(&self, _cx: &Context<Self>) -> Option<Str> {
        Some(self.current_version_str())
    }

    /// The checkbox and the action buttons on one row.
    ///
    /// The checkbox sits in a `flex_1().min_w_0()` cell and the buttons in a
    /// `flex_shrink_0()` one, and that split is load-bearing rather than
    /// decorative. A flex item defaults to `min-width: auto`, so a checkbox
    /// allowed to claim its full label width pushes the buttons off the right of
    /// the card — and a button pushed past the card's edge is not merely
    /// truncated: the part outside the card belongs to the dialog's overlay, so
    /// clicking what looks like **Download and install** dismisses the dialog
    /// instead. That is exactly what "the download button does nothing" was.
    /// The label wraps to a second line instead, which costs a few pixels of
    /// height the body already yields (`justify_between` above), and no
    /// translation of it can ever reach the buttons.
    fn render_footer(&self, cx: &mut Context<Self>) -> AnyElement {
        h_flex()
            .w_full()
            .min_w_0()
            .items_center()
            .gap_2()
            .child(
                div().flex_1().min_w_0().child(
                    Checkbox::new("updater-auto-check")
                        .label(t(Str::UpdateCheckAutomatically, cx))
                        .checked(self.config.auto_update)
                        .on_click(cx.listener(|this, checked: &bool, _, cx| {
                            this.set_auto_check(*checked, cx)
                        })),
                ),
            )
            .child(
                h_flex()
                    .gap_2()
                    .flex_shrink_0()
                    .justify_end()
                    .children(self.actions(cx)),
            )
            .into_any_element()
    }

    /// The buttons for the current state, right to left in importance.
    fn actions(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        match self.machine.state() {
            UpdaterState::Idle | UpdaterState::Checking => vec![
                Button::new("updater-cancel")
                    .label(t(Str::UpdateCancel, cx))
                    .on_click(cx.listener(|this, _, _, cx| this.cancel(cx)))
                    .into_any_element(),
            ],
            UpdaterState::UpdateAvailable(_) => vec![
                Button::new("updater-skip")
                    .ghost()
                    .label(t(Str::UpdateSkipVersion, cx))
                    .on_click(cx.listener(|this, _, window, cx| this.skip(window, cx)))
                    .into_any_element(),
                Button::new("updater-download")
                    .primary()
                    .label(t(Str::UpdateDownloadAction, cx))
                    .on_click(cx.listener(|this, _, _, cx| this.start_download(cx)))
                    .into_any_element(),
            ],
            UpdaterState::Downloading { .. }
            | UpdaterState::Downloaded { .. }
            | UpdaterState::Verifying { .. } => vec![
                Button::new("updater-cancel")
                    .label(t(Str::UpdateCancel, cx))
                    .on_click(cx.listener(|this, _, _, cx| this.cancel(cx)))
                    .into_any_element(),
            ],
            // No cancel: an install that has begun cannot be abandoned safely.
            UpdaterState::Installing { .. } => Vec::new(),
            UpdaterState::ReadyToRestart { outcome, .. } => {
                let mut actions = vec![
                    Button::new("updater-later")
                        .ghost()
                        .label(t(Str::UpdateLater, cx))
                        .on_click(|_, window, cx| close(window, cx))
                        .into_any_element(),
                ];
                // There is nothing to restart into when the install was refused;
                // the user has an archive to unpack instead.
                if matches!(outcome, InstallOutcome::Installed) {
                    actions.push(
                        Button::new("updater-restart")
                            .primary()
                            .label(t(Str::UpdateRestartNow, cx))
                            .on_click(cx.listener(|this, _, _, cx| this.restart(cx)))
                            .into_any_element(),
                    );
                }
                actions
            }
            UpdaterState::Completed => vec![
                Button::new("updater-close")
                    .label(t(Str::UpdateLater, cx))
                    .on_click(|_, window, cx| close(window, cx))
                    .into_any_element(),
            ],
            UpdaterState::Failed { .. } => vec![
                Button::new("updater-close")
                    .ghost()
                    .label(t(Str::UpdateLater, cx))
                    .on_click(|_, window, cx| close(window, cx))
                    .into_any_element(),
                Button::new("updater-retry")
                    .primary()
                    .label(t(Str::UpdateRetry, cx))
                    .on_click(cx.listener(|this, _, _, cx| this.retry(cx)))
                    .into_any_element(),
            ],
        }
    }
}

/// A caption with an optional second line under it — the shape every waiting
/// state and the up-to-date state share.
fn status(headline: Str, detail: Option<Str>, cx: &App) -> AnyElement {
    v_flex()
        .gap_2()
        .child(div().font_bold().child(t(headline, cx)))
        .when_some(detail, |this, detail| {
            this.child(
                div()
                    .text_sm()
                    .text_color(cx.theme().muted_foreground)
                    .child(t(detail, cx)),
            )
        })
        .into_any_element()
}

/// Byte counts for the download line, in the SI units the rest of dodo uses —
/// `docker::models::size` makes the same choice for the same reason (it is what
/// the tools a developer already reads print).
///
/// A local copy rather than a call into `docker`: coupling the updater to a
/// tool's model layer to save eight lines would be the wrong trade, and the two
/// can diverge without either being wrong.
fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "kB", "MB", "GB", "TB"];
    if bytes < 1000 {
        return format!("{bytes}B");
    }
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1000.0 && unit < UNITS.len() - 1 {
        size /= 1000.0;
        unit += 1;
    }
    format!("{size:.1}{}", UNITS[unit])
}

/// Builds the real service bundle. Here rather than in `mod.rs` so that
/// `UpdaterServices`' only construction site sits beside its only consumer.
pub(crate) fn default_services() -> UpdaterServices {
    use crate::updater::services::config_store::DiskUpdaterConfigStore;
    use crate::updater::services::download::HttpDownloader;
    use crate::updater::services::installers::platform_installer;
    use crate::updater::services::manifest_source::HttpManifestSource;
    use crate::updater::services::verify::Sha256Verifier;

    UpdaterServices {
        source: Arc::new(HttpManifestSource::new()),
        downloader: Arc::new(HttpDownloader::new()),
        verifier: Arc::new(Sha256Verifier::new()),
        installer: platform_installer(),
        store: Arc::new(DiskUpdaterConfigStore::new()),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DIALOG_PADDING_X, OpenDecision, PANEL_H, PANEL_MARGIN, PANEL_W, card_size_for, decide_open,
        format_size,
    };
    use gpui::{px, size};

    /// The narrow end. dodo opens at 900x620 and its window can be dragged well
    /// below that; the card has to shrink rather than be pushed off-centre.
    #[test]
    fn the_card_fits_inside_a_narrow_window() {
        for (w, h) in [(480., 400.), (600., 480.), (760., 620.)] {
            let (card_w, body_h) = card_size_for(size(px(w), px(h)));
            assert!(
                card_w <= px(w) - PANEL_MARGIN * 2.,
                "at {w}x{h} the card is {card_w:?}, wider than the window leaves room for"
            );
            assert!(
                body_h <= px(h),
                "at {w}x{h} the body is taller than the window"
            );
            assert!(
                card_w > DIALOG_PADDING_X,
                "at {w}x{h} the card is narrower than its own padding, so the body \
                 would compute a negative width"
            );
        }
    }

    /// The wide end: the card stops growing at its preferred size rather than
    /// stretching a two-line dialog across a 5K display.
    #[test]
    fn the_card_stops_growing_on_a_wide_window() {
        for (w, h) in [(900., 620.), (1280., 800.), (3840., 2160.)] {
            assert_eq!(
                card_size_for(size(px(w), px(h))),
                (PANEL_W, PANEL_H),
                "at {w}x{h}"
            );
        }
    }

    /// dodo's own default window, which is what most people will see.
    #[test]
    fn the_default_window_gets_the_preferred_card() {
        assert_eq!(card_size_for(size(px(900.), px(620.))), (PANEL_W, PANEL_H));
    }

    #[test]
    fn byte_counts_read_the_way_a_download_is_usually_quoted() {
        assert_eq!(format_size(0), "0B");
        assert_eq!(format_size(999), "999B");
        assert_eq!(format_size(1000), "1.0kB");
        assert_eq!(format_size(11_569_143), "11.6MB");
        assert_eq!(format_size(2_500_000_000), "2.5GB");
    }

    #[test]
    fn a_size_larger_than_the_unit_ladder_still_renders() {
        assert!(format_size(u64::MAX).ends_with("TB"));
    }

    // ---- One dialog at a time ------------------------------------------------

    /// The reported defect: a background check that found an update opened a
    /// second, identical dialog over the one the user had opened themselves.
    #[test]
    fn a_second_open_request_does_not_stack_another_dialog() {
        assert_eq!(decide_open(true, true), OpenDecision::AlreadyShowing);
    }

    #[test]
    fn the_first_open_request_opens() {
        for on_screen in [false, true] {
            assert_eq!(
                decide_open(false, on_screen),
                OpenDecision::Open,
                "with no update dialog of our own, another dialog on screen \
                 (settings, say) must not block this one"
            );
        }
    }

    /// The recovery case, and the reason the decision is not just the flag: if a
    /// close path ever failed to release the slot, believing the flag would make
    /// the dialog unopenable for the rest of the session. The window is the
    /// authority on what is actually there.
    #[test]
    fn a_flag_that_outlived_its_dialog_is_corrected_rather_than_believed() {
        assert_eq!(decide_open(true, false), OpenDecision::ClearStaleAndOpen);
    }
}
