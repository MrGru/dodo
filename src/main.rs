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

// The API Explorer is a *feature* crate — `crates/dodo-api-explorer`, the
// fourth and largest module taken out of this binary. `layout.rs` names
// `ApiExplorer`, `run_app` below calls `api_explorer::init`, `settings.rs`
// names `ScriptPolicy` and `models::script_consent::ConsentPolicy`, and
// `quick_nav` reads `models::snapshot::RequestSnapshot` and `services::curl`
// to route a pasted cURL command; this alias is what keeps all of those lines
// reading `crate::api_explorer::…`. There is no `src/api_explorer/` any more.
use dodo_api_explorer as api_explorer;
mod app;
// The icon set moved out to `crates/dodo-app-icon`; this alias is what keeps the
// 39 modules that draw one spelling it `crate::app_icon::AppIcon`. There is no
// `src/app_icon.rs` any more — the crate is the whole of it.
use dodo_app_icon as app_icon;
mod assets;
mod build_info;
// The Cleaner is a *feature* crate — `crates/dodo-cleaner`, the first module
// big enough to be worth taking out of this binary. `layout.rs` names one
// item from it, `CleanerView`, and this alias is what keeps that line reading
// `use crate::cleaner::CleanerView`. There is no `src/cleaner/` any more.
use dodo_cleaner as cleaner;
// The Database Explorer is a *feature* crate — `crates/dodo-database`, the
// third module taken out of this binary. `layout.rs` names `DatabaseView`,
// `run_app` below calls `database::init`, and `quick_nav` reads
// `models::uri` to route a pasted connection string; this alias is what keeps
// all four lines reading `crate::database::…`. There is no `src/database/` any
// more.
use dodo_database as database;
// One dialog at a time for the two dialogs with more than one way in. It used
// to be `src/dialog_slot.rs`; it is `crates/dodo-dialog-slot` now, because the
// slot is a gpui `Global` — identified by its type — and the updater, which
// left this binary, has to claim the *same* one `settings.rs` does. This alias
// is what keeps both call sites reading `crate::dialog_slot::…`.
use dodo_dialog_slot as dialog_slot;
// The Docker/Podman tool is a *feature* crate — `crates/dodo-docker`, the
// second module taken out of this binary. `layout.rs` names `DockerPage` and
// `DockerView`, `run_app` below calls `docker::init`, and this alias is what
// keeps all three lines reading `crate::docker::…`. There is no `src/docker/`
// any more.
use dodo_docker as docker;
mod encoder_decoder;
// Every string dodo shows, plus the three UI-bound pieces (`t`, the active-
// language global, `Language::current` / `set`) that the crate's `gpui`
// feature switches on. Both halves are `crates/dodo-i18n` now: a feature
// crate outside this binary has to be able to render a `Str`, and a gpui
// `Global` is identified by its type, so there can only be one of it. This
// alias is what keeps all 100-odd `use crate::i18n::{…}` lines unchanged.
use dodo_i18n as i18n;
/// Guards the rule that `i18n` only enforces halfway; test-only.
#[cfg(test)]
mod i18n_lint;
// dodo's end of the input method is a *feature* crate —
// `crates/dodo-input-method`, the sixth and last module taken out of this
// binary. `layout.rs` names `InputMethod`, `tools.rs` names
// `views::InputMethodView`, `run_app` below calls `init` and `load`, and this
// alias is what keeps all four lines reading `crate::input_method::…`. There
// is no `src/input_method/` any more.
//
// The *engine* is deliberately not there either — it is the `dodo-ime-core`
// crate, because the macOS/Windows/Linux hosts that drive it load into other
// processes and must link it without linking gpui — and neither is a host,
// which dodo does not link at all. What that crate links is `dodo-ime-ipc`,
// the contract between the two processes.
use dodo_input_method as input_method;
mod json_formatter;
mod layout;
// Where dodo writes its files. Every rule is in `crates/dodo-paths`, which is
// pure, has no dependencies and no build script; what cannot be pure is the one
// impure input — which platform *this binary* was compiled for — so it is read
// here, from the triple `build.rs` embedded, and handed to the crate. That is
// also why `HostOs::current` is `paths::current` now: an inherent method cannot
// follow its type across a crate boundary while its body stays behind.
mod paths {
    use std::path::PathBuf;

