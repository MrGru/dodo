//! The canvas background grid (§33) — **generated from the viewport, never
//! stored as document objects**, and painted as quads.
//!
//! Two decisions carry this module and both come from measurement.
//!
//! # It is quads, and it is bounded
//!
//! A grid is the one thing on the canvas whose primitive count is set by the
//! *view* rather than by the document, so it is the one thing that can be
//! unbounded for free. Zoom out far enough and a fixed world spacing asks for a
//! line every thousandth of a pixel; at 20,000 quads the frame is already at
//! Phase 0's measured 60 fps limit, and the grid would have spent all of it
//! before a single node was drawn.
//!
//! So the spacing **adapts**: the module picks the coarsest level whose
//! on-screen spacing is still at least [`GridLimits::min_line_spacing_px`] (or
//! [`GridLimits::min_dot_spacing_px`]), stepping by
//! [`GridSettings::major_every`] so that zooming out promotes today's major
//! lines into tomorrow's minor ones and the grid appears to breathe rather than
//! to snap. That alone bounds the count to roughly `pane / min_spacing`. A
//! second pass then steps the level up again if the count still exceeds
//! [`GridLimits::max_quads`], which is what makes the bound a *guarantee*
//! rather than an argument — dots are the case that needs it, because their
//! count is the product of two axes and not the sum.
//!
//! # It never becomes a path
//!
//! Every primitive here is a [`QuadPrimitive`]: a grid line is a one-pixel-wide
//! rectangle spanning the pane, a dot is a quad with a corner radius of half
//! its size, and a cross is two short bars. Phase 0 measured 20,000 quads at 60
//! fps against the same count of filled rectangular paths at 30, and — the
//! larger reason — a run of paths costs a full-viewport render pass, so a grid
//! made of paths would put a batch boundary underneath every frame.
//!
//! **This file names no UI framework.** The colours arrive as
//! [`crate::models::Color`], resolved from the theme by `views/`, which is what
//! makes the grid theme-aware without this module knowing a theme exists.

use crate::{
    budgets::RenderBudgets,
    geometry::{Rect, Vec2, Viewport},
    models::Color,
    render::plan::{PaintPlan, QuadPrimitive},
};

/// How the background is drawn. §33's list.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum GridStyle {
    /// No background pattern at all — still the pane's background colour.
    None,
    #[default]
    Dots,
    /// Full lines across the pane in both axes.
    Lines,
    /// A short plus at each intersection: the lines' legibility without their
    /// visual weight, which is what a dense diagram wants.
    Cross,
}

/// The colour and weight of one tier of the grid.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GridInk {
    pub color: Color,
    /// Screen pixels — a grid line is the same weight at every zoom, because it
    /// is a property of the display and not of the world.
    pub thickness: f32,
}

impl GridInk {
    pub fn new(color: Color, thickness: f32) -> GridInk {
        GridInk { color, thickness }
    }
}

/// The grid's configuration. View state, not document data: two people looking
/// at the same document may reasonably want different backgrounds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GridSettings {
    pub style: GridStyle,
    /// The base spacing in **world** units.
    pub spacing: f32,
    /// Every nth line is a major one. Also the factor the adaptive level steps
    /// by, so that a major line stays a line when the level changes.
    pub major_every: u32,
    pub minor: GridInk,
    pub major: GridInk,
}

impl GridSettings {
    /// The smallest sensible step. A `major_every` of 1 would make every line
    /// major and the level search would never coarsen, so it is a floor rather
    /// than a preference.
    pub const MIN_MAJOR_EVERY: u32 = 2;

    /// The smallest sensible world spacing, in world units. Below this the
    /// level search does all its work at the deepest level and the grid is
    /// meaningless anyway.
    pub const MIN_SPACING: f32 = 0.5;

    pub fn step(&self) -> u32 {
        self.major_every.max(GridSettings::MIN_MAJOR_EVERY)
    }

    pub fn base_spacing(&self) -> f32 {
        self.spacing.max(GridSettings::MIN_SPACING)
    }
}

