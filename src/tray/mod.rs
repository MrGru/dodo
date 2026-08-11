//! The macOS menu bar item: a dodo carrying one keyboard-input-language glyph,
//! and a small native menu.
//!
//! macOS and Windows share this module through `tray-icon`; Linux stays out
//! until its GTK backend is deliberately adopted. The only platform-specific
//! work belongs in [`startup`]: macOS Login Items and Windows' per-user Run key.
//!
//! # What it is not
//!
//! **It reads no application state.** There is no status, no aggregation, no
//! observation of Docker or the database or the API Explorer. The captain
//! cancelled all of that on 2026-08-07 in favour of something purely
//! presentational, and the smallness is the point: do not reintroduce machinery
//! to make it look like the original design.
//!
//! The selected keyboard language is the same [`dodo_ime_core::LanguageId`]
//! the native input method reads from `input-method.json`. dodo's interface
//! language remains a separate display preference.
//!
//! # Why this is `src/tray/` and not `src/platform/tray/`
//!
//! dodo has no `src/platform/`, and nothing else would go in one. `paths.rs`
//! and `window_icon.rs` are both thoroughly platform-shaped and both live at the
//! top level — `window_icon.rs` holds three platforms' answers in one file, split
//! by `#[cfg]` functions rather than by directory. `quick_nav` is the precedent
//! for a feature that is not a tool. And a `macos.rs` here would be nearly
//! empty, because `tray-icon` *is* the platform abstraction: the only genuinely
//! per-platform decisions dodo makes are the template flag and the quit mode.
//!
//! # How events get here without a second event loop
//!
//! This is the part that is easy to get wrong, so it is written down.
//!
//! `tray-icon` and `muda` each keep a global handler slot. dodo installs one in
//! each, and those handlers do exactly one thing: `unbounded_send` on a
//! `futures_channel` mpsc. A single long-lived foreground [`gpui::Task`] awaits
//! the receiver. That is the whole mechanism — **no polling, no timer, no
//! background thread, no per-frame tick, and emphatically no second event
//! loop.** When the queue is empty the task is parked; waking it goes through
//! gpui's own dispatcher onto the main queue, so the work lands on the main
//! thread inside gpui's normal update cycle.
//!
//! Two facts make it correct rather than lucky:
//!
//! - **The handlers run on the main thread already.** `tray-icon` emits from
//!   `NSResponder` methods and `muda` from an `NSMenuItem` action, both
//!   synchronously on the main run loop. The channel is not a thread hop; it is
//!   the way into gpui's borrow discipline.
//! - **The handler bound is `Fn + Send + Sync`**, so it cannot capture an
//!   `AsyncApp` — gpui's `ForegroundExecutor` is `!Send` by construction. An
//!   mpsc sender is `Send + Sync`, which is why the seam is a channel and not a
//!   captured context.
//!
//! **The trap:** both slots are `OnceCell`s, and the senders do
//! `get_or_init(|| None)`. If any event fires before the handler is installed,
//! the slot is locked to `None` for the life of the process and every later
//! installation is silently ignored — events then accumulate unread in a
//! channel nobody holds. So [`install_event_handlers`] runs **before** the menu
//! and the status item exist, and it is called exactly once.
//!
//! # Failure is never fatal
//!
//! [`init`] returns `()`. That is not laziness — it is the structural form of
//! "tray failure must not prevent dodo from starting", and it matches
//! `docker::init`, `quick_nav::init`, `updater::init` and `session::init`.
//! Every fallible step is matched and logged; there is no `unwrap` or `expect`
//! on the init or the runtime path. A dodo with no menu bar item is a working
//! dodo.

pub mod icon;
pub mod menu;
pub mod startup;

use dodo_ime_core::{ActiveLanguages, LanguageId};
use futures_channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use futures_util::StreamExt as _;
use gpui::{App, BorrowAppContext as _, Global, QuitMode, Subscription, Task, WeakEntity};
use tray_icon::menu::MenuEvent;
use tray_icon::{TrayIcon, TrayIconBuilder, TrayIconEvent};

use crate::layout::Layout;
use crate::tray::menu::{TrayCommand, TrayMenu};

/// A step happened, and it is worth a developer seeing.
///
/// dodo has **no logging framework** and adding one was rejected deliberately —
/// see `updater::services::log`, which states the reasoning and which this
/// mirrors rather than reaches into: that module is documented as *the
/// updater's* one diagnostic channel. Promoting both to a shared `src/log.rs`
/// is the obvious later move and is not this feature's to make.
///
/// Developer text, so it does **not** go through
/// [`Str`](crate::i18n::Str). Nothing here reaches the UI.
fn note(message: &str) {
    eprintln!("dodo/tray: {message}");
}