    pub use dodo_paths::{Environment, HostOs, resolve};

    use crate::build_info::VERSION_INFO;

    /// The platform this binary was compiled for.
    pub fn current() -> HostOs {
        HostOs::of_target(VERSION_INFO.target)
    }

    /// dodo's data directory on this machine, created by whichever store saves
    /// first.
    pub fn data_dir() -> PathBuf {
        resolve(current(), &Environment::from_env())
    }

    #[cfg(test)]
    mod tests {
        /// `dodo_cleaner::paths` has to answer this question too, and cannot
        /// answer it the same way: a library crate is handed no target triple,
        /// so it names the platform with `cfg!` instead. Two spellings of one
        /// fact is exactly the shape that drifts silently — a Cleaner scanning
        /// as if it were on Windows would hide half the categories — so this is
        /// the test that keeps them one answer. It is the same guard
        /// `dodo-paths` already keeps against `dodo-ime-ipc`'s own copy of the
        /// macOS rule.
        #[test]
        fn the_cleaner_crate_resolves_the_same_host_as_this_binary() {
            assert_eq!(super::current(), dodo_cleaner::paths::current());
            assert_eq!(super::data_dir(), dodo_cleaner::paths::data_dir());
        }

        /// `dodo_docker::paths` is the second copy of the same seam, for the
        /// same reason, and needs the same guard: `models::runtime` decides
        /// which container runtimes exist and how to start them purely from a
        /// [`HostOs`], so a Docker crate that resolved a different platform
        /// than the binary it is linked into would offer `systemctl` on macOS.
        /// It exposes no `data_dir` — nothing under `crates/dodo-docker`
        /// writes a file — so there is only one spelling to compare.
        #[test]
        fn the_docker_crate_resolves_the_same_host_as_this_binary() {
            assert_eq!(super::current(), dodo_docker::paths::current());
        }

        /// `dodo_database::paths` is the third copy of the same seam, and the
        /// one where a disagreement would be worst: a `data_dir()` that did
        /// not match the binary's would leave `connections.json` and
        /// `query-data.json` behind on the next launch, so every saved
        /// connection and every saved query would silently vanish.
        #[test]
        fn the_database_crate_resolves_the_same_host_as_this_binary() {
            assert_eq!(super::current(), dodo_database::paths::current());
            assert_eq!(super::data_dir(), dodo_database::paths::data_dir());
        }

        /// `dodo_input_method::paths` is the sixth and last copy of the same
        /// seam, and it guards the one file dodo writes for *another process*
        /// to read: a `data_dir()` that did not match the binary's would put
        /// `input-method.json` where no native host looks, so every engine
        /// setting and the selected keyboard language would be silently
        /// ignored and the bundle would type with `DEFAULT_CONFIG`.
        #[test]
        fn the_input_method_crate_resolves_the_same_host_as_this_binary() {
            assert_eq!(super::current(), dodo_input_method::paths::current());
            assert_eq!(super::data_dir(), dodo_input_method::paths::data_dir());
        }

        /// `dodo_updater::paths` is the fifth copy of the same seam, and the
        /// smallest: it guards `updater.json` alone. A `data_dir()` that did
        /// not match the binary's would lose a skipped version and a channel
        /// choice on every launch, so the updater would keep re-offering an
        /// update the user already declined.
        #[test]
        fn the_updater_crate_resolves_the_same_host_as_this_binary() {
            assert_eq!(super::current(), dodo_updater::paths::current());
            assert_eq!(super::data_dir(), dodo_updater::paths::data_dir());
        }

