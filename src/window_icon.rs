//! What dodo looks like to the *operating system's* chrome — the macOS Dock,
//! the Linux task bar and Alt-Tab. Not to be confused with `app_icon.rs`, which
//! is the SVG set dodo draws inside its own window.
//!
//! Three platforms answer this question in three unrelated ways, and only one
//! of them is answered by a file sitting next to the binary:
//!
//! | Platform | What the OS reads | Where it comes from |
//! |---|---|---|
//! | Windows | an `RT_GROUP_ICON` resource inside `dodo.exe` | `build.rs`, at compile time |
//! | macOS, bundled | `CFBundleIconFile` -> `Contents/Resources/dodo.icns` | `scripts/macos-app-bundle.sh` |
//! | macOS, bare binary | `-[NSApplication setApplicationIconImage:]` | this module |
//! | Linux | a `.desktop` file matched to the window's application id | `assets/linux/dodo.desktop` + [`APP_ID`] |
//! | Linux, X11 only | the `_NET_WM_ICON` window property | this module, via `WindowOptions::icon` |
//!
//! The two rows this module owns are the two where **no file is available**: a
//! bare executable has no `Info.plist` and no `Resources/`, and a binary run
//! straight out of a tarball has no installed `.desktop` entry. Both therefore
//! need the artwork inside the binary, which is what [`RUNTIME_ICON_PNG`] is
//! and the only reason any icon bytes are embedded at all.
//!
//! "Application icon" in `docs/release.md` is the authority on the pipeline
//! that produces all of it, and on what has and has not actually been seen to
//! work.

/// The 256×256 RGBA master-derived PNG, embedded in the binary.
///
/// **This is the one icon artifact whose bytes are inside the executable.**
/// Everything else under `assets/branding`, `assets/macos`, `assets/windows`
/// and `assets/linux` is packaged beside the binary and costs it nothing;
/// `src/assets.rs`'s `#[include]` filters (`icons/**/*.svg`, `themes/**/*.json`)
/// deliberately exclude all of it. This one is reached by `include_bytes!`
/// instead — a different mechanism with a real bill, roughly the size of the
/// committed file, and only on the two platforms whose `cfg` below admits it.
/// A Windows build never contains it, because `build.rs` has already put a
/// `.ico` in the executable's resource table where Windows expects to find one.
///
/// 256 is the ceiling for both consumers (the macOS Dock draws at most 128pt,
/// which is 256px on a Retina display; an X11 `_NET_WM_ICON` is a task-bar
/// thumbnail), so a larger source would be bytes nothing ever draws. See
/// `RUNTIME_ICON_SIZE` in `scripts/generate-icons.py`.
#[cfg(any(target_os = "macos", target_os = "linux"))]
const RUNTIME_ICON_PNG: &[u8] = include_bytes!("../assets/branding/dodo-256.png");

/// The application id every dodo window reports.
///
/// It has an effect on Linux alone — GPUI's macOS window implements
/// `set_app_id` as an empty function and its Windows one does not implement it
/// at all — but it is set unconditionally rather than behind a `cfg`, because
/// "the id is `dodo` everywhere" is easier to keep true than a platform split
/// that only one platform ever exercises.
///
/// **The value is not free-form: it is the basename of the installed desktop
/// entry.** `assets/linux/dodo.desktop` is installed as
/// `share/applications/dodo.desktop`, so its desktop-entry ID is `dodo`, and a
/// Wayland compositor matches an `xdg_toplevel`'s `app_id` against exactly
/// that to find the `Icon=` line. On X11 the same string is written into
/// `WM_CLASS` (as `instance\0class`), which is what the entry's
/// `StartupWMClass=dodo` line matches. Renaming either without the other is
/// precisely the failure the desktop file's own comment warns about: a correct
/// icon in the launcher and a generic one in the task bar.
pub const APP_ID: &str = "dodo";

