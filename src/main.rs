// Build the Windows binary as a GUI-subsystem app so a normal launch shows the
// window only, with no console window behind it. The cost is paid in
// `attach_parent_console` below: a GUI-subsystem process starts with no valid
// standard handles, which would silently send `--version` / `--build-info`
// nowhere. Only release builds are switched — a debug `cargo run` keeps its
// console so panics and logs stay visible while developing.
#![cfg_attr(
    all(target_os = "windows", not(debug_assertions)),
    windows_subsystem = "windows"
)]

mod api_explorer;
mod app;
mod app_icon;
mod assets;
mod build_info;
mod database;
mod docker;
mod encoder_decoder;
mod i18n;
/// Guards the rule that `i18n` only enforces halfway; test-only.
#[cfg(test)]
mod i18n_lint;
mod json_formatter;
mod layout;
mod paths;
mod quick_nav;
mod settings;
mod updater;
mod window_icon;

use gpui::*;
use gpui_component::*;

use crate::{app::DodoApp, assets::Assets};

// Closes the window, and with it — see `QuitMode::LastWindowClosed` below — the
// app. Bound on macOS only; see `init_close_window_binding`.
actions!(dodo, [CloseWindow]);

fn main() {
    if print_build_metadata_and_exit() {
        return;
    }

    // dodo opens exactly one window, so closing it must end the process rather
    // than leave a windowless app in the macOS Dock. `QuitMode::Default` is
    // `Explicit` on macOS (the AppKit convention) and `LastWindowClosed`
    // everywhere else; stating `LastWindowClosed` makes every platform behave
    // the same. GPUI runs the check itself, after the window has been removed
    // and its close observers have run, which is why nothing here has to quit
    // from a window callback. Dialogs are in-window layers, not OS windows, so
    // they never reach this path.
    let app = gpui_platform::application()
        .with_assets(Assets)
        .with_quit_mode(QuitMode::LastWindowClosed);

    app.run(move |cx| {
        // This must be called before using any GPUI Component features.
        gpui_component::init(cx);
        // Registers the vendored themes; needs the registry `init` just created.
        settings::init(cx);
        // Binds the API Explorer's send shortcut. Like `settings::init`, it has
        // to run after `gpui_component::init` to win the key-binding tie.
        api_explorer::init(cx);
        // Binds the Docker list pages' keyboard navigation, scoped to the Docker
        // view. Same post-`init` ordering rule as the two above.
        docker::init(cx);
        // Binds copy-cell and copy-row while the Database result grid has focus.
        database::init(cx);
        // Loads `updater.json`, sweeps whatever a previous install renamed
        // aside, and schedules the silent background check. Everything it does
        // is asynchronous; it opens no window and blocks nothing. Same
        // post-`gpui_component::init` ordering as the four above — it binds no
        // keys today, and keeping the position means adding one later is not a
        // debugging session.
        updater::init(cx);
        init_close_window_binding(cx);
        // Dock icon for a directly-run macOS binary; a no-op inside a .app and
        // on every other platform. Here rather than earlier because it needs
        // the `NSApplication` GPUI has by now created, and it must be on the
        // main thread — which this closure is.
        #[cfg(target_os = "macos")]
        window_icon::set_macos_dock_icon();

        let window_options = WindowOptions {
            window_bounds: Some(WindowBounds::centered(size(px(900.), px(620.)), cx)),
            // Stops a resize drag before the layout has to cope: the icon rail
            // plus the main pane at its minimum. `layout::window_min_size`
            // derives it, and the scroll container in `Layout::render` is what
            // covers the case where a platform ignores this.
            window_min_size: Some(layout::window_min_size()),
            // What a Linux desktop matches `assets/linux/dodo.desktop` against
            // to find the icon; inert on macOS and Windows. See
            // `window_icon::APP_ID` for why the value is not arbitrary.
            app_id: Some(window_icon::APP_ID.to_owned()),
            // `icon` exists on every platform and is read by exactly one —
            // GPUI's X11 backend, which writes it into `_NET_WM_ICON`. The
            // `cfg` on the *field* rather than a function returning `None`
            // elsewhere is what keeps `image` a Linux-only dependency: no other
            // target ever names the type. `..Default::default()` supplies the
            // `None` they get instead.
            #[cfg(target_os = "linux")]
            icon: window_icon::x11_icon(),
            ..Default::default()
        };

        cx.spawn(async move |cx| {
            cx.open_window(window_options, |window, cx| {
                let view = cx.new(|cx| DodoApp::new(window, cx));
                // This first level on the window, should be a Root.
                cx.new(|cx| Root::new(view, window, cx))
            })
            .expect("Failed to open window");
        })
        .detach();
    });
}

