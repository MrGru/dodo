//! [`Viewport`] — **the single owner of world↔screen**.
//!
//! Requirements §22 is explicit: centralise `world -> screen` and
//! `screen -> world`, and do not scatter transform formulas across node
//! renderers. Every transform in the engine goes through this file. There are
//! exactly two formulas here and everything else is expressed in terms of them:
//!
//! ```text
//! screen = world * zoom + pan
//! world  = (screen - pan) / zoom
//! ```
//!
//! **`pan` is in screen pixels, not world units**, and that is the decision the
//! rest of the file follows from. Storing the pan in world units would make it
//! change meaning every time the zoom changed, so a pan-then-zoom sequence
//! would drift; in screen units a pan is exactly the pixel offset of the world
//! origin from the viewport's top-left, whatever the zoom. It is the same
//! convention React Flow's `[x, y, zoom]` transform uses, which also makes
//! documents interchangeable with the ecosystem the requirements draw from.
//!
//! **Screen coordinates here are the *pane's* coordinates**, with the origin at
//! the top-left of the canvas element, not the window. `views/` subtracts the
//! element's bounds origin before it asks anything of this type — so nothing in
//! here has to know where the pane sits, and a sidebar opening does not move
//! the world.
//!
//! Cursor-anchored zoom ([`Viewport::zoom_around`]) is the reason this type has
//! to be the only owner. It is three lines and it is wrong in a subtle way if
//! the pan and the zoom are updated by two different call sites; the tests at
//! the bottom of this file are the guard.
//!
//! **This file names no UI framework.** `size` is the pane's size in *logical*
//! pixels, expressed as a [`Vec2`] — the conversion from `Bounds<Pixels>`
//! happens in `views/`.

use serde::{Deserialize, Serialize};

use crate::geometry::{Rect, Vec2};

/// The window onto the infinite world: where it is, and how magnified.
///
/// Serializable because a document remembers where it was last left — but note
/// that it is the *view* state, not the document's content, and dodo's
/// persistence rules decide which file it lands in.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Viewport {
    /// Screen-pixel offset of the world origin from the pane's top-left corner.
    pan: Vec2,
    /// Screen pixels per world unit. Always within
    /// [`Viewport::MIN_ZOOM`]..=[`Viewport::MAX_ZOOM`].
    zoom: f32,
    /// The pane's size in logical pixels. Zero until the first frame measures
    /// it, which is why [`Viewport::visible_world_rect`] is degenerate rather
    /// than wrong before then.
    size: Vec2,
}

impl Default for Viewport {
    fn default() -> Viewport {
        Viewport {
            pan: Vec2::ZERO,
            zoom: 1.0,
            size: Vec2::ZERO,
        }
    }
}

impl Viewport {
    /// 40× out. Past this a 100-unit node is under three pixels wide and the
    /// overview LOD has long since replaced it with a box; further zoom buys
    /// nothing but a bigger visible world rect for the spatial query to walk.
    pub const MIN_ZOOM: f32 = 0.025;

    /// 40× in. The ceiling exists because zoom multiplies the flattening error
    /// of any cached tessellation (the flattening error grows as tolerance × k), and
    /// because past it the world coordinates a pane-sized rect spans stop being
    /// representable at useful `f32` precision.
    pub const MAX_ZOOM: f32 = 40.0;

    /// The multiplier one notch of a scroll wheel or one zoom-button press
    /// applies. Geometric, so zoom in then out returns to where it started.
    pub const ZOOM_STEP: f32 = 1.2;

    pub fn new(pan: Vec2, zoom: f32, size: Vec2) -> Viewport {
        Viewport {
            pan,
            zoom: zoom.clamp(Viewport::MIN_ZOOM, Viewport::MAX_ZOOM),
            size,
        }
    }

    pub fn pan(&self) -> Vec2 {
        self.pan
    }

    pub fn zoom(&self) -> f32 {
        self.zoom
    }

    pub fn size(&self) -> Vec2 {
        self.size
    }

    /// Records the pane's measured size. Called once per frame from `views/`;
    /// it changes no world position, so the content under the cursor stays put
    /// when the window is resized from its bottom-right corner.
    pub fn set_size(&mut self, size: Vec2) {
        self.size = size;
    }

    /// `screen = world * zoom + pan`.
    pub fn world_to_screen(&self, world: Vec2) -> Vec2 {
        world * self.zoom + self.pan
    }

    /// `world = (screen - pan) / zoom`.
    pub fn screen_to_world(&self, screen: Vec2) -> Vec2 {
        (screen - self.pan) / self.zoom
    }

    /// A world rectangle in screen space. Sizes scale, positions transform —
    /// which is why this cannot be two independent `world_to_screen` calls at
    /// the call site without repeating the formula, the thing §22 forbids.
    pub fn world_rect_to_screen(&self, rect: Rect) -> Rect {
        Rect::new(self.world_to_screen(rect.origin), rect.size * self.zoom)
    }

