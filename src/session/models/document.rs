//! `session.json` — the **eighth** thing dodo persists, and the one that
//! overturns `settings.rs`'s "nothing is persisted across restarts".
//!
//! The captain asked for it on 2026-08-06: *"setting is not keep when reopen
//! app, save the setting and remember open window size, current tab, to make
//! sure it will be there when reopen app."* One file rather than three, because
//! the three things it holds are written at the same moments and are all read
//! once, before the window exists — see [`super::super`]'s module doc for the
//! argument.
//!
//! It follows the file discipline `AGENTS.md` names as the one to copy —
//! `quick-nav.json`'s and `updater.json`'s, pointedly **not**
//! `collections.json`'s:
//!
//! - an explicit `"version"` written from the very first save;
//! - a parser that **refuses** a higher version rather than half-reading it
//!   (see [`crate::session::services::session_store::parse_document`]);
//! - a missing file meaning *first run*, not an error;
//! - a temp-file-then-rename write.
//!
//! # Why every field is an `Option`
//!
//! `None` means *the user never touched this*, and that is not the same as
//! "the user chose the default". The clearest case is the theme:
//! `gpui_component::init` picks light or dark from the **system appearance**,
//! so writing `"Default Light"` into a fresh file — merely because that is what
//! the app happened to be showing — would freeze every future launch at light
//! and quietly break system-appearance following. An absent key leaves the
//! library's own choice alone.
//!
//! The same reasoning gives [`SessionDocument::window`] its meaning: absent is
//! "dodo has never been resized", which is what makes the default centred
//! 900×620 window still reachable rather than something only a deleted file can
//! get back to.
//!
//! # What is deliberately **not** here
//!
//! **The Run scripts policy.** `ScriptPolicy` resets to the cautious
//! `Ask for imported` on every launch, and that is a security default rather
//! than a preference: a user who allowed a script once, for one imported
//! collection, has not decided that every future launch may run every imported
//! script. `settings.rs`'s `run_scripts_field` states the same thing at the
//! control. The approvals the prompt collects *are* persisted, separately and
//! per script, in `script-consent.json`. If this ever changes it is the
//! captain's call, not an implementation detail.
//!
//! Also absent, and for a much smaller reason: the Docker rail's page, and the
//! Database and API Explorer tabs. The captain asked for the current *tool*;
//! restoring a tool's own inner selection is a separate round.

use serde::{Deserialize, Serialize};

/// The schema version written into every `session.json`.
pub const SCHEMA_VERSION: u32 = 1;

/// Everything dodo restores when it reopens.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SessionDocument {
    /// Written first and read first. See the module doc.
    #[serde(default = "schema_version")]
    pub version: u32,
    /// The Settings dialog's appearance choices, minus the script policy.
    #[serde(default)]
    pub appearance: Appearance,
    /// Where the window was and how big, or `None` before it has ever moved.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window: Option<WindowRecord>,
    /// Which tool was open, and how the sidebar was showing it.
    #[serde(default)]
    pub workspace: Workspace,
}

fn schema_version() -> u32 {
    SCHEMA_VERSION
}

impl SessionDocument {
    /// A first-run document: the version, and nothing chosen yet.
    pub fn new() -> Self {
        Self {
            version: SCHEMA_VERSION,
            ..Self::default()
        }
    }
}

/// The appearance settings, each `None` until the user picks one.
///
/// Deliberately not a struct of concrete values with defaults: see the module
/// doc on why an untouched theme must stay untouched.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Appearance {
    /// A [`Language::code`](crate::i18n::Language::code) — a stable identifier,
    /// never a localized label.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    /// A `ThemeRegistry` key, which is the `name` inside `assets/themes/*.json`.
    /// A theme this build does not have registered is ignored on load rather
    /// than being an error — see `settings::set_theme`, which
    /// `settings::apply_session` calls.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<String>,
    /// Base text size in px. Drives the window's rem size, so it scales the
    /// whole UI.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub font_size: Option<f32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub border_radius: Option<f32>,
}