impl Default for GridSettings {
    /// A 20-unit grid with every fifth line major: the spacing React Flow and
    /// Excalidraw both settle on, and one that divides the 150×40 default node
    /// size evenly so snapping (§27) lands on node edges.
    ///
    /// The colours are placeholders that `views/` overwrites from the theme
    /// every frame — they exist so this type has a `Default` at all, and a
    /// visible mid-grey is a better failure than an invisible one.
    fn default() -> GridSettings {
        GridSettings {
            style: GridStyle::Dots,
            spacing: 20.0,
            major_every: 5,
            minor: GridInk::new(Color::rgba(0.5, 0.5, 0.5, 0.25), 1.0),
            major: GridInk::new(Color::rgba(0.5, 0.5, 0.5, 0.45), 1.0),
        }
    }
}

/// The bounds that keep the grid's cost fixed whatever the zoom.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GridLimits {
    /// Below this on-screen gap, lines merge into a wash and the level
    /// coarsens. 8 px is about where a 1 px line at 25 % opacity stops reading
    /// as a grid and starts reading as a tint.
    pub min_line_spacing_px: f32,
    /// Dots need a wider gap than lines, and not for legibility: their count is
    /// the *product* of the two axis counts, so a 1440×900 pane at an 8 px
    /// spacing would be 20,000 dots — the entire measured quad budget, spent
    /// before anything in the document is drawn.
    pub min_dot_spacing_px: f32,
    /// The hard cap on primitives the grid may contribute.
    pub max_quads: u32,
}

impl GridLimits {
    /// The most levels the search will climb.
    ///
    /// A termination guard rather than a limit anyone reaches: at the default
    /// step of 5 this is a spacing multiplier of 5^16, far past what
    /// [`Viewport::MIN_ZOOM`] can ask for. It exists because the search is a
    /// loop over a float comparison and a NaN spacing would otherwise not
    /// terminate.
    pub const MAX_LEVELS: u32 = 16;

    /// Limits derived from the platform's measured budgets.
    ///
    /// The grid is allowed a quarter of the frame's quads. It is background:
    /// node bodies, handles and selection all draw from the same budget and all
    /// of them matter more, and Phase 4's culling has to have room to work in.
    pub fn from_budgets(budgets: &RenderBudgets) -> GridLimits {
        GridLimits {
            min_line_spacing_px: 8.0,
            min_dot_spacing_px: 16.0,
            max_quads: budgets.target_quads_per_frame / 4,
        }
    }
}

/// What the generator chose, and what it cost. Returned so a caller — and the
/// tests — can see the adaptation rather than infer it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GridLevel {
    /// How many times the base spacing was multiplied by
    /// [`GridSettings::step`]. Zero means the document's own spacing is on
    /// screen.
    pub level: u32,
    /// The spacing actually drawn, in world units.
    pub world_spacing: f32,
    /// The same spacing in screen pixels — always at or above the style's
    /// minimum, unless the pane is degenerate.
    pub screen_spacing: f32,
    /// Quads pushed into the plan.
    pub quads: u32,
    /// Set when the level had to coarsen past what legibility alone asked for,
    /// because [`GridLimits::max_quads`] would otherwise have been exceeded.
    pub clamped_by_budget: bool,
}

impl Default for GridLevel {
    /// The same as [`GridLevel::empty`]: a frame that has not drawn a grid yet
    /// has drawn no grid, and any other default would be a spacing nobody
    /// chose.
    fn default() -> GridLevel {
        GridLevel::empty()
    }
}

impl GridLevel {
    /// The grid drew nothing: [`GridStyle::None`], or a pane with no area.
    pub fn empty() -> GridLevel {
        GridLevel {
            level: 0,
            world_spacing: 0.0,
            screen_spacing: 0.0,
            quads: 0,
            clamped_by_budget: false,
        }
    }
}

/// Indices of the grid lines crossing one axis of the pane.
///
/// Half-open in neither direction: `first` and `last` are both drawn, so a line
/// exactly on the pane's edge is included. The count is what the budget check
/// is made against, and it is computed before a single quad is built.
#[derive(Debug, Clone, Copy, PartialEq)]
struct AxisRange {
    first: i64,
    last: i64,
}

impl AxisRange {
    fn count(&self) -> u32 {
        (self.last - self.first + 1).max(0).min(u32::MAX as i64) as u32
    }
}