    pub fn screen_rect_to_world(&self, rect: Rect) -> Rect {
        Rect::new(self.screen_to_world(rect.origin), rect.size / self.zoom)
    }

    /// A length in world units, in screen pixels.
    pub fn world_to_screen_length(&self, length: f32) -> f32 {
        length * self.zoom
    }

    /// A length in screen pixels, in world units. This is how a constant-width
    /// stroke, a handle radius or a hit-test slop expressed in pixels becomes a
    /// world-space number the pure layers can work with.
    pub fn screen_to_world_length(&self, length: f32) -> f32 {
        length / self.zoom
    }

    /// The world rectangle currently on screen — **the culling query**, and the
    /// only argument the spatial index needs.
    ///
    /// Degenerate before the first frame has measured the pane; a spatial query
    /// against it returns nothing, which is the correct answer for a pane with
    /// no area.
    pub fn visible_world_rect(&self) -> Rect {
        Rect::new(self.screen_to_world(Vec2::ZERO), self.size / self.zoom)
    }

    /// Moves the view by a screen-space delta. `delta` is the pointer's
    /// movement, and the content follows the pointer: dragging right moves the
    /// world right.
    pub fn pan_by(&mut self, delta: Vec2) {
        self.pan += delta;
    }

    /// Puts `world` at `screen`.
    pub fn center_world_on_screen(&mut self, world: Vec2, screen: Vec2) {
        self.pan = screen - world * self.zoom;
    }

    /// **Cursor-anchored zoom: the world point under `anchor` stays under
    /// `anchor`.**
    ///
    /// Derived rather than fudged. Let `w` be the world point under the anchor
    /// before the change, `w = (anchor - pan) / zoom`. Requiring
    /// `anchor = w * zoom' + pan'` and solving for the new pan gives
    /// `pan' = anchor - w * zoom'`, which is the whole implementation.
    ///
    /// The new zoom is clamped *before* the pan is solved, so the anchor is
    /// preserved exactly even when the request runs into
    /// [`Viewport::MIN_ZOOM`] or [`Viewport::MAX_ZOOM`] — a zoom that clamps
    /// must not also drift the view sideways.
    pub fn zoom_around(&mut self, anchor: Vec2, zoom: f32) {
        let world_under_anchor = self.screen_to_world(anchor);
        self.zoom = zoom.clamp(Viewport::MIN_ZOOM, Viewport::MAX_ZOOM);
        self.pan = anchor - world_under_anchor * self.zoom;
    }

    /// Multiplies the zoom by `factor`, anchored at `anchor`. The wheel and
    /// pinch handlers' entry point: a pinch reports a ratio, and a wheel notch
    /// is [`Viewport::ZOOM_STEP`] or its reciprocal.
    pub fn zoom_by(&mut self, anchor: Vec2, factor: f32) {
        self.zoom_around(anchor, self.zoom * factor);
    }

    /// One notch in (`steps` positive) or out (negative), anchored at `anchor`.
    pub fn zoom_steps(&mut self, anchor: Vec2, steps: i32) {
        self.zoom_by(anchor, Viewport::ZOOM_STEP.powi(steps));
    }

    /// Frames `content`, leaving `padding` screen pixels on every side.
    ///
    /// A degenerate pane or degenerate content leaves the zoom alone and only
    /// centres, because "fit a zero-width rectangle" has no finite answer and
    /// the useful behaviour is to go and look at it at the current
    /// magnification.
    pub fn zoom_to_fit(&mut self, content: Rect, padding: f32) {
        let content = content.normalized();
        let available = self.size - Vec2::splat(padding * 2.0);

        if available.x > 0.0 && available.y > 0.0 && content.width() > 0.0 && content.height() > 0.0
        {
            let fit = (available.x / content.width()).min(available.y / content.height());
            self.zoom = fit.clamp(Viewport::MIN_ZOOM, Viewport::MAX_ZOOM);
        }

        self.center_world_on_screen(content.center(), self.size / 2.0);
    }

    /// Resets to the identity view: world origin at the pane's top-left, 1:1.
    pub fn reset(&mut self) {
        self.pan = Vec2::ZERO;
        self.zoom = 1.0;
    }
}

#[cfg(test)]
mod tests {
    use super::Viewport;
    use crate::geometry::{Rect, Vec2};

    /// World units are `f32` and a round-trip divides then multiplies, so exact
    /// equality is the wrong assertion. A tenth of a world unit is far below
    /// anything a canvas can express.
    const EPSILON: f32 = 1e-3;