/// Something went wrong, or was refused. Still stderr, and still not an alert:
/// the user-visible consequence of every one of these is a menu bar with no
/// dodo in it, which speaks for itself.
pub(crate) fn problem(message: &str) {
    eprintln!("dodo/tray: {message}");
}

/// The live menu bar item.
///
/// **Holding [`TrayIcon`] here is what keeps the status item on screen**:
/// `TrayIcon`'s `Drop` calls `NSStatusBar::removeStatusItem`, so a version of
/// this that built one into a local would show an icon for exactly as long as
/// `init` ran.
///
/// A `Global` for the same reason [`Session`](crate::session::Session) and
/// [`Updater`](crate::updater::Updater) are: read from anywhere, written from
/// one place. `TrayIcon` is `!Send` — it is `Rc<RefCell<..>>` around an
/// `NSStatusItem` — and that is fine, because `gpui::Global` requires only
/// `'static` and `App` is main-thread-bound anyway.
pub struct Tray {
    /// Held for two reasons: its `Drop` removes the status item, and
    /// [`Tray::set_input_language`] swaps its image.
    icon: TrayIcon,
    menu: TrayMenu,
    /// The selected keyboard input language, shared with `input-method.json`.
    input_language: LanguageId,
    /// The pane, so the Settings row can open the dialog it belongs to.
    ///
    /// Published by [`Layout::new`] rather than fetched from here, because the
    /// window can be closed and rebuilt: every rebuild constructs a new
    /// `Layout` and re-publishes, so the handle refreshes itself. Weak, and a
    /// handle that will not upgrade degrades to "just show the window", which
    /// is the right failure.
    layout: Option<WeakEntity<Layout>>,
    /// The drain task. Held rather than detached so that delivery stops when
    /// the global is dropped, and so the listener cannot outlive the icon it
    /// speaks for.
    #[allow(dead_code, reason = "held for its Drop; nothing reads it")]
    events: Task<()>,
    /// Keeps the menu's own wording in dodo's **interface** language.
    ///
    /// Needed because an `NSMenu` is not a gpui window:
    /// `i18n::Language::set` repaints every window and cannot reach the menu
    /// bar, so the tray has to hear about the change itself. Held rather than
    /// detached, for the same reason as `events`.
    #[allow(dead_code, reason = "held for its Drop; nothing reads it")]
    localization: Subscription,
    /// Moves the app out of the Dock only after its last window has gone.
    #[cfg(target_os = "macos")]
    #[allow(dead_code, reason = "held for its Drop; nothing reads it")]
    dock_visibility: Subscription,
}

impl Global for Tray {}

impl Tray {
    /// Selects `language` for keyboard input: re-checks the menu, swaps the
    /// menu bar mark, and remembers the choice in `session.json`.
    ///
    /// **Idempotent on the icon.** An unchanged language does no rasterising
    /// and makes no AppKit image call at all. What it *does* still do is
    /// re-assert the check marks, because `muda` has already toggled the
    /// clicked row off by the time the event arrives — see
    /// [`TrayMenu::check_only`], which is why the early return is where it is
    /// and not at the top.
    ///
    /// The persisted write is idempotent too, one level down:
    /// `Session::edit` returns early when the change left the document
    /// identical, so re-selecting costs no disk traffic either.
    ///
    /// The same identity is persisted to `input-method.json`, so the native
    /// input method adopts the selected language on its next notification.
    pub fn set_input_language(language: LanguageId, cx: &mut App) {
        if !crate::input_method::InputMethod::active_languages(cx).contains(language) {
            return;
        }
        let Some(tray) = cx.try_global::<Tray>() else {
            return;
        };
        tray.menu.check_only(language);
        if tray.input_language == language {
            return;
        }

        match icon::render(language, cx) {
            Ok(icon) => {
                // One call rather than `set_icon` then `set_icon_as_template`,
                // so the bitmap and the template flag change together and there
                // is no frame with a non-template image in the menu bar.
                let swapped = cx
                    .global::<Tray>()
                    .icon
                    .set_icon_with_as_template(Some(icon), true);
                if let Err(error) = swapped {
                    problem(&format!("could not swap the menu bar mark: {error}"));
                    return;
                }
            }
            Err(error) => {
                // Keep the mark that is already up rather than clearing it: a
                // stale glyph is a smaller lie than an empty menu bar.
                problem(&format!("could not draw the {language:?} mark: {error:?}"));
                return;
            }
        }

        cx.update_global::<Tray, _>(|tray, _| tray.input_language = language);
        crate::input_method::InputMethod::set_language(language, cx);
    }
}