/// The X11 `_NET_WM_ICON` payload, or `None` when there is nothing useful to
/// say.
///
/// `WindowOptions::icon` is documented "X11 only" and the pinned GPUI means it:
/// the field is read in `gpui_linux`'s X11 backend and nowhere else, and the
/// Wayland backend has no equivalent (the `xdg-toplevel-icon-v1` protocol that
/// would provide one is not implemented there). So this covers an X11 session
/// running a bare binary; a Wayland session must be reached through the
/// `.desktop` entry and [`APP_ID`], which is the path that works for both.
///
/// Decoding costs one PNG decode of a 256×256 image at startup, on the thread
/// opening the window.
#[cfg(target_os = "linux")]
pub fn x11_icon() -> Option<std::sync::Arc<image::RgbaImage>> {
    // A corrupt embedded PNG is a build-time mistake, not a runtime condition,
    // and an app that refuses to open a window over its own icon would be a
    // worse bug than a generic icon. So: degrade.
    let decoded = image::load_from_memory_with_format(RUNTIME_ICON_PNG, image::ImageFormat::Png)
        .inspect_err(|error| eprintln!("dodo: could not decode the embedded window icon: {error}"))
        .ok()?;
    Some(std::sync::Arc::new(decoded.into_rgba8()))
}

/// Gives a directly-run macOS binary dodo's artwork in the Dock.
///
/// A bare Mach-O executable has no `Info.plist` and no `Contents/Resources`, so
/// there is no `CFBundleIconFile` for the Dock to follow and it draws the
/// generic executable tile. `-[NSApplication setApplicationIconImage:]` is the
/// documented way for a running process to answer that question itself, and it
/// is the only route: GPUI exposes no dock-icon API (`WindowOptions::icon` is
/// X11), so this is a platform call or nothing.
///
/// **A bundled `.app` is deliberately left alone.** Its `dodo.icns` carries
/// hand-built 16 and 32 pt variants that a downscaled 256px PNG cannot match,
/// so overriding it would be a small regression in the one macOS case that has
/// always worked. The check is a path shape rather than an AppKit call so that
/// it can be tested on any host — see [`runs_from_macos_bundle`].
///
/// Must run on the main thread, after GPUI has created the `NSApplication`;
/// `App::run`'s callback is both. Every failure is silent and leaves the Dock
/// exactly as it was.
#[cfg(target_os = "macos")]
pub fn set_macos_dock_icon() {
    // `AnyThread` is what puts `NSImage::alloc` in scope; allocation, unlike
    // the setter below, carries no thread requirement.
    use objc2::{AnyThread, MainThreadMarker};
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::NSData;

    // `current_exe` failing is not a reason to skip: the worst case of setting
    // the icon inside a bundle is the same artwork at slightly softer small
    // sizes, while the worst case of skipping outside one is the generic tile
    // this function exists to remove.
    if std::env::current_exe().is_ok_and(|exe| runs_from_macos_bundle(&exe)) {
        return;
    }

    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let Some(image) =
        NSImage::initWithData(NSImage::alloc(), &NSData::with_bytes(RUNTIME_ICON_PNG))
    else {
        return;
    };
    // SAFETY: an AppKit setter called on the main thread (`mtm` is the proof)
    // with a fully initialised `NSImage`. It is `unsafe` in `objc2` only
    // because AppKit does not annotate the argument's nullability, not because
    // there is a precondition to uphold beyond the thread. The image is
    // retained by AppKit; the `Retained` handle dropping here is correct.
    unsafe {
        NSApplication::sharedApplication(mtm).setApplicationIconImage(Some(&image));
    }
}