    fn assert_close(a: Vec2, b: Vec2, what: &str) {
        assert!(
            (a.x - b.x).abs() < EPSILON && (a.y - b.y).abs() < EPSILON,
            "{what}: {a:?} != {b:?}"
        );
    }

    fn viewport() -> Viewport {
        Viewport::new(Vec2::new(-120.0, 45.0), 1.75, Vec2::new(1440.0, 900.0))
    }

    #[test]
    fn the_identity_viewport_is_the_identity() {
        let v = Viewport::default();
        let p = Vec2::new(37.0, -12.5);

        assert_eq!(v.world_to_screen(p), p);
        assert_eq!(v.screen_to_world(p), p);
    }

    #[test]
    fn world_to_screen_is_the_documented_formula() {
        let v = viewport();

        assert_close(
            v.world_to_screen(Vec2::new(100.0, 200.0)),
            Vec2::new(100.0 * 1.75 - 120.0, 200.0 * 1.75 + 45.0),
            "world_to_screen",
        );
    }

    #[test]
    fn world_screen_round_trips_in_both_directions() {
        let v = viewport();

        for point in [
            Vec2::ZERO,
            Vec2::new(1.0, 1.0),
            Vec2::new(-9_000.0, 12_345.0),
            Vec2::new(0.125, -0.125),
        ] {
            assert_close(
                v.screen_to_world(v.world_to_screen(point)),
                point,
                "world -> screen -> world",
            );
            assert_close(
                v.world_to_screen(v.screen_to_world(point)),
                point,
                "screen -> world -> screen",
            );
        }
    }

    #[test]
    fn round_trips_hold_at_both_zoom_extremes() {
        let point = Vec2::new(4_321.0, -8_765.0);

        for zoom in [Viewport::MIN_ZOOM, Viewport::MAX_ZOOM] {
            let v = Viewport::new(Vec2::new(17.0, -3.0), zoom, Vec2::new(800.0, 600.0));
            let round_tripped = v.screen_to_world(v.world_to_screen(point));

            // The relative error is what is bounded here: at 40x, a world unit
            // is 40 screen pixels, and the reverse divide reintroduces `f32`
            // rounding proportional to the magnitude.
            let error = (round_tripped - point).length() / point.length();
            assert!(error < 1e-5, "zoom {zoom}: relative error {error}");
        }
    }

    #[test]
    fn rects_transform_position_and_scale_size() {
        let v = viewport();
        let world = Rect::new(Vec2::new(10.0, 20.0), Vec2::new(100.0, 50.0));

        let screen = v.world_rect_to_screen(world);
        assert_close(screen.origin, v.world_to_screen(world.origin), "origin");
        assert_close(screen.size, world.size * v.zoom(), "size");

        let back = v.screen_rect_to_world(screen);
        assert_close(back.origin, world.origin, "rect round-trip origin");
        assert_close(back.size, world.size, "rect round-trip size");
    }

    #[test]
    fn lengths_scale_with_zoom_only() {
        let v = viewport();

        assert!((v.world_to_screen_length(10.0) - 17.5).abs() < EPSILON);
        assert!((v.screen_to_world_length(17.5) - 10.0).abs() < EPSILON);
    }

    #[test]
    fn the_visible_world_rect_is_the_pane_pulled_back_into_the_world() {
        let v = viewport();
        let visible = v.visible_world_rect();

        assert_close(visible.origin, v.screen_to_world(Vec2::ZERO), "top-left");
        assert_close(
            visible.max(),
            v.screen_to_world(v.size()),
            "bottom-right corner",
        );
        assert!(visible.contains_point(v.screen_to_world(v.size() / 2.0)));
    }

    #[test]
    fn an_unmeasured_pane_sees_nothing_rather_than_everything() {
        let v = Viewport::default();

        assert!(v.visible_world_rect().is_empty());
    }

    #[test]
    fn panning_moves_the_world_with_the_pointer_and_leaves_the_zoom_alone() {
        let mut v = viewport();
        let before = v.world_to_screen(Vec2::new(5.0, 5.0));

        v.pan_by(Vec2::new(30.0, -10.0));

        assert_eq!(v.zoom(), 1.75);
        assert_close(
            v.world_to_screen(Vec2::new(5.0, 5.0)),
            before + Vec2::new(30.0, -10.0),
            "content follows the pan exactly",
        );
    }

    // --- cursor-anchored zoom -------------------------------------------------

    #[test]
    fn zoom_keeps_the_world_point_under_the_cursor() {
        let anchors = [
            Vec2::ZERO,
            Vec2::new(1.0, 1.0),
            Vec2::new(720.0, 450.0),
            Vec2::new(1440.0, 900.0),
            Vec2::new(37.5, 812.25),
        ];

        for anchor in anchors {
            for factor in [0.25_f32, 0.9, 1.0, 1.1, 4.0] {
                let mut v = viewport();
                let world_before = v.screen_to_world(anchor);

                v.zoom_by(anchor, factor);

                assert_close(
                    v.world_to_screen(world_before),
                    anchor,
                    &format!("anchor {anchor:?} factor {factor}"),
                );
            }
        }
    }

