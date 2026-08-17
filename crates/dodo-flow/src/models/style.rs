//! Shared style structures, plus the one field measurement insisted on.
//!
//! # Flattening tolerance is a style/quality field, not a constant
//!
//! [`RenderQuality::flattening_tolerance`] is lyon's curve-flattening tolerance
//! in screen pixels. lyon's default is 0.1 px, and 0.5 px was measured as
//! **halving both tessellation cost and vertex count — a straight 2.1× increase
//! in everything the frame budget is spent on**. That makes it the single
//! largest lever in the renderer, and it belongs here rather than buried in the
//! geometry layer for two reasons:
//!
//! 1. **It is a trade the user makes**, exactly like a theme: smoother curves or
//!    a bigger scene. A constant chosen by the geometry code takes that away.
//! 2. **It is part of every geometry cache key.** Two tessellations of the same
//!    Bézier at two tolerances are different geometry, and a cache that ignores
//!    the tolerance serves the wrong one after a quality change.
//!    [`RenderQuality::cache_key`] is the hashable form, because `f32` is
//!    neither `Eq` nor `Hash` and the cache needs both.
//!
//! # Theme colours are not resolved here
//!
//! Every colour on an element is an `Option<Color>`, and `None` means *"ask the
//! theme"*. A document must not bake a theme's foreground colour into its
//! elements: switching dodo's theme would then have to rewrite every element, or
//! — far more likely — silently not apply, leaving a document permanently
//! painted in the theme it was drawn under. The resolution happens at the render
//! boundary in `views/`, which is the only layer that may name
//! `gpui_component::ActiveTheme`. This is also why [`Color`] is four `f32`s of
//! this crate's own rather than `gpui::Hsla`: `models/` names no UI framework.

use serde::{Deserialize, Serialize};

/// A straight RGBA colour, components in `0.0..=1.0`, **not** premultiplied.
///
/// Its own type rather than `gpui::Hsla` because `models/` names no UI
/// framework — see this module's doc. `views/` converts.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub const TRANSPARENT: Color = Color::rgba(0.0, 0.0, 0.0, 0.0);
    pub const BLACK: Color = Color::rgb(0.0, 0.0, 0.0);
    pub const WHITE: Color = Color::rgb(1.0, 1.0, 1.0);

    pub const fn rgb(r: f32, g: f32, b: f32) -> Color {
        Color { r, g, b, a: 1.0 }
    }

    pub const fn rgba(r: f32, g: f32, b: f32, a: f32) -> Color {
        Color { r, g, b, a }
    }

    /// From the 8-bit-per-channel form a colour picker or a pasted `#rrggbb`
    /// deals in.
    pub fn from_rgba8(r: u8, g: u8, b: u8, a: u8) -> Color {
        Color::rgba(
            r as f32 / 255.0,
            g as f32 / 255.0,
            b as f32 / 255.0,
            a as f32 / 255.0,
        )
    }

    /// The same colour at a different alpha.
    pub fn with_alpha(self, a: f32) -> Color {
        Color { a, ..self }
    }

    /// Fully transparent, and therefore not worth painting at all. The renderer
    /// checks this before building geometry, since an invisible stroke still
    /// costs its full vertex count.
    pub fn is_invisible(&self) -> bool {
        self.a <= 0.0
    }
}

/// A dash pattern, in screen pixels, as `PathBuilder::dash_array` takes it.
///
/// **Dashing is not a free style flag.** A dashed line was measured at ~14× the
/// CPU and ~63× the vertices of a solid one, so the renderer budgets dashed
/// edges as a distinct expensive kind rather than a variation on a solid one. Keeping the pattern in its own type — rather than
/// as an `Option<Vec<f32>>` on the stroke — is what lets
/// [`DashPattern::is_solid`] be the cheap question the budget asks.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct DashPattern(Vec<f32>);

impl DashPattern {
    pub const fn solid() -> DashPattern {
        DashPattern(Vec::new())
    }

    /// An odd-length array is doubled by `PathBuilder::dash_array` itself, so
    /// it is stored exactly as given.
    pub fn new(dashes: impl Into<Vec<f32>>) -> DashPattern {
        DashPattern(dashes.into())
    }

    pub fn is_solid(&self) -> bool {
        self.0.is_empty()
    }

