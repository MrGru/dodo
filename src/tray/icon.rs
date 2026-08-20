//! The menu bar mark: one embedded SVG, rasterised into the RGBA buffer
//! `tray_icon` wants.
//!
//! **No new PNG assets, and no change to `src/assets.rs`.** The `#[include]`
//! filter there is `icons/**/*.svg`, and `**` already reaches
//! `assets/icons/tray/`, so these files are embedded by the mechanism dodo
//! already had. `scripts/generate-icons.py` could not have produced PNGs for
//! them anyway: it is stdlib-only — its own PNG codec and box filter, no PIL,
//! no ImageMagick — so it has no way to draw a glyph.
//!
//! # macOS reads the alpha; Windows reads the colour
//!
//! On macOS the status item is a **template** image (`setTemplate:`): AppKit
//! reads its alpha and paints the result itself — dark on a light menu bar,
//! light on a dark one, inverted while the menu is open. The colour channels
//! carry nothing, so they are zeroed rather than un-premultiplied out of gpui's
//! buffer.
//!
//! Windows has no template image. `tray_icon` hands the RGBA straight to
//! `CreateIcon`, which uses the colour bits, so the same all-zero buffer drew a
//! **solid black** dodo — invisible on the dark taskbar that is Windows'
//! default. The mark therefore gets an explicit ink there, and which ink is a
//! decision, not a detail:
//!
//! - **The taskbar's own light/dark setting picks it**, read from
//!   `SystemUsesLightTheme` under `Themes\Personalize`. That is the value
//!   Windows itself uses for the taskbar; `AppsUseLightTheme` beside it is for
//!   application windows and is a different question.
//! - **An outline or halo treatment was the alternative** and was not taken:
//!   these marks are silhouettes with a glyph, the notification area draws them
//!   at 16 px, and an outline computed at that size turns a legible dodo into a
//!   smudge. Picking the ink keeps the artwork as drawn.
//! - **A missing or unreadable value means dark**, because that is what a fresh
//!   Windows install has and because white-on-dark is the safer wrong answer:
//!   the accent-colour taskbars this cannot detect are far more often dark than
//!   light.
//!
//! The setting is read each time a mark is rasterised — at startup and whenever
//! the input language changes. A user who flips the system theme mid-session
//! keeps the previous ink until one of those happens; observing
//! `WM_SETTINGCHANGE` would need a window procedure dodo does not own here.
//!
//! [`paint`] is the whole decision and takes the host as a parameter rather
//! than reading a `#[cfg]`, so both platforms' answers are asserted from
//! whichever machine runs the tests — the trick `src/paths.rs` uses.

use anyhow::{Context as _, anyhow};
use dodo_ime_core::LanguageId;
use gpui::App;
use tray_icon::Icon;

use crate::assets::Assets;

/// The height, in pixels, every tray mark is rasterised at.
///
/// `tray_icon` fixes the `NSImage` at 18 **points** tall and derives its width
/// from the source's aspect ratio, so 36 px is exactly 2×: crisp on every
/// current Mac, all of which are 1× or 2×. Asking the screen for its backing
/// scale and rendering 18 or 36 conditionally would buy nothing — a 2× image on
/// a 1× display is downsampled, which is the good direction.
const ICON_HEIGHT_PX: f32 = 36.;

/// The intrinsic height of every SVG under `assets/icons/tray`, in user units.
///
/// Fixed by convention so the marks share a baseline; the **width** is free,
/// which is what lets one language's glyph be wider than another's without any
/// per-language code. See the comment at the top of any of those files.
const SVG_HEIGHT_UNITS: f32 = 24.;

/// Which host is drawing the mark.
///
/// A parameter rather than a `#[cfg]` so that both answers are testable from
/// one machine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayHost {
    /// The mark is an AppKit template image.
    MacOs,
    /// The mark is an ordinary bitmap and nothing will recolour it.
    Other,
}

impl TrayHost {
    pub fn current() -> TrayHost {
        if cfg!(target_os = "macos") {
            TrayHost::MacOs
        } else {
            TrayHost::Other
        }
    }
}

/// What the taskbar is painted with, when the system will say.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(
    not(target_os = "windows"),
    allow(
        dead_code,
        reason = "Only Windows reads the setting; both answers are asserted on every host."
    )
)]
pub enum TaskbarTheme {
    Light,
    Dark,
}

/// How a mark's opaque pixels are coloured.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarkPaint {
    /// AppKit paints from the alpha, so the colour channels carry nothing.
    Template,
    /// An explicit ink, because nothing on this host will invert the bitmap.
    Solid([u8; 3]),
}

