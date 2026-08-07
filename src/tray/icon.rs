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
//! # Why only the alpha channel matters
//!
//! The status item is a macOS **template** image (`setTemplate:`), which means
//! AppKit reads its alpha and paints the result itself — dark on a light menu
//! bar, light on a dark one, inverted while the menu is open. So the colour
//! channels are set to zero rather than un-premultiplied out of gpui's buffer:
//! there is no value there to preserve, and pretending otherwise would be a
//! conversion nobody could see the result of.

use anyhow::{Context as _, anyhow};
use gpui::App;
use tray_icon::Icon;

use crate::assets::Assets;
use crate::tray::input_language::InputLanguage;

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

/// Rasterises `language`'s mark.
///
/// Costs one SVG parse and one rasterisation, on the calling (main) thread.
/// That is a few hundred microseconds and happens only when the user picks a
/// different language from the menu, so there is deliberately no cache: a
/// `HashMap<InputLanguage, Icon>` would be state to keep correct in exchange
/// for time nobody can perceive.
pub fn render(language: InputLanguage, cx: &App) -> anyhow::Result<Icon> {
    let path = language.asset();
    let file = Assets::get(&path).with_context(|| format!("no embedded asset at {path}"))?;

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
        alpha_mask_rgba(bgra),
        size.width.0 as u32,
        size.height.0 as u32,
    )
    .map_err(|error| anyhow!("{path} is not a usable tray icon: {error}"))
}

/// gpui's premultiplied BGRA -> the straight RGBA `tray_icon` encodes to PNG.
///
/// Pure, so the one interesting property — that the alpha survives byte for
/// byte and the colour channels do not carry stale premultiplied values — is a
/// unit test rather than something to squint at in the menu bar.
fn alpha_mask_rgba(bgra: &[u8]) -> Vec<u8> {
    let mut rgba = Vec::with_capacity(bgra.len());
    for pixel in bgra.chunks_exact(4) {
        rgba.extend_from_slice(&[0, 0, 0, pixel[3]]);
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
    fn every_input_language_has_an_embedded_asset() {
        for language in InputLanguage::ALL {
            let path = language.asset();
            assert!(
                Assets::get(&path).is_some(),
                "{language:?} names {path}, which is not embedded — is it under \
                 assets/icons/tray/ with an .svg extension?"
            );
        }
    }

    /// The `icons/**/*.svg` filter in `src/assets.rs` is what makes the line
    /// above true, and `**` reaching into a subdirectory is the part that is
    /// easy to assume and wrong. Asserting it here means a future narrowing of
    /// that filter fails in this module rather than at runtime.
    #[test]
    fn the_tray_assets_live_under_the_existing_icon_filter() {
        for language in InputLanguage::ALL {
            let path = language.asset();
            assert!(
                path.starts_with("icons/") && path.ends_with(".svg"),
                "{path} is outside the embed filter src/assets.rs already has"
            );
        }
    }

    #[test]
    fn only_the_alpha_channel_survives() {
        // Two pixels of premultiplied BGRA with colour that must not leak.
        let bgra = [10, 20, 30, 200, 1, 2, 3, 0];
        assert_eq!(alpha_mask_rgba(&bgra), vec![0, 0, 0, 200, 0, 0, 0, 0]);
    }

    #[test]
    fn a_partial_trailing_pixel_is_dropped_rather_than_panicking() {
        // `chunks_exact` is the reason this is safe; the test pins it, because
        // switching to `chunks` would silently produce a mis-sized buffer that
        // `Icon::from_rgba` would then reject at runtime.
        assert_eq!(alpha_mask_rgba(&[1, 2, 3, 4, 5]), vec![0, 0, 0, 4]);
    }
}
