//! The preview's view transform: how far one zoom step moves, how far one
//! wheel notch moves, and where the pan has to land so the point under the
//! pointer stays under it.
//!
//! **No GPUI here**, the same rule and the same reason as [`crate::render`]:
//! every number below is an `f32` over screen pixels gpui already computed
//! this frame, so the arithmetic deciding what a gesture *means* is a plain
//! `#[test]` away — which matters more in this crate than most, because
//! [`crate::view`]'s module doc records that a `#[gpui::test]` cannot be added
//! here at all at the pinned revision.
//!
//! Zoom is a multiplier over "fit the preview pane", never over the image's
//! own pixels; [`crate::view`]'s module doc says why that makes
//! fit-on-first-render and preserve-zoom-on-edit free. Everything here is
//! therefore unitless except [`anchored_pan`], whose pan and cursor offsets
//! are screen pixels.

/// How far one `+`/`-` press or one `cmd-=`/`cmd--` moves the zoom.
/// Multiplicative, so repeated steps feel even whether zooming in or out.
pub(crate) const ZOOM_STEP: f32 = 1.25;

/// The zoom range, as a multiplier over "fit". `1.0` is fit; this is generous
/// enough for a close read of a dense diagram or a wide view of a small one,
/// without letting a stray scroll send the diagram somewhere the user has to
/// hunt for the Fit button to escape.
pub(crate) const MIN_ZOOM: f32 = 0.1;
pub(crate) const MAX_ZOOM: f32 = 8.0;

/// How many screen pixels one *line* of wheel scroll is worth.
///
/// A mouse wheel reports `ScrollDelta::Lines` and a trackpad reports
/// `ScrollDelta::Pixels`; gpui's `ScrollDelta::pixel_delta` normalises the two
/// against a line height, and this is the one the preview hands it. Same value
/// and same reasoning as `dodo-flow`'s canvas: it is not a *text* line height
/// — nothing being scrolled here has text — so it is stated rather than
/// borrowed from a font.
pub(crate) const SCROLL_LINE_HEIGHT: f32 = 20.0;

/// `zoom` scaled by `factor`, held inside [`MIN_ZOOM`]..=[`MAX_ZOOM`].
///
/// **Every** zoom change in this crate goes through here. The buttons, the key
/// bindings and the wheel used to be free to clamp or not clamp on their own,
/// which is exactly the shape that lets a third caller arrive later and forget
/// — and a zoom outside the range does not fail loudly, it paints a diagram
/// nobody can find.
pub(crate) fn scaled(zoom: f32, factor: f32) -> f32 {
    let scaled = zoom * factor;
    if !scaled.is_finite() {
        // Not reachable from the button or key paths, but a wheel delta is
        // platform-supplied and a NaN zoom would paint nothing at all with no
        // visible cause. Fit is the one value that is always meaningful.
        return 1.0;
    }
    scaled.clamp(MIN_ZOOM, MAX_ZOOM)
}

/// One `+` step.
pub(crate) fn stepped_in(zoom: f32) -> f32 {
    scaled(zoom, ZOOM_STEP)
}

/// One `-` step.
pub(crate) fn stepped_out(zoom: f32) -> f32 {
    scaled(zoom, 1.0 / ZOOM_STEP)
}

/// The zoom factor `vertical_pixels` of wheel travel asks for.
///
/// [`ZOOM_STEP`] per [`SCROLL_LINE_HEIGHT`] of travel, *exponentially*: one
/// full notch lands on exactly the factor a button press does, while a
/// trackpad's stream of small deltas composes into it smoothly instead of
/// stepping in the button's coarse jumps. `dodo-flow`'s `wheel_zoom_factor` is
/// the same formula over its own step — the two zoomable surfaces in dodo
/// deliberately do not invent separate wheel rules.
pub(crate) fn wheel_factor(vertical_pixels: f32) -> f32 {
    ZOOM_STEP.powf(vertical_pixels / SCROLL_LINE_HEIGHT)
}