        /// `dodo_api_explorer::paths` is the fourth and last copy of the same
        /// seam, and it guards three files rather than two: a `data_dir()`
        /// that did not match the binary's would leave `collections.json`,
        /// `environments.json` and `script-consent.json` behind on the next
        /// launch, so every saved request, every environment variable and
        /// every approved script would silently vanish.
        #[test]
        fn the_api_explorer_crate_resolves_the_same_host_as_this_binary() {
            assert_eq!(super::current(), dodo_api_explorer::paths::current());
            assert_eq!(super::data_dir(), dodo_api_explorer::paths::data_dir());
        }
    }
}
mod quick_nav;
mod session;
mod settings;
// **The tool table.** One row per sidebar tool — its code, title, icon, the
// platforms it exists on, its view entity and the pastes it accepts — from
// which `View`, `View::ALL` and the `Panes` holding every view are generated.
// Adding a tool is a row here plus its own crate or module; `layout.rs` is the
// shell around whatever the table declares. Unrelated to the repo-root `tools/`
// directory, which holds the standalone `update-manifest` crate.
mod tools;
#[cfg(any(target_os = "macos", target_os = "windows"))]
mod tray;
// The in-app updater is a *feature* crate — `crates/dodo-updater`, the fifth
// module taken out of this binary. `layout.rs` names `updater::open` and
// `run_app` below calls `updater::init`; this alias is what keeps both lines
// reading `crate::updater::…`. There is no `src/updater/` any more.
//
// `init` takes a `BuildInfo` because a library crate is handed none of the
// `env!("DODO_*")` variables `build.rs` sets — see that crate's `build_info`
// module, and `paths` below for the same trade made for `dodo-paths`.
use dodo_updater as updater;
/// The other half of the `build_info` seam: what `crates/dodo-updater` cannot
/// find out for itself, asserted here where `VERSION_INFO` is reachable.
#[cfg(test)]
mod updater_build_info {
    use dodo_updater::models::platform::PlatformKey;
    use dodo_updater::models::version::Version;

    use crate::build_info::VERSION_INFO;

    /// `dodo_updater::build_info` falls back to naming the platform with
    /// `cfg!` whenever `init` has not run — which is every `cargo test` — and
    /// the two spellings must classify alike, or the crate's own pipeline
    /// tests would exercise a manifest entry no real build ever asks for.
    /// This is the same guard `paths` keeps for the four other feature crates;
    /// the *strings* may differ (`…-windows-gnu` against `…-windows-msvc`),
    /// because classifying is the only thing anything does with them.
    #[test]
    fn the_updater_crate_classifies_this_binarys_target_the_same_way() {
        assert_eq!(
            PlatformKey::from_target(VERSION_INFO.target),
            PlatformKey::current()
        );
    }

    /// This binary must be able to find itself in a manifest, or the updater is
    /// dead code on the platform it was compiled for. It moved here from
    /// `models::platform` with the crate: only a binary is handed the triple.
    #[test]
    fn this_build_knows_which_platform_it_is() {
        assert!(
            PlatformKey::from_target(VERSION_INFO.target).is_some(),
            "no manifest key for the target this binary was built for: {}",
            VERSION_INFO.target
        );
    }