/// Near-black rather than black, which is the ink Windows draws its own
/// notification-area glyphs with on a light taskbar.
const DARK_INK: [u8; 3] = [0x1a, 0x1a, 0x1a];
const LIGHT_INK: [u8; 3] = [0xff, 0xff, 0xff];

/// The one platform-conditional decision in this module.
pub fn paint(host: TrayHost, theme: Option<TaskbarTheme>) -> MarkPaint {
    match host {
        TrayHost::MacOs => MarkPaint::Template,
        TrayHost::Other => MarkPaint::Solid(match theme {
            Some(TaskbarTheme::Light) => DARK_INK,
            Some(TaskbarTheme::Dark) | None => LIGHT_INK,
        }),
    }
}

/// Windows' taskbar light/dark setting, or `None` on every other host and
/// whenever the value is absent or not a `DWORD`.
#[cfg(target_os = "windows")]
fn taskbar_theme() -> Option<TaskbarTheme> {
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};

    let wide = |value: &str| -> Vec<u16> { value.encode_utf16().chain(Some(0)).collect() };
    let subkey = wide(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize");
    let name = wide("SystemUsesLightTheme");
    let mut data = 0_u32;
    let mut size = std::mem::size_of::<u32>() as u32;
    // SAFETY: both strings are NUL-terminated for the length of the call, and
    // the output buffer is exactly the `DWORD` the flags demand.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            subkey.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            (&raw mut data).cast(),
            &mut size,
        )
    };
    if status != ERROR_SUCCESS {
        return None;
    }
    Some(if data == 0 {
        TaskbarTheme::Dark
    } else {
        TaskbarTheme::Light
    })
}

#[cfg(not(target_os = "windows"))]
fn taskbar_theme() -> Option<TaskbarTheme> {
    None
}

/// Rasterises `language`'s mark.
///
/// Costs one SVG parse and one rasterisation, on the calling (main) thread.
/// That is a few hundred microseconds and happens only when the user picks a
/// different language from the menu, so there is deliberately no cache: a
/// `HashMap<LanguageId, Icon>` would be state to keep correct in exchange
/// for time nobody can perceive.
pub fn render(language: LanguageId, cx: &App) -> anyhow::Result<Icon> {
    let path = asset(language);
    let file = Assets::get(path).with_context(|| format!("no embedded asset at {path}"))?;

    // `render_single_frame` multiplies the scale by `SMOOTH_SVG_SCALE_FACTOR`
    // (2) and by the SVG's own intrinsic size, so this is the scale that lands
    // on `ICON_HEIGHT_PX` rather than a magic number.
    let scale = ICON_HEIGHT_PX / SVG_HEIGHT_UNITS / gpui::SMOOTH_SVG_SCALE_FACTOR;
    let image = cx
        .svg_renderer()
        .render_single_frame(&file.data, scale)
        // `usvg::Error` is not `std::error::Error`, so it cannot be `?`-ed into
        // `anyhow` and has to be rendered here.
        .map_err(|error| anyhow!("could not rasterise {path}: {error}"))?;

    let size = image.size(0);
    let bgra = image
        .as_bytes(0)
        .with_context(|| format!("{path} rasterised to no frame"))?;

    Icon::from_rgba(
        mark_rgba(bgra, paint(TrayHost::current(), taskbar_theme())),
        size.width.0 as u32,
        size.height.0 as u32,
    )
    .map_err(|error| anyhow!("{path} is not a usable tray icon: {error}"))
}

fn asset(language: LanguageId) -> &'static str {
    match language {
        LanguageId::English => "icons/tray/dodo-en.svg",
        LanguageId::Vietnamese => "icons/tray/dodo-vi.svg",
        LanguageId::Japanese => "icons/tray/dodo-ja.svg",
    }
}