/// How the window was showing when it was last seen.
///
/// The three modes are exactly `gpui::WindowBounds`' three, and the rectangle is
/// the same **restore** rectangle that type carries: for `Maximized` and
/// `Fullscreen` it is the size the window returns to, not the size it is
/// covering the screen with. Storing it in every mode is what lets an unzoomed
/// window come back somewhere sensible.
///
/// **macOS never reports `Maximized`.** Its `window_bounds()` (verified in the
/// pinned checkout, `gpui_macos/src/window.rs`) answers `Fullscreen` or
/// `Windowed` and nothing else, so a green-button-zoomed window is saved as
/// `Windowed` at the zoomed rectangle — which restores to the same place
/// anyway. Windows and Linux do report it, and it is honoured there.
///
/// # Why the display is remembered by UUID
///
/// **The rectangle alone cannot say which monitor it was on**, and on macOS it
/// provably cannot. In the pinned checkout `MacDisplay::bounds` returns an
/// origin of `(0, 0)` for *every* display — it throws `CGDisplayBounds`' global
/// origin away — `MacDisplay::visible_bounds` subtracts the screen origin back
/// out, `MacWindow::bounds` subtracts it too, and `MacWindow::open` adds the
/// target screen's origin back on. Every macOS coordinate here is therefore
/// **display-local**, so "x = 120" means the same thing on the laptop panel and
/// on the monitor beside it, and no arithmetic over rectangles can tell them
/// apart. Windows and Linux do report real global bounds.
///
/// [`PlatformDisplay::uuid`](gpui::PlatformDisplay::uuid) is documented as
/// "stable … across system restarts", which is exactly the identity needed, and
/// it is what `WindowOptions::display_id` is resolved from at launch. A UUID
/// that is not attached any more is the unplugged monitor: the window comes
/// back on the primary display instead, and [`super::geometry::place`] makes
/// sure it is somewhere reachable.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WindowRecord {
    #[serde(default)]
    pub mode: WindowMode,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    /// The display's stable UUID, or `None` when the platform would not give
    /// one — which is not an error, only a window that comes back on the
    /// primary display.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display: Option<String>,
}

/// Windowed, zoomed, or filling the display.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WindowMode {
    #[default]
    Windowed,
    Maximized,
    Fullscreen,
}

/// The main pane and the sidebar around it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Workspace {
    /// A tool's stable code — `layout::View::code`. A code this build does not
    /// know falls back to the default tool rather than failing to start; that
    /// is `View::from_code`'s job and is tested there.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_tool: Option<String>,
    /// Whether the sidebar was showing the icon rail. Left unpersisted by the
    /// sidebar round because it was a decision above that worker; the captain's
    /// request for session restoration is what settles it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sidebar_collapsed: Option<bool>,
}

#[cfg(test)]
mod tests {
    use super::{SCHEMA_VERSION, SessionDocument, WindowMode, WindowRecord};

    #[test]
    fn a_first_run_document_carries_the_version_and_no_choices() {
        let document = SessionDocument::new();
        assert_eq!(document.version, SCHEMA_VERSION);
        assert_eq!(document.appearance, Default::default());
        assert_eq!(document.window, None);
        assert_eq!(document.workspace, Default::default());
    }

    /// The point of the `Option`s: a document that has chosen nothing writes
    /// nothing but its version, so a fresh install does not freeze the
    /// system-appearance theme into a file.
    #[test]
    fn an_untouched_document_serializes_to_the_version_alone() {
        let json = serde_json::to_string(&SessionDocument::new()).expect("serializes");
        assert_eq!(json, r#"{"version":1,"appearance":{},"workspace":{}}"#);
    }

    #[test]
    fn a_document_round_trips_every_field() {
        let mut document = SessionDocument::new();
        document.appearance.language = Some("vi".to_owned());
        document.appearance.theme = Some("Ayu Dark".to_owned());
        document.appearance.font_size = Some(18.);
        document.appearance.border_radius = Some(0.);
        document.window = Some(WindowRecord {
            mode: WindowMode::Maximized,
            x: 12.,
            y: 34.,
            width: 1000.,
            height: 700.,
            display: Some("6E1E9C3F-0000-0000-0000-000000000001".to_owned()),
        });
        document.workspace.active_tool = Some("docker".to_owned());
        document.workspace.sidebar_collapsed = Some(false);

        let json = serde_json::to_string(&document).expect("serializes");
        let read: SessionDocument = serde_json::from_str(&json).expect("parses");
        assert_eq!(read, document);
    }

    /// A file written by a dodo that has never been resized still parses, and
    /// the absent window is the "never moved" state rather than a zero rect.
    #[test]
    fn a_file_with_no_window_section_means_never_resized() {
        let document: SessionDocument = serde_json::from_str(r#"{"version":1}"#).expect("parses");
        assert_eq!(document.window, None);
        assert_eq!(document.appearance.theme, None);
        assert_eq!(document.workspace.active_tool, None);
    }

    #[test]
    fn the_window_mode_is_written_as_a_stable_lowercase_code() {
        for (mode, code) in [
            (WindowMode::Windowed, "\"windowed\""),
            (WindowMode::Maximized, "\"maximized\""),
            (WindowMode::Fullscreen, "\"fullscreen\""),
        ] {
            assert_eq!(serde_json::to_string(&mode).expect("serializes"), code);
        }
    }

    /// A window record from a build that predates the mode field reads as a
    /// plain window, which is what it was.
    #[test]
    fn a_window_record_without_a_mode_is_windowed() {
        let record: WindowRecord =
            serde_json::from_str(r#"{"x":0,"y":0,"width":900,"height":620}"#).expect("parses");
        assert_eq!(record.mode, WindowMode::Windowed);
    }
}