    /// And it must be able to compare itself against a manifest's version.
    /// Moved here from `services::pipeline` for the same reason: the version
    /// the updater actually reports is the one `build.rs` embedded *here*.
    #[test]
    fn this_build_reports_a_version_the_updater_can_compare() {
        let version =
            Version::parse(VERSION_INFO.version).expect("build.rs embeds a semantic version");
        assert_eq!(version.to_display(), VERSION_INFO.version);
    }
}
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

    // Closing the window ends the process, so dodo never sits windowless in the
    // macOS Dock with no way back. `QuitMode::Default` is `Explicit` on macOS
    // (the AppKit convention) and `LastWindowClosed` everywhere else; stating
    // `LastWindowClosed` makes every platform behave the same. GPUI runs the
    // check itself, after the window has been removed and its close observers
    // have run, which is why nothing here has to quit from a window callback.
    // Dialogs are in-window layers, not OS windows, so they never reach this
    // path.
    //
    // **The tray overrides this at runtime, and deliberately not here.** Once
    // its icon exists there is a way back, so `tray::init` switches to
    // `QuitMode::Explicit` and closing the window leaves dodo running behind
    // it. Setting `Explicit` statically instead would be a trap: a tray that
    // failed to come up would leave a process with no window and no icon —
    // unquittable except from Activity Monitor. Deriving the mode from the icon
    // actually existing makes that state unreachable.
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
        // Binds quick navigation's paste chords and Escape, and starts the
        // `quick-nav.json` load. Same post-`gpui_component::init` ordering as
        // the four above, and it matters more here than for any of them: the
        // Escape binding is deliberately *shallower* than the library's own, so
        // every existing Escape still wins.
        quick_nav::init(cx);
        // Loads `updater.json`, sweeps whatever a previous install renamed
        // aside, and schedules the silent background check. Everything it does
        // is asynchronous; it opens no window and blocks nothing. Same
        // post-`gpui_component::init` ordering as the four above — it binds no
        // keys today, and keeping the position means adding one later is not a
        // debugging session.
        updater::init(
            // The two `VERSION_INFO` fields the updater reads, handed over
            // rather than re-derived: `build_info` stays in this binary
            // because only a binary is given the variables `build.rs` sets.
            updater::BuildInfo {
                version: build_info::VERSION_INFO.version,
                target: build_info::VERSION_INFO.target,
            },
            cx,
        );
        // Installs the session global and the quit-time flush of
        // `session.json`. It reads nothing here — the read is awaited below,
        // because the window cannot be opened until its geometry is known.
        session::init(cx);
        // Installs the input-method global. It reads `input-method.json` below.
        // Native hosts are launched independently by macOS/Windows; dodo-owned
        // Event Tap and Keyboard Hook start only after their stored selection
        // has been reconciled. Same post-`gpui_component::init` position as the
        // rest.
        // The tray is the one thing that wants to hear about a language
        // change, and it lives here rather than in the crate — `src/tray`
        // already reads the input method, so the notification is what had to
        // invert when the module became a crate. Handed over before `init` so
        // the two lines cannot drift apart; a platform with no tray registers
        // nothing and is called back never.
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        input_method::observe_languages(tray::set_active_languages);
        input_method::init(cx);
        init_close_window_binding(cx);
        // Read this while the OS launch arguments are still the only startup
        // signal. It is captured into the foreground task below, which avoids
        // any dependency on the selected input-method backend.
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        let startup_launch = tray::startup::launched_at_login();
        // Dock icon for a directly-run macOS binary; a no-op inside a .app and
        // on every other platform. Here rather than earlier because it needs
        // the `NSApplication` GPUI has by now created, and it must be on the
        // main thread — which this closure is. A login launch becomes an
        // accessory app before the asynchronous settings load, so it has no
        // transient Dock entry.
        #[cfg(target_os = "macos")]
        {
            window_icon::set_macos_dock_icon();
            if startup_launch {
                window_icon::set_macos_dock_visible(false);
            }
        }

        cx.spawn(async move |cx| {
            // `session.json` decides the theme, the font size and the window
            // rectangle, so it is read *before* the window exists rather than
            // applied over a frame the user has already seen. It is one small
            // local file, read on the background executor; a missing or
            // unreadable one leaves every default exactly as it was before
            // session restoration existed.
            session::load(cx).await;
            // The input method's settings and whatever the bundle last said about
            // itself. Awaited here rather than spawned and forgotten so that the
            // Settings dialog cannot be opened before the answer arrives — but
            // unlike `session.json` nothing on screen depends on it, so a slow
            // read costs the window nothing.
            input_method::load(cx).await;

            cx.update(|cx| {
                // Theme, font size, border radius and language, in that
                // order — see `settings::apply_session`.
                settings::apply_session(cx);
                // The tray, **after** the session is known and before the
                // window exists. It opens no window, and its remembered input
                // language is already available for the first glyph. A failed
                // tray falls back to the normal window so dodo is never left
                // running without a way back.
                #[cfg(any(target_os = "macos", target_os = "windows"))]
                if tray::init(cx) && startup_launch {
                    return;
                }

                open_main_window(cx).expect("Failed to open window");
            });
        })
        .detach();
    });
}

