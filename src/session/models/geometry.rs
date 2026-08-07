//! Turning a saved rectangle back into a window that is actually reachable.
//!
//! This is the part of session restoration that goes wrong, and it goes wrong
//! silently: the app opens somewhere the user cannot see, and to them dodo has
//! simply stopped starting. Everything here is a pure function over rectangles
//! so the whole of it is tested without a frame.
//!
//! # gpui does none of this for us
//!
//! Worth stating, because it is easy to assume otherwise. `gpui::Window::new`
//! only sanity-checks placement in `default_bounds`, the branch it takes when
//! `WindowOptions::window_bounds` is `None` — cascade offset, display clamp and
//! all. Hand it `Some(bounds)` and it passes the rectangle straight to
//! `platform.open_window` unexamined (verified in the pinned checkout,
//! `gpui/src/window.rs`). Restoring a window therefore opts *out* of gpui's own
//! placement care, and [`place`] is what has to replace it.
//!
//! The upside of that is the one thing it would otherwise be easy to get wrong:
//! because gpui does not cascade a supplied rectangle, a restored window does
//! **not** creep 25px down and right on every launch.
//!
//! # The rules
//!
//! 1. **No displays at all** — nothing to reason about, so no opinion:
//!    [`place`] returns `None` and the caller opens its default window.
//! 2. **A size that cannot be honoured is corrected, not discarded.** It is
//!    raised to `min` (`layout::window_min_size`, 600×440 since the sidebar
//!    round) and lowered to the chosen display, in that order, so a saved
//!    window from a 5K monitor opens usable on a laptop panel.
//! 3. **The display is chosen by overlap**, most-overlapping first. That is the
//!    display the user actually had the window on, which is the one to keep it
//!    on when several are attached.
//! 4. **Overlapping nothing means the display is gone.** The size is kept — it
//!    is the half of "remember the window size" that still makes sense — and
//!    the window is centred on the primary display. This is the unplugged
//!    second monitor, and it is why restoring blind is not an option.
//! 5. **Whatever display is chosen, the window ends up inside it.** The origin
//!    is clamped last, so even a one-pixel overlap resolves to a fully visible
//!    window rather than a mostly-offscreen one.
//!
//! Rule 5 also makes [`place`] **idempotent**: re-placing a rectangle it
//! already produced changes nothing, which is what keeps a window from drifting
//! across restarts as each launch saves what the last one clamped.
//!
//! # Which display rectangle
//!
//! `visible_bounds()`, not `bounds()` — it excludes the macOS menu bar and the
//! Windows taskbar, so a clamped window is under neither. It is what gpui's own
//! `default_bounds` uses.

use gpui::{Bounds, Pixels, Point, Size, point, px, size};

use super::document::WindowRecord;

/// Where to open a window whose saved rectangle is `saved`.
///
/// `displays` is every attached display's usable area, **primary first**;
/// `min` is the smallest window the platform is being asked to allow. Returns
/// `None` only when there is nothing to place against, which leaves the caller
/// on its own default.
pub fn place(
    saved: Bounds<Pixels>,
    displays: &[Bounds<Pixels>],
    min: Size<Pixels>,
) -> Option<Bounds<Pixels>> {
    if !is_sane(saved) {
        return None;
    }

    let usable: Vec<Bounds<Pixels>> = displays.iter().copied().filter(|d| is_sane(*d)).collect();
    let (primary, rest) = usable.split_first()?;

    // Rule 3: the display the window was most on. `max_by` keeps the *last*
    // maximum, so it is asked to compare against the primary explicitly rather
    // than being handed the whole list — a tie has to go to the primary.
    let display = rest.iter().fold(*primary, |best, candidate| {
        if overlap(saved, *candidate) > overlap(saved, best) {
            *candidate
        } else {
            best
        }
    });

    // Rule 2, and it has to run before the origin is decided: a size wider than
    // the display leaves no origin that could fit it.
    let fitted = clamp_size(saved.size, display.size, min);

    // Rule 4: nothing overlaps anything, so the display it was on is gone.
    if overlap(saved, display) <= px(0.) {
        return Some(centered_in(
            *primary,
            clamp_size(saved.size, primary.size, min),
        ));
    }

    // Rule 5.
    Some(Bounds {
        origin: clamp_origin(saved.origin, fitted, display),
        size: fitted,
    })
}