    #[test]
    fn the_anchor_survives_a_clamped_zoom_request() {
        let anchor = Vec2::new(400.0, 300.0);

        for absurd in [1e-9_f32, 1e9] {
            let mut v = viewport();
            let world_before = v.screen_to_world(anchor);

            v.zoom_around(anchor, absurd);

            assert!((Viewport::MIN_ZOOM..=Viewport::MAX_ZOOM).contains(&v.zoom()));
            assert_close(
                v.world_to_screen(world_before),
                anchor,
                "a clamped zoom must not drift the view",
            );
        }
    }

    #[test]
    fn a_step_in_and_a_step_out_return_to_the_start() {
        let anchor = Vec2::new(613.0, 227.0);
        let mut v = viewport();
        let (pan_before, zoom_before) = (v.pan(), v.zoom());

        v.zoom_steps(anchor, 1);
        v.zoom_steps(anchor, -1);

        assert!((v.zoom() - zoom_before).abs() < EPSILON);
        assert_close(v.pan(), pan_before, "pan returns");
    }

    #[test]
    fn repeated_zooming_at_one_anchor_does_not_drift() {
        let anchor = Vec2::new(200.0, 700.0);
        let mut v = viewport();
        let world_before = v.screen_to_world(anchor);

        for _ in 0..200 {
            v.zoom_steps(anchor, 1);
            v.zoom_steps(anchor, -1);
        }

        assert_close(
            v.world_to_screen(world_before),
            anchor,
            "400 anchored zooms must not accumulate error",
        );
    }

    #[test]
    fn zoom_around_the_pane_centre_is_the_keyboard_zoom() {
        let mut v = viewport();
        let centre = v.size() / 2.0;
        let world_before = v.screen_to_world(centre);

        v.zoom_by(centre, 2.0);

        assert!((v.zoom() - 3.5).abs() < EPSILON);
        assert_close(v.world_to_screen(world_before), centre, "centre held");
    }

    // --- fit ------------------------------------------------------------------

    #[test]
    fn zoom_to_fit_frames_the_content_and_centres_it() {
        let mut v = viewport();
        let content = Rect::new(Vec2::new(-500.0, -250.0), Vec2::new(1000.0, 500.0));

        v.zoom_to_fit(content, 20.0);

        // 1440 - 40 = 1400 across 1000 units; 900 - 40 = 860 across 500. The
        // tighter axis wins.
        assert!((v.zoom() - 1.4).abs() < EPSILON, "zoom {}", v.zoom());
        assert_close(
            v.world_to_screen(content.center()),
            v.size() / 2.0,
            "content centre lands at the pane centre",
        );

        let visible = v.visible_world_rect();
        assert!(visible.contains_rect(content), "all content is on screen");
    }

    #[test]
    fn zoom_to_fit_only_centres_when_the_content_has_no_area() {
        let mut v = viewport();
        let point = Rect::new(Vec2::new(80.0, 80.0), Vec2::ZERO);

        v.zoom_to_fit(point, 20.0);

        assert_eq!(v.zoom(), 1.75, "an unfittable rect leaves the zoom alone");
        assert_close(
            v.world_to_screen(Vec2::new(80.0, 80.0)),
            v.size() / 2.0,
            "and still goes and looks at it",
        );
    }

    #[test]
    fn zoom_to_fit_clamps_rather_than_exceeding_the_zoom_range() {
        let mut v = viewport();

        v.zoom_to_fit(Rect::new(Vec2::ZERO, Vec2::splat(1.0)), 0.0);
        assert_eq!(v.zoom(), Viewport::MAX_ZOOM);

        v.zoom_to_fit(Rect::new(Vec2::ZERO, Vec2::splat(1e7)), 0.0);
        assert_eq!(v.zoom(), Viewport::MIN_ZOOM);
    }

    #[test]
    fn construction_and_reset_keep_the_zoom_in_range() {
        assert_eq!(
            Viewport::new(Vec2::ZERO, 1e9, Vec2::ZERO).zoom(),
            Viewport::MAX_ZOOM
        );
        assert_eq!(
            Viewport::new(Vec2::ZERO, 0.0, Vec2::ZERO).zoom(),
            Viewport::MIN_ZOOM
        );

        let mut v = viewport();
        v.reset();
        assert_eq!(v.zoom(), 1.0);
        assert_eq!(v.pan(), Vec2::ZERO);
        assert_eq!(v.size(), Vec2::new(1440.0, 900.0), "resize is not a reset");
    }
}