    /// The first on/off pair, as the pair the renderer's primitive carries, or
    /// `None` for a solid stroke.
    ///
    /// Longer patterns are truncated, and that is a real limitation rather than
    /// an oversight: the render primitive is `Copy` so that a frame's paths do
    /// not allocate, and every pattern anyone has asked for — dashed, dotted,
    /// dash-dot at a push — fits in two numbers. See
    /// [`DashSpec`](crate::render::plan::DashSpec).
    pub fn spec(&self) -> Option<(f32, f32)> {
        match self.0.as_slice() {
            [] => None,
            [on] => Some((*on, *on)),
            [on, off, ..] => Some((*on, *off)),
        }
    }

    pub fn dashes(&self) -> &[f32] {
        &self.0
    }
}

/// How a stroke is drawn (§32).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StrokeStyle {
    /// `None` resolves to the theme's border colour at render time.
    pub color: Option<Color>,
    /// In **world** units. `views/` multiplies by the zoom; a stroke that must
    /// stay a constant screen thickness is a renderer concern rather than a
    /// second field here: a cached tessellation cannot be scaled for that case
    /// anyway, because scaling it scales the stroke width with it.
    pub width: f32,
    pub dash: DashPattern,
}

impl Default for StrokeStyle {
    fn default() -> StrokeStyle {
        StrokeStyle {
            color: None,
            width: 1.0,
            dash: DashPattern::solid(),
        }
    }
}

impl StrokeStyle {
    /// Nothing would be painted. Checked before geometry is built.
    pub fn is_invisible(&self) -> bool {
        self.width <= 0.0 || self.color.is_some_and(|c| c.is_invisible())
    }
}

/// Arrow markers on a linear element or an edge (§32, §8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum ArrowMarker {
    #[default]
    None,
    /// An open V.
    Arrow,
    /// A filled triangle.
    ArrowClosed,
    Dot,
    Diamond,
}

/// Font properties (§32). Sizes are in **world** units, like stroke widths.
///
/// **The renderer will not use this size directly.** `font_size` is part of
/// GPUI's shaped-line cache key, so a continuous zoom would re-shape every
/// label on every frame; the LOD ladder quantises to
/// the few discrete sizes named in
/// [`budgets::LodThresholds`](crate::budgets::LodThresholds) before shaping.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FontStyle {
    pub size: f32,
    /// `None` resolves to the theme's foreground colour at render time.
    pub color: Option<Color>,
    /// `None` resolves to the theme's UI font.
    pub family: Option<String>,
    pub bold: bool,
    pub italic: bool,
}

impl Default for FontStyle {
    fn default() -> FontStyle {
        FontStyle {
            size: 14.0,
            color: None,
            family: None,
            bold: false,
            italic: false,
        }
    }
}

/// Everything paintable about one element (§32).
///
/// One flat struct per element for now. §32 also says not to duplicate large
/// style structures across elements and to consider interning if profiling
/// shows benefit — that is a deliberate later step, and the seam for it is that
/// nothing outside this module constructs an [`ElementStyle`] by field access
/// in a hot path.
/// **`#[serde(default)]` is on the container, and [`Default`] is written by
/// hand rather than derived.** A derived `Default` would give `opacity` a
/// `0.0`, so every element in a document saved before the field existed would
/// load fully transparent — a migration bug that costs nothing to prevent and
/// is invisible until someone opens an old file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ElementStyle {
    pub stroke: StrokeStyle,
    /// `None` resolves to the theme's surface colour; [`Color::TRANSPARENT`] is
    /// an explicit "no fill", which is a different thing.
    pub fill: Option<Color>,
    /// Multiplies both stroke and fill alpha. `1.0` is opaque.
    pub opacity: f32,
    /// In world units. Ignored by kinds that are not rectangular.
    pub corner_radius: f32,
    pub font: FontStyle,
    pub start_marker: ArrowMarker,
    pub end_marker: ArrowMarker,
}

impl Default for ElementStyle {
    fn default() -> ElementStyle {
        ElementStyle {
            stroke: StrokeStyle::default(),
            fill: None,
            opacity: 1.0,
            corner_radius: 0.0,
            font: FontStyle::default(),
            start_marker: ArrowMarker::None,
            end_marker: ArrowMarker::None,
        }
    }
}