/// Same, from what the file actually holds.
///
/// A record whose numbers do not describe a rectangle is `None`, which the
/// caller reads as "no saved geometry" — the same answer a first run gives.
pub fn place_record(
    record: &WindowRecord,
    displays: &[Bounds<Pixels>],
    min: Size<Pixels>,
) -> Option<Bounds<Pixels>> {
    place(
        Bounds {
            origin: point(px(record.x), px(record.y)),
            size: size(px(record.width), px(record.height)),
        },
        displays,
        min,
    )
}

/// The rectangle as the file holds it.
pub fn record_of(bounds: Bounds<Pixels>) -> (f32, f32, f32, f32) {
    (
        f32::from(bounds.origin.x),
        f32::from(bounds.origin.y),
        f32::from(bounds.size.width),
        f32::from(bounds.size.height),
    )
}

/// A rectangle with finite coordinates and a positive size.
///
/// JSON cannot spell `NaN` or `Infinity`, so serde would already have refused
/// those — but a hand-edited `1e40` parses fine and multiplying it out later
/// does reach infinity, and a zero or negative size is a window with nothing in
/// it. Both are cheaper to reject here than to reason about downstream.
fn is_sane(bounds: Bounds<Pixels>) -> bool {
    let finite = [
        bounds.origin.x,
        bounds.origin.y,
        bounds.size.width,
        bounds.size.height,
    ]
    .into_iter()
    .all(|value| f32::from(value).is_finite());

    finite && !bounds.is_empty()
}

/// The area two rectangles share, `0` when they share none.
fn overlap(a: Bounds<Pixels>, b: Bounds<Pixels>) -> Pixels {
    let shared = a.intersect(&b);
    if shared.is_empty() {
        return px(0.);
    }
    px(f32::from(shared.size.width) * f32::from(shared.size.height))
}

/// At least `min`, at most `display` — in that order, so a display smaller than
/// the app's own minimum yields the minimum rather than an inverted range.
fn clamp_size(wanted: Size<Pixels>, display: Size<Pixels>, min: Size<Pixels>) -> Size<Pixels> {
    let axis = |wanted: Pixels, display: Pixels, min: Pixels| wanted.max(min).min(display.max(min));

    size(
        axis(wanted.width, display.width, min.width),
        axis(wanted.height, display.height, min.height),
    )
}

/// Slides a rectangle of `size` until it is inside `display`.
///
/// The caller has already fitted the size, so the low end of each range is
/// never above the high end.
fn clamp_origin(
    origin: Point<Pixels>,
    size: Size<Pixels>,
    display: Bounds<Pixels>,
) -> Point<Pixels> {
    let axis = |value: Pixels, low: Pixels, high: Pixels| value.max(low).min(high.max(low));

    point(
        axis(origin.x, display.origin.x, display.right() - size.width),
        axis(origin.y, display.origin.y, display.bottom() - size.height),
    )
}

/// A rectangle of `size` in the middle of `display`.
fn centered_in(display: Bounds<Pixels>, size: Size<Pixels>) -> Bounds<Pixels> {
    let origin = point(
        display.origin.x + (display.size.width - size.width) / 2.,
        display.origin.y + (display.size.height - size.height) / 2.,
    );
    Bounds {
        origin: clamp_origin(origin, size, display),
        size,
    }
}

#[cfg(test)]
mod tests {
    use gpui::{Bounds, Pixels, Size, point, px, size};

    use super::{place, place_record};
    use crate::session::models::document::{WindowMode, WindowRecord};