/// Whether `exe` is the executable *inside* a macOS application bundle, i.e.
/// whether it sits at `<name>.app/Contents/MacOS/<exe>`.
///
/// Deliberately a pure function over a path rather than
/// `NSBundle.mainBundle.bundleIdentifier`, for the same reason `paths.rs`
/// classifies platforms from a target triple instead of `#[cfg]`: it makes the
/// decision testable from any host and without a running AppKit. The shape it
/// matches is the one `scripts/macos-app-bundle.sh` builds and the one macOS
/// requires — `Contents/MacOS` is not a convention, it is where
/// `CFBundleExecutable` is resolved from.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn runs_from_macos_bundle(exe: &std::path::Path) -> bool {
    let mut ancestors = exe.ancestors().skip(1);
    let named = |dir: Option<&std::path::Path>, name: &str| {
        dir.and_then(std::path::Path::file_name) == Some(std::ffi::OsStr::new(name))
    };

    named(ancestors.next(), "MacOS")
        && named(ancestors.next(), "Contents")
        && ancestors
            .next()
            .and_then(std::path::Path::extension)
            .is_some_and(|ext| ext.eq_ignore_ascii_case("app"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn bundle_layout_is_recognised() {
        // Exactly what scripts/macos-app-bundle.sh writes, and where the
        // release archive lands once dragged to /Applications.
        assert!(runs_from_macos_bundle(Path::new(
            "/Applications/dodo.app/Contents/MacOS/dodo"
        )));
        assert!(runs_from_macos_bundle(Path::new(
            "/Users/someone/dodo/dist/dodo.app/Contents/MacOS/dodo"
        )));
        // The bundle name is not dodo's to assume: an installer or a user may
        // rename it, and macOS follows CFBundleExecutable, not the folder.
        assert!(runs_from_macos_bundle(Path::new(
            "/Applications/Dodo 2.app/Contents/MacOS/dodo"
        )));
        // macOS itself treats the .app extension case-insensitively.
        assert!(runs_from_macos_bundle(Path::new(
            "/Applications/dodo.APP/Contents/MacOS/dodo"
        )));
    }

    #[test]
    fn bare_binaries_are_not_bundles() {
        // The two the captain runs daily.
        assert!(!runs_from_macos_bundle(Path::new(
            "/Users/someone/dodo/target/debug/dodo"
        )));
        assert!(!runs_from_macos_bundle(Path::new(
            "/Users/someone/dodo/target/release/dodo"
        )));
        assert!(!runs_from_macos_bundle(Path::new("/usr/local/bin/dodo")));
        assert!(!runs_from_macos_bundle(Path::new("dodo")));
    }

    #[test]
    fn near_misses_are_rejected() {
        // Every level has to match: a lookalike directory tree is not a bundle,
        // and neither is the bundle directory itself.
        assert!(!runs_from_macos_bundle(Path::new(
            "/Applications/dodo/Contents/MacOS/dodo"
        )));
        assert!(!runs_from_macos_bundle(Path::new(
            "/Applications/dodo.app/Resources/MacOS/dodo"
        )));
        assert!(!runs_from_macos_bundle(Path::new(
            "/Applications/dodo.app/Contents/Helpers/dodo"
        )));
        assert!(!runs_from_macos_bundle(Path::new("/Contents/MacOS/dodo")));
    }

    /// The desktop entry is hand-written and the app id is a Rust constant, so
    /// nothing but this test stops the two drifting apart — which is exactly
    /// the failure that produced a correct launcher icon and a generic task-bar
    /// one before [`APP_ID`] was set at all.
    #[test]
    fn app_id_matches_the_desktop_entry() {
        let entry = include_str!("../assets/linux/dodo.desktop");
        let value = |key: &str| {
            entry
                .lines()
                .find_map(|line| line.strip_prefix(key))
                .unwrap_or_else(|| panic!("dodo.desktop has no {key} line"))
        };

        assert_eq!(
            value("StartupWMClass="),
            APP_ID,
            "StartupWMClass must equal the WM_CLASS GPUI writes from APP_ID"
        );
        // The desktop-entry ID a Wayland compositor matches app_id against is
        // the filename without `.desktop`.
        assert_eq!(
            "dodo.desktop",
            format!("{APP_ID}.desktop"),
            "the desktop entry's filename must equal APP_ID"
        );
        // The icon name resolves against share/icons/hicolor/*/apps/<name>.png,
        // which the packaging step fills from assets/linux/hicolor.
        assert_eq!(value("Icon="), "dodo");
    }
}
