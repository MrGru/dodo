//! The macOS menu bar item: a dodo carrying one keyboard-input-language glyph,
//! and a small native menu.
//!
//! macOS only for now. Windows and Linux are *possible* without rewriting
//! anything above the platform line — `tray-icon` implements both, and
//! everything in [`input_language`], [`menu`] and [`icon`] is platform-free —
//! but neither is built or tested here, so the whole module sits behind
//! `#[cfg(target_os = "macos")]` in `main.rs`.
//!
//! # What it is not
//!
//! **It reads no application state.** There is no status, no aggregation, no
//! observation of Docker or the database or the API Explorer. The captain
//! cancelled all of that on 2026-08-07 in favour of something purely
//! presentational, and the smallness is the point: do not reintroduce machinery
//! to make it look like the original design.
//!
//! **It is not dodo's interface language.** [`input_language`] carries that
//! warning in full; the short version is that `tray::InputLanguage` and
//! `i18n::Language` are two settings that must never share a type, a code
//! table, or a `session.json` key.
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
pub mod input_language;
pub mod menu;

use futures_channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use futures_util::StreamExt as _;
use gpui::{App, Global, Task};
use tray_icon::menu::MenuEvent;
use tray_icon::{TrayIcon, TrayIconBuilder};

use crate::tray::input_language::InputLanguage;
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
    /// Held for its `Drop`, and — from phase 3 — for `set_icon_with_as_template`.
    #[allow(
        dead_code,
        reason = "phase 2 only needs this alive; the language switcher reads it. Remove the allow when `Tray::set_input_language` lands."
    )]
    icon: TrayIcon,
    menu: TrayMenu,
    /// The selected **keyboard input** language. Nothing to do with
    /// [`i18n::Language`](crate::i18n::Language), which is dodo's interface
    /// language and lives in its own global.
    #[allow(
        dead_code,
        reason = "written by phase 3's menu; phase 2 only ever holds the default. Remove the allow when `Tray::set_input_language` lands."
    )]
    input_language: InputLanguage,
    /// The drain task. Held rather than detached so that delivery stops when
    /// the global is dropped, and so the listener cannot outlive the icon it
    /// speaks for.
    #[allow(dead_code, reason = "held for its Drop; nothing reads it")]
    events: Task<()>,
}

impl Global for Tray {}

impl Tray {
    /// The selected keyboard input language, or the default before [`init`] has
    /// run.
    ///
    /// **Not dodo's interface language** — that is `i18n::Language::current`.
    #[allow(
        dead_code,
        reason = "the public read side of a value phase 2 cannot yet change. Remove the allow when the Keyboard Input menu lands."
    )]
    pub fn input_language(cx: &App) -> InputLanguage {
        cx.try_global::<Tray>()
            .map_or_else(InputLanguage::default, |tray| tray.input_language)
    }
}

/// One event from either of the two global channels `tray-icon` owns.
///
/// Only menu events carry a command today. Tray events — clicks, enter, leave,
/// move — are drained and dropped: nothing reacts to them, but the channel must
/// still be emptied, and a future floating panel would want the `rect` they
/// carry.
enum Signal {
    Menu(MenuEvent),
}

/// Builds the status item, or says why it could not and returns.
///
/// Must run on the main thread with `NSApp`'s run loop already going. Both hold
/// wherever this is called from `main.rs`: gpui's macOS backend calls
/// `App::run`'s closure from inside `applicationDidFinishLaunching:`, i.e. after
/// `[NSApp run]` has started, and a `cx.spawn` continuation of that closure runs
/// on the foreground executor, which dispatches to the main queue. If that ever
/// stops being true the cost is one logged line: `TrayIcon::new` returns
/// `Error::NotMainThread` rather than panicking.
pub fn init(cx: &mut App) {
    let receiver = install_event_handlers();

    let language = InputLanguage::default();
    let menu = TrayMenu::new(cx);

    let mut builder = TrayIconBuilder::new()
        .with_menu(menu.as_context_menu())
        // Not localized: it names the application, which has one name.
        .with_tooltip("dodo")
        // macOS then paints the mark itself from its alpha — dark on a light
        // menu bar, light on a dark one, inverted while the menu is open. It is
        // also why the marks are shapes rather than colours.
        .with_icon_as_template(true);

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
            return;
        }
    };

    let events = cx.spawn(async move |cx| drain(receiver, cx).await);

    cx.set_global(Tray {
        icon,
        menu,
        input_language: language,
        events,
    });
    note("menu bar item ready");
}

/// Points both of `tray-icon`'s global handler slots at one channel.
///
/// **Call this before anything can emit** — see the module doc's note about
/// `OnceCell`. It is called once, from [`init`], before the menu or the status
/// item exists.
fn install_event_handlers() -> UnboundedReceiver<Signal> {
    let (sender, receiver) = unbounded::<Signal>();

    let menu_sender: UnboundedSender<Signal> = sender;
    MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
        // Runs on the main thread, synchronously from the `NSMenuItem` action.
        // A closed receiver means dodo is shutting down, so the error is
        // deliberately dropped rather than logged from an AppKit callback.
        let _ = menu_sender.unbounded_send(Signal::Menu(event));
    }));

    receiver
}

/// The one long-lived listener. Parks when the channel is empty; ends when the
/// sender is dropped or the task is.
async fn drain(mut receiver: UnboundedReceiver<Signal>, cx: &mut gpui::AsyncApp) {
    while let Some(Signal::Menu(event)) = receiver.next().await {
        let command = cx.update(|cx| {
            cx.try_global::<Tray>()
                .and_then(|tray| tray.menu.command_for(&event.id))
        });

        let Some(command) = command else {
            continue;
        };

        cx.update(|cx| match command {
            TrayCommand::OpenDodo => open_dodo(cx),
            TrayCommand::Quit => cx.quit(),
        });
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
    }
}