/// Opens dodo's one window, restored the way the session left it.
///
/// **Extracted so the tray's "Open Dodo" can call it too.** A user who closed
/// the window keeps the process — see `tray::init` — and reopening has to
/// go through exactly this, or the window comes back ignoring its saved
/// rectangle, its saved display and the layout's minimum size. A second copy of
/// these options that drifts from this one is the bug this shape prevents.
///
/// Fallible rather than panicking, because the tray's caller must not take the
/// process down; `main` still treats a failure at launch as fatal.
fn open_main_window(cx: &mut App) -> Result<WindowHandle<Root>> {
    // A close-to-tray macOS app uses the accessory activation policy to leave
    // the Dock. Restore the regular policy before opening a real window.
    #[cfg(target_os = "macos")]
    window_icon::set_macos_dock_visible(true);

    let options = window_options(cx);
    cx.open_window(options, |window, cx| {
        let view = cx.new(|cx| DodoApp::new(window, cx));
        // This first level on the window, should be a Root.
        cx.new(|cx| Root::new(view, window, cx))
    })
}

/// Where and how the window opens. Read from the session every time, so a
/// reopen honours a rectangle the user has moved since launch.
fn window_options(cx: &mut App) -> WindowOptions {
    WindowOptions {
        // The saved rectangle has already been placed against the displays
        // that exist now, so an unplugged monitor or a changed resolution
        // arrives here as `None` or as a corrected rectangle, never as
        // somewhere unreachable. `session::models::geometry` is where that
        // judgement lives.
        window_bounds: Some(
            session::Session::window_bounds(cx)
                .unwrap_or_else(|| session::default_window_bounds(cx)),
        ),
        // The monitor the window was on, when it is still attached. Paired with
        // the rectangle above rather than optional decoration: on macOS every
        // coordinate gpui reports is display-*local*, so the rectangle alone
        // cannot say which screen it meant. `Session::window_display` has the
        // detail.
        display_id: session::Session::window_display(cx),
        // Stops a resize drag before the layout has to cope: the icon rail plus
        // the main pane at its minimum. `layout::window_min_size` derives it,
        // and the scroll container in `Layout::render` is what covers the case
        // where a platform ignores this. A *restored* window is held to the same
        // floor by `geometry::place`, which this option cannot do for it — the
        // platform only polices dragging.
        window_min_size: Some(layout::window_min_size()),
        // What a Linux desktop matches `assets/linux/dodo.desktop` against to
        // find the icon; inert on macOS and Windows. See `window_icon::APP_ID`
        // for why the value is not arbitrary.
        app_id: Some(window_icon::APP_ID.to_owned()),
        // `icon` exists on every platform and is read by exactly one — GPUI's
        // X11 backend, which writes it into `_NET_WM_ICON`. The `cfg` on the
        // *field* rather than a function returning `None` elsewhere is what
        // keeps `image` a Linux-only dependency: no other target ever names the
        // type. `..Default::default()` supplies the `None` they get instead.
        #[cfg(target_os = "linux")]
        icon: window_icon::x11_icon(),
        ..Default::default()
    }
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
/// **Since the menu bar item shipped, that path no longer ends the process.**
/// `tray::init` moves the quit mode to `QuitMode::Explicit` once the status item
/// exists, so Cmd-W now *hides* dodo: the window goes, the process and the menu
/// bar item stay, and **Quit Dodo** in that menu is the way out. The binding
/// itself is unchanged and still correct — it is the same close the red button
/// performs — but do not read it as a quit shortcut any more. When the tray
/// fails to come up the quit mode is never switched, and this closes dodo
/// exactly as it always did.
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