/// The input language to open on, given whatever `session.json` had.
///
/// Pure, so the rule it encodes is a test rather than something to discover by
/// hand-editing a file: **nothing here is a reason to refuse to start.** Never
/// chosen, a code from a later dodo that had a language this build does not,
/// and a hand-typed mistake all land on the default — the same posture
/// `View::shown` takes for a stored tool code.
fn restored_language(stored: Option<String>, fallback: LanguageId) -> LanguageId {
    stored
        .as_deref()
        .and_then(LanguageId::from_code)
        .unwrap_or(fallback)
}

/// Hands the tray the pane it needs for the Settings row.
///
/// Called from [`Layout::new`], which runs once per window — including the
/// window the tray itself rebuilds — so the handle is refreshed rather than
/// stale. A no-op when the tray never came up.
/// Updates the live menu when the shared enabled-language setting changes.
pub fn set_active_languages(active: ActiveLanguages, selected: LanguageId, cx: &mut App) {
    let Some(tray) = cx.try_global::<Tray>() else {
        return;
    };
    tray.menu.set_active_languages(active, selected);
    let needs_selection = tray.input_language != selected;
    if needs_selection {
        Tray::set_input_language(selected, cx);
    }
}

pub fn attach_layout(layout: WeakEntity<Layout>, cx: &mut App) {
    if cx.try_global::<Tray>().is_none() {
        return;
    }
    cx.update_global::<Tray, _>(|tray, _| tray.layout = Some(layout));
}

/// One event from either of the two global channels `tray-icon` owns.
enum Signal {
    Menu(MenuEvent),
    Tray,
}

/// Builds the tray icon and returns whether it is usable.
///
/// It runs on GPUI's event-loop thread. A failed tray must not make startup
/// tray-only, because a windowless process without an icon has no exit path.
pub fn init(cx: &mut App) -> bool {
    let receiver = install_event_handlers();

    let language = restored_language(
        crate::session::Session::legacy_input_language(cx),
        crate::input_method::InputMethod::language(cx),
    );
    // Migrate a pre-IPC tray choice, or make the input-method document match
    // the selection it already supplied. The latter is an idempotent no-op.
    crate::input_method::InputMethod::set_language(language, cx);
    let menu = TrayMenu::new(
        language,
        crate::input_method::InputMethod::active_languages(cx),
        cx,
    );

    let mut builder = TrayIconBuilder::new()
        .with_menu(menu.as_context_menu())
        // Not localized: it names the application, which has one name.
        .with_tooltip("dodo")
        // macOS paints this template mark from its alpha. Windows ignores the
        // flag and uses the same bitmap normally.
        .with_icon_as_template(true)
        // Match a right click: both buttons open the native menu. The explicit
        // Open Dodo command remains the only tray action that shows a window.
        .with_menu_on_left_click(true);

    match icon::render(language, cx) {
        Ok(icon) => builder = builder.with_icon(icon),
        // A status item with no image is a narrow, empty click target — poor,
        // but better than no menu at all, and the menu is where Quit lives.
        Err(error) => problem(&format!("no menu bar mark: {error:?}")),
    }

    let icon = match builder.build() {
        Ok(icon) => icon,
        Err(error) => {
            problem(&format!(
                "no menu bar item, continuing without one: {error}"
            ));
            return false;
        }
    };

    let events = cx.spawn(async move |cx| drain(receiver, cx).await);

    // **Only the menu's own wording follows this**, never the selected input
    // language and never the glyph. The three language rows are endonyms and do
    // not move at all; see `TrayMenu::relabel`.
    let localization = cx.observe_global::<crate::i18n::Language>(|cx| {
        if let Some(tray) = cx.try_global::<Tray>() {
            tray.menu.relabel(cx);
        }
    });

    // `on_window_closed` runs after GPUI has removed the window, which makes
    // this a real last-window check instead of a close callback that guesses.
    #[cfg(target_os = "macos")]
    let dock_visibility = cx.on_window_closed(|cx, _| {
        if cx.windows().is_empty() {
            crate::window_icon::set_macos_dock_visible(false);
        }
    });

    cx.set_global(Tray {
        icon,
        menu,
        input_language: language,
        layout: None,
        events,
        localization,
        #[cfg(target_os = "macos")]
        dock_visibility,
    });

    // **Only now**, and never statically in `main.rs`. Closing the window stops
    // ending the process, which is what the captain asked for — but dodo
    // installs no menu bar, so there is no Cmd-Q, and the tray's Quit becomes
    // the only way out. Deriving the mode from the status item actually
    // existing means the one state that would be unquittable — no window, no
    // menu bar, no tray — cannot be reached: if `build` above had failed, this
    // line was never run and closing the window still quits.
    cx.set_quit_mode(QuitMode::Explicit);
    note("tray ready");
    true
}