/// The pan that keeps the point under the pointer under the pointer.
///
/// One axis at a time: the two are independent, and a `Point` here would drag
/// gpui's geometry types into a module that deliberately has none.
///
/// `pan` is the axis's pan as `MermaidTab::pan` stores it (screen pixels from
/// centred) and `cursor_from_centre` is the pointer's offset from the preview
/// pane's centre on the same axis. `factor` must be the zoom change that was
/// **actually applied** — `after / before` once [`scaled`] has clamped — not
/// the factor that was asked for: passing the requested one makes the diagram
/// creep out from under the pointer on every notch turned at either end of the
/// range, which is precisely when a user is leaning on the wheel hardest.
///
/// Derivation. `paint_preview_image` puts the image at `centre - natural·s/2 +
/// pan` with `s = fit·zoom`, so the image-local point under the pointer is
/// `(cursor_from_centre - pan)/s + natural/2`. Holding that equal across a
/// change of `s` (with `fit` fixed, because the pane did not resize) leaves
/// `(d - pan')/s' = (d - pan)/s`, and with `s'/s = factor` that is the one
/// line below.
pub(crate) fn anchored_pan(pan: f32, cursor_from_centre: f32, factor: f32) -> f32 {
    cursor_from_centre - factor * (cursor_from_centre - pan)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The image-local coordinate under the pointer, in the image's own
    /// pixels. [`anchored_pan`] exists to hold this constant, so the tests
    /// assert on it rather than on the pan it produces.
    fn under_pointer(cursor_from_centre: f32, pan: f32, scale: f32) -> f32 {
        (cursor_from_centre - pan) / scale
    }

    #[test]
    fn a_step_in_and_a_step_out_return_to_where_they_started() {
        let zoom = stepped_out(stepped_in(1.0));
        assert!((zoom - 1.0).abs() < 1e-5, "{zoom}");
    }

    #[test]
    fn stepping_in_repeatedly_stops_at_the_ceiling() {
        let mut zoom = 1.0;
        for _ in 0..100 {
            zoom = stepped_in(zoom);
        }
        assert_eq!(zoom, MAX_ZOOM);
    }

    #[test]
    fn stepping_out_repeatedly_stops_at_the_floor() {
        let mut zoom = 1.0;
        for _ in 0..100 {
            zoom = stepped_out(zoom);
        }
        assert_eq!(zoom, MIN_ZOOM);
    }

    /// The wheel is the third caller of [`scaled`] and the only one whose
    /// factor comes from outside dodo, so it gets its own bounds assertion.
    #[test]
    fn no_amount_of_wheel_travel_leaves_the_bounds() {
        for pixels in [-10_000.0, -600.0, -20.0, 0.0, 20.0, 600.0, 10_000.0] {
            let zoom = scaled(1.0, wheel_factor(pixels));
            assert!(
                (MIN_ZOOM..=MAX_ZOOM).contains(&zoom),
                "{pixels}px of wheel travel left the range: {zoom}"
            );
        }
    }

    #[test]
    fn a_full_wheel_notch_matches_one_button_step() {
        let wheeled = scaled(1.0, wheel_factor(SCROLL_LINE_HEIGHT));
        let stepped = stepped_in(1.0);
        assert!((wheeled - stepped).abs() < 1e-5, "{wheeled} vs {stepped}");
    }

    /// The reason the wheel does not simply call [`stepped_in`]: a trackpad
    /// delivers a fraction of a line at a time, and those must accumulate
    /// smoothly rather than each jumping a whole button step.
    #[test]
    fn a_partial_notch_moves_less_than_a_whole_one() {
        let partial = scaled(1.0, wheel_factor(SCROLL_LINE_HEIGHT / 4.0));
        assert!(partial > 1.0, "{partial}");
        assert!(partial < stepped_in(1.0), "{partial}");
    }

    /// Four quarter-notches are one notch: the exponential form is what makes
    /// a trackpad's stream indistinguishable from a wheel's single click.
    #[test]
    fn partial_notches_compose_into_a_whole_one() {
        let mut zoom = 1.0;
        for _ in 0..4 {
            zoom = scaled(zoom, wheel_factor(SCROLL_LINE_HEIGHT / 4.0));
        }
        let stepped = stepped_in(1.0);
        assert!((zoom - stepped).abs() < 1e-5, "{zoom} vs {stepped}");
    }

    #[test]
    fn wheeling_one_way_zooms_in_and_the_other_way_zooms_out() {
        assert!(wheel_factor(SCROLL_LINE_HEIGHT) > 1.0);
        assert!(wheel_factor(-SCROLL_LINE_HEIGHT) < 1.0);
        assert_eq!(wheel_factor(0.0), 1.0);
    }

    #[test]
    fn a_non_finite_factor_falls_back_to_fit() {
        assert_eq!(scaled(1.0, f32::NAN), 1.0);
        assert_eq!(scaled(f32::NAN, 1.0), 1.0);
        assert_eq!(scaled(1.0, f32::INFINITY), 1.0);
    }

    #[test]
    fn an_anchored_zoom_holds_the_point_under_the_pointer() {
        let fit = 0.4;
        for cursor in [-300.0, -1.0, 0.0, 7.5, 220.0] {
            for pan in [-90.0, 0.0, 45.0] {
                for factor in [0.5, 0.8, 1.0, 1.25, 3.0] {
                    let before = 1.6;
                    let after = before * factor;
                    let moved = anchored_pan(pan, cursor, factor);

                    let was = under_pointer(cursor, pan, fit * before);
                    let is = under_pointer(cursor, moved, fit * after);
                    assert!(
                        (was - is).abs() < 1e-3,
                        "cursor {cursor}, pan {pan}, factor {factor}: {was} moved to {is}"
                    );
                }
            }
        }
    }

    /// A zoom the clamp refused must not move the diagram either — the pan is
    /// derived from the factor that was applied, and at the ceiling that is
    /// exactly `1.0`.
    #[test]
    fn a_zoom_the_clamp_refused_leaves_the_pan_alone() {
        let before = MAX_ZOOM;
        let after = scaled(before, wheel_factor(SCROLL_LINE_HEIGHT));
        assert_eq!(after, MAX_ZOOM);

        let applied = after / before;
        assert_eq!(anchored_pan(37.0, -120.0, applied), 37.0);
    }

    /// With the pointer exactly on the pane's centre there is nothing to
    /// anchor to but the centre itself, so the pan simply scales with the
    /// zoom — the same thing the `+`/`-` buttons do, which is why they need no
    /// anchor of their own.
    #[test]
    fn zooming_at_the_centre_just_scales_the_pan() {
        assert_eq!(anchored_pan(40.0, 0.0, 2.0), 80.0);
    }
}