/// How an edge gets from its source to its target. The graph engine implements
/// the routing; the choice is document data because it is the author's, and it
/// survives a reload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum EdgeRouting {
    Straight,
    #[default]
    Bezier,
    /// A Bézier whose control points come from the endpoints alone, without
    /// consulting handle directions — React Flow's "simple bezier".
    SimpleBezier,
    /// Orthogonal legs with square corners.
    Step,
    /// Orthogonal legs with rounded corners.
    SmoothStep,
}

/// Clean or hand-drawn (§13). **A renderer strategy, not document geometry** —
/// switching it must not touch a single element, which is what lets sketch mode
/// be a second painter over the same canonical shapes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum RenderStyle {
    #[default]
    Clean,
    Sketch,
}

/// The quality/cost trade the whole render budget turns on. See this module's
/// doc for why the tolerance lives here.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct RenderQuality {
    /// lyon's curve-flattening tolerance, in screen pixels.
    pub flattening_tolerance: f32,
}

impl Default for RenderQuality {
    fn default() -> RenderQuality {
        RenderQuality::BALANCED
    }
}

impl RenderQuality {
    /// lyon's own default. Smoothest curves, and the most expensive: every
    /// number in [`crate::budgets`] was measured at this tolerance, so this is
    /// the setting those ceilings are stated against.
    pub const PRECISE: RenderQuality = RenderQuality {
        flattening_tolerance: 0.1,
    };

    /// The default. Measurably indistinguishable at 100 % zoom on a Retina
    /// display, and roughly 1.4× the scene for the same vertex count.
    pub const BALANCED: RenderQuality = RenderQuality {
        flattening_tolerance: 0.25,
    };

    /// The measured 2.1× budget multiplier: half the vertices and half the
    /// tessellation cost of [`RenderQuality::PRECISE`]. Visible on
    /// a tight corner at high zoom; the right setting for a large scene.
    pub const DRAFT: RenderQuality = RenderQuality {
        flattening_tolerance: 0.5,
    };

    /// Sane bounds. Below 0.01 px lyon produces vertices no display can
    /// resolve; above 2.0 px a curve is visibly a polygon.
    pub const MIN_TOLERANCE: f32 = 0.01;
    pub const MAX_TOLERANCE: f32 = 2.0;

    pub fn new(flattening_tolerance: f32) -> RenderQuality {
        RenderQuality {
            flattening_tolerance: flattening_tolerance
                .clamp(RenderQuality::MIN_TOLERANCE, RenderQuality::MAX_TOLERANCE),
        }
    }

    /// The hashable form, for a geometry cache key.
    ///
    /// `f32` is neither `Eq` nor `Hash`, and a cache keyed on the raw float
    /// would be wrong in both directions: `-0.0 != 0.0` as bits but equal as
    /// numbers, and a `NaN` never matches itself. Quantising to hundredths of a
    /// pixel — a hundredth is already an order of magnitude below the smallest
    /// useful tolerance — gives an integer key that is stable, comparable and
    /// coarse enough that a slider drag does not thrash the cache.
    pub fn cache_key(&self) -> u32 {
        (self.flattening_tolerance * 100.0).round().max(0.0) as u32
    }

    /// Roughly how the vertex count scales against
    /// [`RenderQuality::PRECISE`], which is what the budget numbers were
    /// measured at.
    ///
    /// Flattening a curve to a tolerance `t` needs O(1/√t) segments, so
    /// halving the vertex count means quadrupling the tolerance — which is
    /// exactly what was measured between 0.1 px and 0.5 px. This is the
    /// factor `budgets` applies when it converts a measured ceiling into a
    /// ceiling for the *current* quality, and it is an estimate rather than a
    /// measurement at any tolerance but those two.
    pub fn vertex_scale_vs_precise(&self) -> f32 {
        (RenderQuality::PRECISE.flattening_tolerance / self.flattening_tolerance).sqrt()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ArrowMarker, Color, DashPattern, EdgeRouting, ElementStyle, RenderQuality, RenderStyle,
        StrokeStyle,
    };