/// gpui's premultiplied BGRA -> the straight RGBA `tray_icon` wants.
///
/// The alpha survives byte for byte and the colour is *replaced* rather than
/// un-premultiplied: gpui rendered the mark in whatever colour the SVG names,
/// and neither host wants that. A template image ignores the channels
/// altogether; everywhere else they are the flat ink [`paint`] chose, at full
/// intensity, because straight RGBA lets the alpha do the anti-aliasing.
///
/// Pure, so the one property that matters is a unit test rather than something
/// to squint at in a menu bar or a notification area.
fn mark_rgba(bgra: &[u8], paint: MarkPaint) -> Vec<u8> {
    let [red, green, blue] = match paint {
        MarkPaint::Template => [0, 0, 0],
        MarkPaint::Solid(ink) => ink,
    };
    let mut rgba = Vec::with_capacity(bgra.len());
    for pixel in bgra.as_chunks::<4>().0 {
        rgba.extend_from_slice(&[red, green, blue, pixel[3]]);
    }
    rgba
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every language's mark has to be *reachable*, and the failure this
    /// prevents is silent: a missing file leaves `Assets::get` returning `None`
    /// and dodo drawing an empty status item. Adding a language without adding
    /// its asset should break the build, not the menu bar.
    #[test]
    fn every_language_has_an_embedded_asset() {
        for language in LanguageId::ALL {
            let path = asset(language);
            assert!(
                Assets::get(path).is_some(),
                "{language:?} names {path}, which is not embedded — is it under \
                 assets/icons/tray/ with an .svg extension?"
            );
        }
    }

    /// The language leg of the switch chain, in the only form a headless test
    /// can reach it: three languages, three different marks. One asset named
    /// twice would leave the tray showing the wrong glyph after a switch that
    /// otherwise worked, which is invisible to every other test here.
    #[test]
    fn each_language_names_its_own_mark() {
        let mut seen = std::collections::HashSet::new();
        for language in LanguageId::ALL {
            assert!(
                seen.insert(asset(language)),
                "{language:?} shares {} with another language",
                asset(language)
            );
        }
        assert_eq!(seen.len(), LanguageId::ALL.len());
    }

    /// The `icons/**/*.svg` filter in `src/assets.rs` is what makes the line
    /// above true, and `**` reaching into a subdirectory is the part that is
    /// easy to assume and wrong. Asserting it here means a future narrowing of
    /// that filter fails in this module rather than at runtime.
    #[test]
    fn the_tray_assets_live_under_the_existing_icon_filter() {
        for language in LanguageId::ALL {
            let path = asset(language);
            assert!(
                path.starts_with("icons/") && path.ends_with(".svg"),
                "{path} is outside the embed filter src/assets.rs already has"
            );
        }
    }

    /// macOS is unchanged and must stay unchanged: a template image reads its
    /// alpha and nothing else, so the colour channels are zero however the
    /// taskbar question is answered.
    #[test]
    fn macos_keeps_its_template_image() {
        for theme in [None, Some(TaskbarTheme::Light), Some(TaskbarTheme::Dark)] {
            assert_eq!(
                paint(TrayHost::MacOs, theme),
                MarkPaint::Template,
                "{theme:?}"
            );
        }

        // Two pixels of premultiplied BGRA with colour that must not leak.
        let bgra = [10, 20, 30, 200, 1, 2, 3, 0];
        assert_eq!(
            mark_rgba(&bgra, MarkPaint::Template),
            vec![0, 0, 0, 200, 0, 0, 0, 0]
        );
    }

    /// The defect: an all-zero colour buffer is a solid black dodo on Windows,
    /// where nothing recolours the bitmap. Each taskbar setting gets the ink
    /// that is legible against it, and an unknown setting gets the one that is
    /// legible against the default.
    #[test]
    fn a_non_template_host_gets_an_ink_that_contrasts_with_the_taskbar() {
        assert_eq!(
            paint(TrayHost::Other, Some(TaskbarTheme::Dark)),
            MarkPaint::Solid(LIGHT_INK)
        );
        assert_eq!(
            paint(TrayHost::Other, Some(TaskbarTheme::Light)),
            MarkPaint::Solid(DARK_INK)
        );
        assert_eq!(
            paint(TrayHost::Other, None),
            MarkPaint::Solid(LIGHT_INK),
            "a taskbar we cannot ask about is dark, which is Windows' default"
        );

        // Opaque pixels take the ink; transparent ones stay transparent, and the
        // alpha still comes through byte for byte so the edges anti-alias.
        let bgra = [10, 20, 30, 255, 1, 2, 3, 0, 4, 5, 6, 128];
        assert_eq!(
            mark_rgba(&bgra, MarkPaint::Solid(LIGHT_INK)),
            vec![255, 255, 255, 255, 255, 255, 255, 0, 255, 255, 255, 128]
        );
        assert_ne!(
            mark_rgba(&bgra, MarkPaint::Solid(LIGHT_INK))[..3],
            [0, 0, 0],
            "an all-zero colour channel is the black icon this fixes"
        );
    }

    #[test]
    fn a_partial_trailing_pixel_is_dropped_rather_than_panicking() {
        // `chunks_exact` is the reason this is safe; the test pins it, because
        // switching to `chunks` would silently produce a mis-sized buffer that
        // `Icon::from_rgba` would then reject at runtime.
        assert_eq!(
            mark_rgba(&[1, 2, 3, 4, 5], MarkPaint::Template),
            vec![0, 0, 0, 4]
        );
    }
}