    /// The two displays every case here is reasoned about: a laptop panel at
    /// the origin, and an external monitor to the right of it.
    fn laptop() -> Bounds<Pixels> {
        Bounds {
            origin: point(px(0.), px(0.)),
            size: size(px(1512.), px(945.)),
        }
    }

    fn external() -> Bounds<Pixels> {
        Bounds {
            origin: point(px(1512.), px(-200.)),
            size: size(px(2560.), px(1440.)),
        }
    }

    /// dodo's own floor, as `layout::window_min_size` states it.
    fn min() -> Size<Pixels> {
        size(px(600.), px(440.))
    }

    fn rect(x: f32, y: f32, w: f32, h: f32) -> Bounds<Pixels> {
        Bounds {
            origin: point(px(x), px(y)),
            size: size(px(w), px(h)),
        }
    }

    #[test]
    fn an_ordinary_saved_window_comes_back_exactly_where_it_was() {
        let saved = rect(120., 80., 1000., 700.);
        assert_eq!(place(saved, &[laptop(), external()], min()), Some(saved));
    }

    /// The claim the whole module exists to make: the second display is gone,
    /// so the window is not restored into the void.
    #[test]
    fn a_window_left_on_an_unplugged_display_comes_back_centred_on_the_primary() {
        let saved = rect(2000., 300., 1200., 800.);
        let placed = place(saved, &[laptop()], min()).expect("placed somewhere");

        assert_eq!(
            placed.size, saved.size,
            "the size is the half of this that still means something",
        );
        assert!(
            laptop().intersect(&placed) == placed,
            "{placed:?} has to be wholly inside {:?}",
            laptop()
        );
        // Centred, not merely nudged in.
        assert_eq!(placed.origin, point(px(156.), px(72.5)));
    }

    /// Same monitor, still attached: nothing moves.
    #[test]
    fn a_window_on_the_second_display_stays_on_the_second_display() {
        let saved = rect(1800., 100., 1400., 900.);
        assert_eq!(place(saved, &[laptop(), external()], min()), Some(saved));
    }

    /// The resolution changed under a saved window — 4K to 1080p, say. It has
    /// to shrink rather than open with its right-hand half unreachable.
    #[test]
    fn a_window_larger_than_its_display_is_shrunk_to_fit() {
        let saved = rect(0., 0., 3840., 2160.);
        let placed = place(saved, &[laptop()], min()).expect("placed");

        assert_eq!(placed, laptop());
    }

    /// The minimum the sidebar round introduced. A file predating it, or one
    /// hand-edited, must not open a window smaller than the layout can hold.
    #[test]
    fn a_saved_size_below_the_minimum_is_raised_to_it() {
        let placed = place(rect(10., 10., 300., 200.), &[laptop()], min()).expect("placed");
        assert_eq!(placed.size, min());
    }

    /// …and if the display itself is smaller than dodo's minimum, the minimum
    /// wins. Clamping to the display first would invert the range.
    #[test]
    fn a_display_smaller_than_the_minimum_still_yields_the_minimum() {
        let tiny = rect(0., 0., 480., 320.);
        let placed = place(rect(0., 0., 900., 620.), &[tiny], min()).expect("placed");
        assert_eq!(placed.size, min());
    }

    /// Mostly offscreen is still restored — but wholly onscreen. A window whose
    /// title bar is off the top of the display is one the user cannot drag back.
    #[test]
    fn a_window_hanging_off_the_edge_is_pulled_back_inside() {
        for saved in [
            rect(-800., 100., 1000., 700.),
            rect(1400., 100., 1000., 700.),
            rect(100., -600., 1000., 700.),
            rect(100., 800., 1000., 700.),
        ] {
            let placed = place(saved, &[laptop()], min()).expect("placed");
            assert_eq!(
                laptop().intersect(&placed),
                placed,
                "{saved:?} restored to {placed:?}, which is not wholly on the display",
            );
            assert_eq!(placed.size, saved.size, "only the origin should move");
        }
    }