    #[test]
    fn colors_convert_from_the_eight_bit_form() {
        let c = Color::from_rgba8(255, 128, 0, 255);

        assert_eq!(c.r, 1.0);
        assert!((c.g - 0.502).abs() < 0.01);
        assert_eq!(c.b, 0.0);
        assert_eq!(c.a, 1.0);
        assert!(Color::TRANSPARENT.is_invisible());
        assert!(!Color::BLACK.is_invisible());
        assert!(Color::WHITE.with_alpha(0.0).is_invisible());
    }

    #[test]
    fn a_solid_dash_pattern_is_the_empty_one() {
        assert!(DashPattern::solid().is_solid());
        assert!(DashPattern::default().is_solid());
        assert!(!DashPattern::new([4.0, 2.0]).is_solid());
        assert_eq!(DashPattern::new([4.0, 2.0]).dashes(), &[4.0, 2.0]);
    }

    #[test]
    fn an_invisible_stroke_is_detected_before_geometry_is_built() {
        assert!(!StrokeStyle::default().is_invisible());
        assert!(
            StrokeStyle {
                width: 0.0,
                ..Default::default()
            }
            .is_invisible()
        );
        assert!(
            StrokeStyle {
                color: Some(Color::TRANSPARENT),
                ..Default::default()
            }
            .is_invisible()
        );
    }

    #[test]
    fn default_colours_are_unset_so_the_theme_can_answer() {
        let style = ElementStyle::default();

        assert_eq!(style.stroke.color, None);
        assert_eq!(style.fill, None);
        assert_eq!(style.font.color, None);
    }

    #[test]
    fn defaults_are_the_documented_ones() {
        assert_eq!(EdgeRouting::default(), EdgeRouting::Bezier);
        assert_eq!(RenderStyle::default(), RenderStyle::Clean);
        assert_eq!(ArrowMarker::default(), ArrowMarker::None);
        assert_eq!(RenderQuality::default(), RenderQuality::BALANCED);
    }

    #[test]
    fn tolerance_is_clamped_to_something_a_display_can_show() {
        assert_eq!(RenderQuality::new(0.0).flattening_tolerance, 0.01);
        assert_eq!(RenderQuality::new(100.0).flattening_tolerance, 2.0);
        assert_eq!(RenderQuality::new(0.3).flattening_tolerance, 0.3);
    }

    #[test]
    fn the_cache_key_separates_the_three_presets_and_is_stable() {
        let keys = [
            RenderQuality::PRECISE.cache_key(),
            RenderQuality::BALANCED.cache_key(),
            RenderQuality::DRAFT.cache_key(),
        ];

        assert_eq!(keys, [10, 25, 50]);
        assert_eq!(
            RenderQuality::PRECISE.cache_key(),
            RenderQuality::new(0.1).cache_key(),
            "the same tolerance is the same key"
        );
        assert_ne!(
            RenderQuality::PRECISE.cache_key(),
            RenderQuality::DRAFT.cache_key(),
            "two tolerances are two tessellations and must not share a cache entry"
        );
    }

    #[test]
    fn a_coarser_tolerance_buys_a_bigger_scene() {
        assert_eq!(RenderQuality::PRECISE.vertex_scale_vs_precise(), 1.0);

        let draft = RenderQuality::DRAFT.vertex_scale_vs_precise();
        assert!(
            (draft - 0.447).abs() < 0.01,
            "0.5px should cost about half the vertices of 0.1px, got {draft}"
        );
        assert!(RenderQuality::BALANCED.vertex_scale_vs_precise() < 1.0);
    }

    #[test]
    fn a_style_round_trips_through_json() {
        let style = ElementStyle {
            fill: Some(Color::from_rgba8(20, 30, 40, 255)),
            opacity: 0.5,
            corner_radius: 6.0,
            end_marker: ArrowMarker::ArrowClosed,
            ..Default::default()
        };

        let json = serde_json::to_string(&style).unwrap();
        assert_eq!(serde_json::from_str::<ElementStyle>(&json).unwrap(), style);
    }

    #[test]
    fn opacity_defaults_to_opaque_when_an_older_document_omits_it() {
        // `#[serde(default)]` on a float defaults to 0.0, which would make
        // every element from a document written without the field invisible.
        let style: ElementStyle = serde_json::from_str("{}").expect("all fields are defaulted");

        assert_eq!(style.opacity, 1.0);
    }
}