/// The grid indices visible along one axis at `spacing`.
fn axis_range(min_world: f32, max_world: f32, spacing: f32) -> AxisRange {
    if !spacing.is_finite() || spacing <= 0.0 || !min_world.is_finite() || !max_world.is_finite() {
        return AxisRange { first: 0, last: -1 };
    }

    AxisRange {
        first: (min_world / spacing).ceil() as i64,
        last: (max_world / spacing).floor() as i64,
    }
}

/// Quads one style spends on a given pair of axis counts.
fn quads_for(style: GridStyle, x: u32, y: u32) -> u32 {
    match style {
        GridStyle::None => 0,
        GridStyle::Lines => x.saturating_add(y),
        GridStyle::Dots => x.saturating_mul(y),
        // A plus is two bars.
        GridStyle::Cross => x.saturating_mul(y).saturating_mul(2),
    }
}

/// **Generates the grid into `plan`, and returns what it chose.**
///
/// The only entry point. Pushes quads and nothing else, so it can never open a
/// path batch under the rest of the frame.
pub fn generate(
    settings: &GridSettings,
    viewport: &Viewport,
    limits: &GridLimits,
    plan: &mut PaintPlan,
) -> GridLevel {
    let pane = viewport.size();
    if settings.style == GridStyle::None
        || !pane.is_finite()
        || pane.x <= 0.0
        || pane.y <= 0.0
        || !viewport.zoom().is_finite()
        || viewport.zoom() <= 0.0
    {
        return GridLevel::empty();
    }

    let world = viewport.visible_world_rect().normalized();
    let step = settings.step() as f32;
    let base = settings.base_spacing();
    let min_screen_spacing = match settings.style {
        GridStyle::Dots | GridStyle::Cross => limits.min_dot_spacing_px,
        _ => limits.min_line_spacing_px,
    };

    let mut level = 0u32;
    let mut clamped_by_budget = false;

    let (world_spacing, x_range, y_range) = loop {
        let world_spacing = base * step.powi(level as i32);
        let screen_spacing = viewport.world_to_screen_length(world_spacing);

        let x_range = axis_range(world.min().x, world.max().x, world_spacing);
        let y_range = axis_range(world.min().y, world.max().y, world_spacing);
        let quads = quads_for(settings.style, x_range.count(), y_range.count());

        let too_dense = screen_spacing < min_screen_spacing;
        let too_many = quads > limits.max_quads;

        // Recorded whenever the count — rather than the eye — is what is
        // still unsatisfied, so a caller can tell a grid that coarsened for
        // legibility from one the budget had to hold back.
        clamped_by_budget |= !too_dense && too_many;

        if (!too_dense && !too_many) || level >= GridLimits::MAX_LEVELS {
            break (world_spacing, x_range, y_range);
        }

        level += 1;
    };

    let screen_spacing = viewport.world_to_screen_length(world_spacing);
    let before = plan.quad_count();

    // A last, unconditional guard. The level search gives up at MAX_LEVELS, and
    // a pathological spacing or zoom could in principle still leave the count
    // over budget; emitting is where that must not be allowed to matter,
    // because past the cap the grid is a wash of colour and costs the frame.
    if quads_for(settings.style, x_range.count(), y_range.count()) > limits.max_quads {
        return GridLevel {
            level,
            world_spacing,
            screen_spacing,
            quads: 0,
            clamped_by_budget: true,
        };
    }

    let major_every = settings.step() as i64;
    let is_major = |index: i64| index.rem_euclid(major_every) == 0;

    match settings.style {
        GridStyle::None => {}
        GridStyle::Lines => {
            for index in x_range.first..=x_range.last {
                let ink = if is_major(index) {
                    settings.major
                } else {
                    settings.minor
                };
                let x = viewport
                    .world_to_screen(Vec2::new(index as f32 * world_spacing, 0.0))
                    .x;
                plan.push_quad(QuadPrimitive::filled(
                    Rect::new(
                        Vec2::new(x - ink.thickness * 0.5, 0.0),
                        Vec2::new(ink.thickness, pane.y),
                    ),
                    ink.color,
                ));
            }

            for index in y_range.first..=y_range.last {
                let ink = if is_major(index) {
                    settings.major
                } else {
                    settings.minor
                };
                let y = viewport
                    .world_to_screen(Vec2::new(0.0, index as f32 * world_spacing))
                    .y;
                plan.push_quad(QuadPrimitive::filled(
                    Rect::new(
                        Vec2::new(0.0, y - ink.thickness * 0.5),
                        Vec2::new(pane.x, ink.thickness),
                    ),
                    ink.color,
                ));
            }
        }
        GridStyle::Dots => {
            for ix in x_range.first..=x_range.last {
                for iy in y_range.first..=y_range.last {
                    let ink = if is_major(ix) && is_major(iy) {
                        settings.major
                    } else {
                        settings.minor
                    };
                    let center = viewport.world_to_screen(Vec2::new(
                        ix as f32 * world_spacing,
                        iy as f32 * world_spacing,
                    ));
                    // A quad whose corner radius is half its size is a circle,
                    // which is why a dot costs no more than a square dot does.
                    let radius = ink.thickness.max(0.5);
                    plan.push_quad(
                        QuadPrimitive::filled(
                            Rect::new(center - Vec2::splat(radius), Vec2::splat(radius * 2.0)),
                            ink.color,
                        )
                        .with_corner_radius(radius),
                    );
                }
            }
        }
        GridStyle::Cross => {
            for ix in x_range.first..=x_range.last {
                for iy in y_range.first..=y_range.last {
                    let ink = if is_major(ix) && is_major(iy) {
                        settings.major
                    } else {
                        settings.minor
                    };
                    let center = viewport.world_to_screen(Vec2::new(
                        ix as f32 * world_spacing,
                        iy as f32 * world_spacing,
                    ));
                    let arm = (screen_spacing * 0.15).clamp(2.0, 6.0);
                    let half = ink.thickness * 0.5;

                    plan.push_quad(QuadPrimitive::filled(
                        Rect::new(
                            Vec2::new(center.x - arm, center.y - half),
                            Vec2::new(arm * 2.0, ink.thickness),
                        ),
                        ink.color,
                    ));
                    plan.push_quad(QuadPrimitive::filled(
                        Rect::new(
                            Vec2::new(center.x - half, center.y - arm),
                            Vec2::new(ink.thickness, arm * 2.0),
                        ),
                        ink.color,
                    ));
                }
            }
        }
    }

    GridLevel {
        level,
        world_spacing,
        screen_spacing,
        quads: plan.quad_count() - before,
        clamped_by_budget,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::budgets::{RenderBackend, for_backend};

    const PANE: Vec2 = Vec2::new(1440.0, 900.0);

    fn limits() -> GridLimits {
        GridLimits::from_budgets(&for_backend(RenderBackend::Metal))
    }

    fn viewport_at(zoom: f32) -> Viewport {
        Viewport::new(Vec2::ZERO, zoom, PANE)
    }

    fn settings(style: GridStyle) -> GridSettings {
        GridSettings {
            style,
            ..GridSettings::default()
        }
    }

    fn run(style: GridStyle, zoom: f32) -> (GridLevel, PaintPlan) {
        let mut plan = PaintPlan::new();
        let level = generate(&settings(style), &viewport_at(zoom), &limits(), &mut plan);
        (level, plan)
    }

    /// **The bound the whole module exists for**, swept across the viewport's
    /// entire legal zoom range for every style. The grid must never be able to
    /// spend the frame.
    #[test]
    fn output_is_bounded_at_every_zoom_and_every_style() {
        let limits = limits();
        let mut zoom = Viewport::MIN_ZOOM;

        while zoom <= Viewport::MAX_ZOOM {
            for style in [
                GridStyle::None,
                GridStyle::Dots,
                GridStyle::Lines,
                GridStyle::Cross,
            ] {
                let (level, plan) = run(style, zoom);

                assert_eq!(level.quads, plan.quad_count());
                assert!(
                    level.quads <= limits.max_quads,
                    "{style:?} at zoom {zoom} produced {} quads, over the {} cap",
                    level.quads,
                    limits.max_quads
                );
                assert_eq!(plan.path_count(), 0, "the grid never opens a path batch");
                assert_eq!(plan.text_count(), 0);
            }
            zoom *= 1.35;
        }
    }

    /// Zooming out coarsens the grid rather than densifying it. This is the
    /// property that keeps the count bounded, stated as behaviour rather than
    /// as a cap.
    #[test]
    fn zooming_out_raises_the_level_and_never_lowers_it() {
        let mut previous = 0u32;
        let mut zoom = Viewport::MAX_ZOOM;

        while zoom >= Viewport::MIN_ZOOM {
            let (level, _) = run(GridStyle::Lines, zoom);
            assert!(
                level.level >= previous,
                "level fell from {previous} to {} on the way out to zoom {zoom}",
                level.level
            );
            previous = level.level;
            zoom /= 1.5;
        }

        assert!(previous > 0, "some coarsening has to have happened");
    }

    /// The legibility floor, which is the first of the two bounds.
    #[test]
    fn the_drawn_spacing_never_falls_below_the_style_minimum() {
        let limits = limits();
        let mut zoom = Viewport::MIN_ZOOM;

        while zoom <= Viewport::MAX_ZOOM {
            let (lines, _) = run(GridStyle::Lines, zoom);
            let (dots, _) = run(GridStyle::Dots, zoom);

            assert!(
                lines.screen_spacing >= limits.min_line_spacing_px,
                "lines at zoom {zoom} were {} px apart",
                lines.screen_spacing
            );
            assert!(
                dots.screen_spacing >= limits.min_dot_spacing_px,
                "dots at zoom {zoom} were {} px apart",
                dots.screen_spacing
            );
            zoom *= 1.35;
        }
    }

    /// Dots coarsen sooner than lines at the same zoom, because their count is
    /// a product rather than a sum. If this ever inverts, the dot grid is one
    /// zoom-out from eating the quad budget.
    #[test]
    fn dots_step_up_before_lines_do() {
        let (lines, _) = run(GridStyle::Lines, 0.35);
        let (dots, _) = run(GridStyle::Dots, 0.35);

        assert!(
            dots.level >= lines.level,
            "dots {} vs lines {}",
            dots.level,
            lines.level
        );
    }

    #[test]
    fn a_none_grid_draws_nothing() {
        let (level, plan) = run(GridStyle::None, 1.0);
        assert_eq!(level, GridLevel::empty());
        assert!(plan.is_empty());
    }

    /// The viewport is zero-sized until the first frame measures it, which is a
    /// real state and not a bug.
    #[test]
    fn an_unmeasured_pane_draws_nothing() {
        let mut plan = PaintPlan::new();
        let level = generate(
            &settings(GridStyle::Dots),
            &Viewport::default(),
            &limits(),
            &mut plan,
        );

        assert_eq!(level.quads, 0);
        assert!(plan.is_empty());
    }

    /// At 1:1 with the default 20-unit spacing the grid is exactly the
    /// document's own: no adaptation, and the arithmetic is checkable by hand —
    /// 1440/20 + 1 vertical lines and 900/20 + 1 horizontal.
    #[test]
    fn at_one_to_one_the_grid_is_the_documents_own_spacing() {
        let (level, plan) = run(GridStyle::Lines, 1.0);

        assert_eq!(level.level, 0);
        assert_eq!(level.world_spacing, 20.0);
        assert_eq!(level.screen_spacing, 20.0);
        assert_eq!(plan.quad_count(), (1440 / 20 + 1) + (900 / 20 + 1));
    }

    /// Panning must not change how much the grid costs — only which lines it
    /// draws. A pan that grew the count would make dragging progressively
    /// slower the further you went.
    ///
    /// The bound is exact rather than approximate. A pan slides the pane across
    /// the lattice, so each axis gains or loses at most one index; for lines
    /// that is a difference of two, and for dots — whose count is the *product*
    /// of the two axes — it is `x + y + 1`. Asserting the loose version of this
    /// would let a real regression through, because for a dot grid the honest
    /// bound is in the thousands.
    #[test]
    fn panning_does_not_change_the_cost() {
        for style in [GridStyle::Lines, GridStyle::Dots, GridStyle::Cross] {
            let (baseline, _) = run(style, 1.0);
            let world = viewport_at(1.0).visible_world_rect();
            let x = (world.width() / baseline.world_spacing).ceil() as u32 + 1;
            let y = (world.height() / baseline.world_spacing).ceil() as u32 + 1;
            let tolerance = match style {
                GridStyle::Lines => 2,
                GridStyle::Dots => x + y + 1,
                GridStyle::Cross => (x + y + 1) * 2,
                GridStyle::None => 0,
            };

            for offset in [7.0, 123.5, -1_000.0, 99_999.0] {
                let mut viewport = viewport_at(1.0);
                viewport.pan_by(Vec2::splat(offset));
                let mut plan = PaintPlan::new();
                let level = generate(&settings(style), &viewport, &limits(), &mut plan);

                assert_eq!(
                    level.level, baseline.level,
                    "{style:?} changed level on a pure pan"
                );
                assert!(
                    level.quads.abs_diff(baseline.quads) <= tolerance,
                    "{style:?} pan by {offset} moved the count from {} to {} \
                     (tolerance {tolerance})",
                    baseline.quads,
                    level.quads
                );
            }
        }
    }

    /// Major lines exist and are a minority of the whole — the visual structure
    /// §33 asks for, asserted rather than eyeballed.
    #[test]
    fn every_nth_line_is_major() {
        let settings = settings(GridStyle::Lines);
        let mut plan = PaintPlan::new();
        generate(&settings, &viewport_at(1.0), &limits(), &mut plan);

        let mut major = 0;
        let mut minor = 0;
        let mut recording = CountingSink {
            major_color: settings.major.color,
            minor_color: settings.minor.color,
            major: &mut major,
            minor: &mut minor,
        };
        plan.paint_into(&mut recording);

        assert!(major > 0, "no major lines at all");
        assert!(minor > major, "major lines should be the minority");
        // Every fifth index, in both axes.
        assert_eq!(major + minor, plan.quad_count());
    }

    /// A grid whose spacing is finer than the level search can ever fix is
    /// still not allowed to draw. The guard after the search is what this
    /// covers, and it is the difference between a slow frame and a bounded one.
    #[test]
    fn an_absurdly_fine_grid_gives_up_rather_than_flooding_the_frame() {
        let settings = GridSettings {
            style: GridStyle::Dots,
            spacing: GridSettings::MIN_SPACING,
            major_every: GridSettings::MIN_MAJOR_EVERY,
            ..GridSettings::default()
        };
        let limits = GridLimits {
            min_line_spacing_px: 0.0,
            min_dot_spacing_px: 0.0,
            max_quads: 32,
        };

        let mut plan = PaintPlan::new();
        let level = generate(&settings, &viewport_at(1.0), &limits, &mut plan);

        assert!(level.clamped_by_budget);
        assert!(plan.quad_count() <= 32);
    }

    #[test]
    fn a_major_every_of_one_is_treated_as_the_floor_rather_than_looping_forever() {
        let settings = GridSettings {
            style: GridStyle::Lines,
            major_every: 1,
            ..GridSettings::default()
        };
        let mut plan = PaintPlan::new();
        let level = generate(&settings, &viewport_at(0.05), &limits(), &mut plan);

        assert_eq!(settings.step(), GridSettings::MIN_MAJOR_EVERY);
        assert!(level.quads > 0);
        assert!(level.screen_spacing >= limits().min_line_spacing_px);
    }

    /// Counts quads by which ink they were drawn in.
    struct CountingSink<'a> {
        major_color: Color,
        minor_color: Color,
        major: &'a mut u32,
        minor: &'a mut u32,
    }

    impl crate::render::plan::PrimitiveSink for CountingSink<'_> {
        fn quad(&mut self, quad: &QuadPrimitive) {
            if quad.background == self.major_color {
                *self.major += 1;
            } else if quad.background == self.minor_color {
                *self.minor += 1;
            }
        }

        fn path(&mut self, _path: &crate::render::plan::PathPrimitive) -> u32 {
            unreachable!("the grid never pushes a path")
        }

        fn text(&mut self, _text: &crate::render::plan::TextPrimitive) -> u32 {
            unreachable!("the grid never pushes text")
        }
    }
}