    /// The property that keeps a window from walking across the screen: each
    /// launch saves what the last one placed, so placing twice must be placing
    /// once.
    #[test]
    fn placing_an_already_placed_window_changes_nothing() {
        let displays = [laptop(), external()];
        for saved in [
            rect(120., 80., 1000., 700.),
            rect(-800., 100., 1000., 700.),
            rect(9000., 9000., 4000., 3000.),
            rect(0., 0., 10., 10.),
        ] {
            let once = place(saved, &displays, min()).expect("placed");
            let twice = place(once, &displays, min()).expect("placed again");
            assert_eq!(once, twice, "{saved:?} is not a fixed point after one pass");
        }
    }

    #[test]
    fn with_no_displays_there_is_no_opinion_to_have() {
        assert_eq!(place(rect(0., 0., 900., 620.), &[], min()), None);
    }

    /// Nonsense in the file is "no saved geometry", which is what a first run
    /// looks like — so the caller's default window opens rather than a
    /// zero-sized one.
    #[test]
    fn a_rectangle_that_is_not_a_rectangle_is_refused() {
        for saved in [
            rect(0., 0., 0., 620.),
            rect(0., 0., 900., 0.),
            rect(0., 0., -900., -620.),
            rect(f32::INFINITY, 0., 900., 620.),
            rect(0., 0., f32::NAN, 620.),
        ] {
            assert_eq!(place(saved, &[laptop()], min()), None, "{saved:?}");
        }
    }

    /// A display list where the primary itself is nonsense — a headless or
    /// mid-reconfiguration machine — must not be trusted into an answer.
    #[test]
    fn a_degenerate_display_is_ignored() {
        let broken = rect(0., 0., 0., 0.);
        assert_eq!(place(rect(0., 0., 900., 620.), &[broken], min()), None);

        // …and with a real display behind it, that one is used.
        let placed = place(rect(120., 80., 1000., 700.), &[broken, laptop()], min());
        assert_eq!(placed, Some(rect(120., 80., 1000., 700.)));
    }

    /// A window straddling two displays goes to whichever holds more of it.
    #[test]
    fn a_straddling_window_lands_on_the_display_it_was_mostly_on() {
        // 90% on the external monitor, 10% on the laptop.
        let saved = rect(1412., 100., 1000., 700.);
        let placed = place(saved, &[laptop(), external()], min()).expect("placed");
        assert_eq!(
            external().intersect(&placed),
            placed,
            "{placed:?} should have been resolved onto the external display",
        );
    }

    /// The list is preference-ordered, not merely display-ordered: whichever
    /// display is first is the one an unplaceable window is centred on. It is
    /// how `Session::window_bounds` says "put it back on the monitor it was
    /// on", and on macOS it is the *only* thing that can say so — every
    /// coordinate gpui reports there is display-local.
    #[test]
    fn the_first_display_is_the_one_a_homeless_window_lands_on() {
        // Overlapping neither display, so rule 4 decides.
        let saved = rect(9000., 9000., 1000., 700.);

        let on_laptop = place(saved, &[laptop(), external()], min()).expect("placed");
        assert_eq!(
            laptop().intersect(&on_laptop),
            on_laptop,
            "with the laptop preferred it must land there",
        );

        let on_external = place(saved, &[external(), laptop()], min()).expect("placed");
        assert_eq!(
            external().intersect(&on_external),
            on_external,
            "and with the external monitor preferred, there instead",
        );
    }

    #[test]
    fn a_record_from_the_file_is_placed_the_same_way() {
        let record = WindowRecord {
            mode: WindowMode::Maximized,
            x: 120.,
            y: 80.,
            width: 1000.,
            height: 700.,
            display: None,
        };
        assert_eq!(
            place_record(&record, &[laptop()], min()),
            Some(rect(120., 80., 1000., 700.)),
        );
    }

    #[test]
    fn the_record_of_a_rectangle_is_its_four_numbers() {
        assert_eq!(
            super::record_of(rect(12., 34., 900., 620.)),
            (12., 34., 900., 620.)
        );
    }
}