/// Points both of `tray-icon`'s global handler slots at one channel.
///
/// **Call this before anything can emit** — see the module doc's note about
/// `OnceCell`. It is called once, from [`init`], before the menu or the status
/// item exists.
fn install_event_handlers() -> UnboundedReceiver<Signal> {
    let (sender, receiver) = unbounded::<Signal>();

    let menu_sender: UnboundedSender<Signal> = sender.clone();
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        // A closed receiver means dodo is shutting down, so the error is
        // deliberately dropped rather than logged from the native callback.
        let _ = menu_sender.unbounded_send(Signal::Menu(event));
    }));

    TrayIconEvent::set_event_handler(Some(move |_event: TrayIconEvent| {
        let _ = sender.unbounded_send(Signal::Tray);
    }));

    receiver
}

/// The one long-lived listener. Parks when the channel is empty; ends when the
/// sender is dropped or the task is.
async fn drain(mut receiver: UnboundedReceiver<Signal>, cx: &mut gpui::AsyncApp) {
    while let Some(signal) = receiver.next().await {
        match signal {
            Signal::Menu(event) => {
                let command = cx.update(|cx| {
                    cx.try_global::<Tray>()
                        .and_then(|tray| tray.menu.command_for(&event.id))
                });

                let Some(command) = command else {
                    continue;
                };

                cx.update(|cx| match command {
                    TrayCommand::OpenDodo => open_dodo(cx),
                    TrayCommand::SelectInputLanguage(language) => {
                        Tray::set_input_language(language, cx)
                    }
                    TrayCommand::OpenSettings => open_settings(cx),
                    TrayCommand::Quit => cx.quit(),
                });
            }
            // Native clicks open the attached menu; only an explicit menu
            // command may show or focus the application window.
            Signal::Tray => {}
        }
    }
}

/// Shows dodo's window and brings it to the front.
///
/// `cx.activate` comes first because raising a window in a background
/// application does not bring the application forward. `cx.windows()` rather
/// than `cx.active_window()` for the reason `main.rs` already documents: the
/// active window is the *key* window and is `None` whenever dodo is not
/// frontmost — which, clicking a menu bar item, it usually is not.
fn open_dodo(cx: &mut App) {
    cx.activate(true);
    if let Some(window) = cx.windows().first().cloned() {
        let _ = window.update(cx, |_, window, _| window.activate_window());
        return;
    }
    // The user closed it and the process stayed. Rebuild through `main`'s own
    // path so the window comes back with its saved rectangle, its saved display
    // and the layout's minimum size — never `WindowOptions::default()`.
    if let Err(error) = crate::open_main_window(cx) {
        problem(&format!("could not reopen the window: {error}"));
    }
}

/// Shows the window, then opens the Settings dialog inside it.
///
/// The dialog is an in-window layer, not an OS window, so it needs a window to
/// live in first — and after a close there may not be one.
fn open_settings(cx: &mut App) {
    open_dodo(cx);

    let Some(layout) = cx.try_global::<Tray>().and_then(|tray| tray.layout.clone()) else {
        // No pane published yet: the window is on screen, which is most of what
        // was asked for, and the user is one click from Settings themselves.
        return;
    };
    let Some(window) = cx.windows().first().cloned() else {
        return;
    };
    let _ = window.update(cx, |_, window, cx| {
        crate::settings::open(layout, window, cx);
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_legacy_menu_choice_uses_the_shared_language_identity() {
        for language in LanguageId::ALL {
            assert_eq!(
                restored_language(Some(language.code().to_owned()), LanguageId::English),
                language
            );
        }
    }

    #[test]
    fn an_unusable_legacy_code_falls_back_to_the_ipc_selection() {
        assert_eq!(
            restored_language(Some("ko".to_owned()), LanguageId::Japanese),
            LanguageId::Japanese,
        );
        assert_eq!(
            restored_language(None, LanguageId::Vietnamese),
            LanguageId::Vietnamese,
        );
    }
}