/// Makes Cmd-W do what the red close button does.
///
/// On macOS that shortcut is normally the key equivalent of a **File > Close**
/// menu item, and dodo installs no menu bar at all — so without this the
/// keystroke reaches nothing and the window stays open. The binding carries no
/// key context so it fires wherever focus happens to be, and it removes the
/// window exactly as GPUI's own platform close callback does
/// (`Window::remove_window`), which drops the platform window and lands on the
/// same quit path.
///
/// macOS only. Windows and Linux close a single-window app with Alt-F4, and
/// Ctrl-W is not free there — it is "delete previous word" in a text field.
///
/// **The `defer` is load-bearing, and the first version of this shipped without
/// it and did nothing.** A keystroke is dispatched from inside
/// `App::update_window_id`, which `take()`s the `Window` out of `App::windows`
/// for the duration of the update — so `AnyWindowHandle::update` called straight
/// from an action handler is re-entrant and always fails with `window not found`,
/// while the keystroke still counts as consumed (which is why the mouse cursor
/// hid and nothing else happened). `cx.defer` runs the removal from the effect
/// flush instead, once the window is back. Do not inline it again.
#[cfg(target_os = "macos")]
fn init_close_window_binding(cx: &mut App) {
    cx.bind_keys([KeyBinding::new("cmd-w", CloseWindow, None)]);
    cx.on_action(|_: &CloseWindow, cx: &mut App| {
        cx.defer(|cx| {
            // Every open window, which is one. `App::active_window` is
            // deliberately not used: it is the *key* window and is `None`
            // whenever the app is not frontmost. The only way `update` can fail
            // from here is a window that closed between the keystroke and the
            // flush, which needs no handling — it is already gone.
            for window in cx.windows() {
                let _ = window.update(cx, |_, window, _| window.remove_window());
            }
        });
    });
}

#[cfg(not(target_os = "macos"))]
fn init_close_window_binding(_cx: &mut App) {}

/// dodo's only command-line surface: `--version` / `-V` and `--build-info`
/// print the metadata `build.rs` embedded and exit, before any GPUI state or
/// window exists.
///
/// This is what lets CI prove a packaged binary actually runs — a GUI app
/// cannot be launched on a headless runner, so the release workflow executes
/// this path instead (see `docs/release.md` for exactly what that does and
/// does not prove).
///
/// Returns `true` when it handled the arguments and `main` should stop.
/// Anything else — no arguments, an unrecognised argument, the arguments macOS
/// passes to a bundled `.app` — falls through to the window, so normal launch
/// behaviour is unchanged.
fn print_build_metadata_and_exit() -> bool {
    match std::env::args().nth(1).as_deref() {
        Some("--version" | "-V") => {
            attach_parent_console();
            println!("{}", build_info::VERSION_INFO.short());
        }
        Some("--build-info") => {
            attach_parent_console();
            println!("{}", build_info::VERSION_INFO.long());
        }
        _ => return false,
    }
    true
}

/// Gives the two metadata flags somewhere to print on Windows.
///
/// The crate-level `windows_subsystem = "windows"` attribute is what keeps a
/// console window from appearing behind the app on a normal launch, and the
/// price is stated in Microsoft's own `AttachConsole` documentation: for a
/// `/SUBSYSTEM:WINDOWS` binary "the standard handles retrieved with
/// `GetStdHandle` will likely be invalid on startup until `AttachConsole` is
/// called. The exception to this is if the application is launched with handle
/// inheritance by its parent process." Without this, `dodo --build-info` typed
/// into a terminal would print into the void.
///
/// So: attach to the console that launched us, but *only* when we have no
/// stdout of our own. That exception is the case CI relies on — PowerShell
/// capturing `& dodo.exe --build-info` hands the child a pipe, which a
/// GUI-subsystem process inherits and which must not be replaced by a console.
///
/// It is called from the CLI path alone. A GUI launch never attaches, so the
/// console a developer started dodo from is not written to, and a launch from
/// Explorer or the Dock has no parent console to find.
///
/// One inherent wrinkle, not a bug: a shell does not wait for a GUI-subsystem
/// process, so the output can land after the prompt has already returned.
#[cfg(target_os = "windows")]
fn attach_parent_console() {
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
    use windows_sys::Win32::System::Console::{
        ATTACH_PARENT_PROCESS, AttachConsole, GetStdHandle, STD_OUTPUT_HANDLE,
    };

    // SAFETY: both are plain kernel32 calls that take no pointers and mutate
    // nothing this process owns. A failed `AttachConsole` (no parent console —
    // launched from Explorer) leaves the handles exactly as they were, which is
    // why the result is deliberately ignored.
    unsafe {
        let stdout = GetStdHandle(STD_OUTPUT_HANDLE);
        if stdout.is_null() || stdout == INVALID_HANDLE_VALUE {
            AttachConsole(ATTACH_PARENT_PROCESS);
        }
    }
}

/// No-op off Windows: every other platform gives a process working standard
/// handles regardless of how it was launched.
#[cfg(not(target_os = "windows"))]
fn attach_parent_console() {}
