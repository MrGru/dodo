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

use dodo_paths::HostOs;
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

/// **How a closed shape's interior is filled** (§32).
///
/// Three answers rather than a boolean, because each is a genuinely different
/// picture rather than a degree of one: a solid fill is one quad or one
/// tessellated body, and the other two are *line sets* clipped to the shape —
/// see [`hatch`](mod@crate::render::hatch).
///
/// **[`Solid`](FillStyle::Solid) is the default, and that is a compatibility
/// decision rather than a taste one.** Excalidraw defaults to hachure; every
/// document dodo has written so far was drawn solid, and a default that changed
/// how existing files look is the migration bug `ElementStyle`'s hand-written
/// [`Default`] already exists to avoid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum FillStyle {
    /// Parallel strokes at a fixed angle.
    Hachure,
    /// Two hachure sets at right angles.
    CrossHatch,
    #[default]
    Solid,
}

impl FillStyle {
    pub const ALL: &'static [FillStyle] =
        &[FillStyle::Hachure, FillStyle::CrossHatch, FillStyle::Solid];

    /// Whether this fill is drawn as a line set rather than as a filled body.
    /// The one question the painter asks, so a fourth pattern is an arm here
    /// rather than a comparison at three paint sites.
    pub const fn is_hatched(self) -> bool {
        matches!(self, FillStyle::Hachure | FillStyle::CrossHatch)
    }

    /// A short, stable name. **Not user-facing** — the panel's labels are
    /// `dodo_i18n::flow`'s.
    pub const fn name(self) -> &'static str {
        match self {
            FillStyle::Hachure => "hachure",
            FillStyle::CrossHatch => "cross-hatch",
            FillStyle::Solid => "solid",
        }
    }
}

/// **How wobbly one element is drawn by §13's hand.**
///
/// Three steps, not a number, for the same reason [`FontSize`] has four: the
/// property panel's control is three buttons, and a discrete field means there
/// is no second answer to "how rough is this shape?".
///
/// It multiplies [`SketchStyle::roughness`], which is the *document's* hand —
/// so the document still decides what a hand looks like and each element says
/// how hard it presses. [`Artist`](Sloppiness::Artist) is `1.0` and therefore
/// the identity, which is what makes this field free to add to a format that
/// already has documents in it.
///
/// **It means nothing in [`RenderStyle::Clean`]**, and the panel says so rather
/// than storing a choice nobody can see — see `crate::properties`.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default, Serialize, Deserialize,
)]
pub enum Sloppiness {
    /// A steady hand: half the document's roughness.
    Architect,
    /// The document's hand, unmodified.
    #[default]
    Artist,
    /// Twice the document's roughness.
    Cartoonist,
}

impl Sloppiness {
    pub const ALL: &'static [Sloppiness] = &[
        Sloppiness::Architect,
        Sloppiness::Artist,
        Sloppiness::Cartoonist,
    ];

    /// What this step multiplies [`SketchStyle::roughness`] by.
    pub const fn roughness_scale(self) -> f32 {
        match self {
            Sloppiness::Architect => 0.5,
            Sloppiness::Artist => 1.0,
            Sloppiness::Cartoonist => 2.0,
        }
    }

    /// A short, stable name. **Not user-facing.**
    pub const fn name(self) -> &'static str {
        match self {
            Sloppiness::Architect => "architect",
            Sloppiness::Artist => "artist",
            Sloppiness::Cartoonist => "cartoonist",
        }
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

/// **Four discrete text sizes, and the reason there are four** (§9, §32).
///
/// Not a number. The property panel offers exactly four steps — S / M / L / XL
/// — and that is a gift rather than a constraint: Phase 5 found
/// that `font_size` is part of GPUI's own shaped-line cache key, so a
/// continuously-sized label is re-shaped on **every frame of a zoom** — 7–11 µs
/// each against 1.7 µs to paint a cached one. A continuous size field would
/// have had to be quantised somewhere anyway; making the *document* discrete
/// means there is no second, disagreeing answer to "what size is this text?".
///
/// The world sizes below are rungs of
/// [`LodThresholds::font_size_ladder`](crate::budgets::LodThresholds::font_size_ladder),
/// so at 100 % zoom a label is shaped at exactly the size it was authored at
/// and the quantiser is the identity. `the_four_steps_are_rungs_of_the_ladder`
/// in [`crate::budgets`] pins that, because the two live in different modules
/// and nothing else would notice them drifting apart.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Default, Serialize, Deserialize,
)]
pub enum FontSize {
    Small,
    #[default]
    Medium,
    Large,
    ExtraLarge,
}

impl FontSize {
    /// The four, in panel order.
    pub const ALL: &'static [FontSize] = &[
        FontSize::Small,
        FontSize::Medium,
        FontSize::Large,
        FontSize::ExtraLarge,
    ];

    /// The size in **world** units, like a stroke width — so text zooms with
    /// the document rather than staying a constant screen size.
    pub const fn world_size(self) -> f32 {
        match self {
            FontSize::Small => 12.0,
            FontSize::Medium => 16.0,
            FontSize::Large => 20.0,
            FontSize::ExtraLarge => 28.0,
        }
    }

    /// The step nearest a world size, for a migration or an import that has a
    /// number rather than a step. Ties go to the smaller step, which is the
    /// direction that never makes an old document's text overflow its box.
    pub fn nearest(world_size: f32) -> FontSize {
        let mut best = FontSize::Small;
        let mut best_gap = f32::INFINITY;
        for &step in FontSize::ALL {
            let gap = (step.world_size() - world_size).abs();
            if gap < best_gap {
                best_gap = gap;
                best = step;
            }
        }
        best
    }

    /// A short stable name, for an element id or a test. **Not user-facing** —
    /// the panel's labels are `dodo_i18n::flow`'s.
    pub const fn name(self) -> &'static str {
        match self {
            FontSize::Small => "s",
            FontSize::Medium => "m",
            FontSize::Large => "l",
            FontSize::ExtraLarge => "xl",
        }
    }
}

/// The three type families the property panel offers (§9, §32).
///
/// An enum rather than the font *name* it resolves to, for the same reason
/// every colour here is an `Option<Color>`: a document must not bake this
/// build's font stack into its elements. `views/` resolves a family against the
/// theme, which is the only layer that knows what is installed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum FontFamily {
    /// The hand-drawn face, which is what §13's sketch mode is drawn beside.
    HandDrawn,
    /// The theme's UI font.
    #[default]
    Normal,
    /// The theme's monospace font.
    Code,
}

impl FontFamily {
    pub const ALL: &'static [FontFamily] =
        &[FontFamily::HandDrawn, FontFamily::Normal, FontFamily::Code];

    /// **The faces this family prefers, best first**, for the platform it is
    /// being drawn on. Empty means *"the theme's own font"* — which is what
    /// [`Normal`](FontFamily::Normal) means and what
    /// [`Code`](FontFamily::Code) resolves to through the theme's monospace
    /// setting rather than through a name.
    ///
    /// A total function of a [`HostOs`] rather than an item behind `#[cfg]`,
    /// which is the root `AGENTS.md` invariant: two of dodo's four release
    /// targets cannot be built from a Mac, so every platform's answer has to be
    /// assertable from any machine. `views/` picks the first name the text
    /// system actually has and falls back to the theme's font, so a machine
    /// with none of them draws in the UI font rather than in something
    /// arbitrary.
    ///
    /// **dodo ships no hand-drawn face of its own**, deliberately: a bundled
    /// font is a licence, a build step and about half a megabyte in every
    /// release artefact, and §13's sketch mode is about the *geometry* rather
    /// than about the type. So this is a preference over what the platform
    /// already has, and the honest consequence is recorded in
    /// [`crate::render::painter`]: on a machine with none of these installed,
    /// picking Hand-drawn changes nothing on screen.
    pub const fn preferred_faces(self, host: HostOs) -> &'static [&'static str] {
        match self {
            FontFamily::Normal | FontFamily::Code => &[],
            FontFamily::HandDrawn => match host {
                HostOs::MacOs => &["Bradley Hand", "Chalkboard", "Marker Felt", "Comic Sans MS"],
                HostOs::Windows => &["Segoe Print", "Ink Free", "Comic Sans MS"],
                HostOs::Unix => &["Comic Neue", "Comic Relief", "Purisa", "URW Chancery L"],
            },
        }
    }

    /// A short stable name, for an element id or a test. **Not user-facing.**
    pub const fn name(self) -> &'static str {
        match self {
            FontFamily::HandDrawn => "hand-drawn",
            FontFamily::Normal => "normal",
            FontFamily::Code => "code",
        }
    }
}

/// Horizontal alignment within the element's box (§9, §32).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
}

impl TextAlign {
    pub const ALL: &'static [TextAlign] = &[TextAlign::Left, TextAlign::Center, TextAlign::Right];

    /// Where a run of `width` starts inside a box of `available`, measured from
    /// the box's left edge.
    ///
    /// Pure arithmetic here rather than a flag handed to the text system,
    /// because the painter needs the origin anyway — `ShapedLine::paint` takes
    /// one — and because an alignment that is a number can be asserted with no
    /// window. Never negative: a run wider than its box starts at the left edge
    /// and is truncated, rather than being centred into its own neighbours.
    pub fn offset(self, available: f32, width: f32) -> f32 {
        let slack = (available - width).max(0.0);
        match self {
            TextAlign::Left => 0.0,
            TextAlign::Center => slack * 0.5,
            TextAlign::Right => slack,
        }
    }

    /// A short stable name, for an element id or a test. **Not user-facing.**
    pub const fn name(self) -> &'static str {
        match self {
            TextAlign::Left => "left",
            TextAlign::Center => "center",
            TextAlign::Right => "right",
        }
    }
}

/// **Vertical alignment within the element's box** — [`TextAlign`]'s twin, and
/// deliberately its exact twin.
///
/// The property panel's fourth text row (the reference screenshot's unlabelled
/// one): top, middle, bottom. It is a separate enum rather than a second use of
/// [`TextAlign`] because "left" and "top" are different words for a reader and
/// because a single `Align` shared by both axes is how a control ends up
/// writing the wrong one.
///
/// [`Middle`](VerticalAlign::Middle) is the default because it is what every
/// label in every document already displayed: `render::scene`'s `plan_labels`
/// has centred a label on its body's vertical centre since §9. So a document
/// written before this field existed loads with the field missing,
/// `#[serde(default)]` answers `Middle`, and nothing on screen moves — which is
/// why this field costs no format version of its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum VerticalAlign {
    Top,
    #[default]
    Middle,
    Bottom,
}

impl VerticalAlign {
    pub const ALL: &'static [VerticalAlign] = &[
        VerticalAlign::Top,
        VerticalAlign::Middle,
        VerticalAlign::Bottom,
    ];

    /// Where a block of `height` starts inside a box of `available`, measured
    /// from the box's top edge.
    ///
    /// Arithmetic, asserted with no window, for exactly the reasons
    /// [`TextAlign::offset`] is — including the clamp: a block taller than its
    /// box starts at the top edge and overflows **downwards**, rather than
    /// being centred into whatever is above it. That is the limitation
    /// [`crate::render::painter`] already records for a paragraph that outgrows
    /// its element, stated once as a number instead of twice as prose.
    pub fn offset(self, available: f32, height: f32) -> f32 {
        let slack = (available - height).max(0.0);
        match self {
            VerticalAlign::Top => 0.0,
            VerticalAlign::Middle => slack * 0.5,
            VerticalAlign::Bottom => slack,
        }
    }

    /// A short stable name, for an element id or a test. **Not user-facing.**
    pub const fn name(self) -> &'static str {
        match self {
            VerticalAlign::Top => "top",
            VerticalAlign::Middle => "middle",
            VerticalAlign::Bottom => "bottom",
        }
    }
}

/// Font properties (§32). **The whole text vocabulary the property panel
/// edits**, and deliberately no more: stroke colour, opacity and layer order
/// are the element's, not the font's, which is why they are not here.
///
/// [`size`](FontStyle::size) is a [`FontSize`] rather than a number — see that
/// type for why the discreteness is load-bearing rather than a simplification.
///
/// **[`Default`] is derived here and hand-written on [`ElementStyle`]**, and
/// the difference is the point rather than an inconsistency: every field below
/// is a type whose own default is the one wanted, so a derive cannot go wrong.
/// `ElementStyle::opacity` is an `f32` whose derived default is `0.0`, which
/// would make every element of a pre-field document load fully transparent.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FontStyle {
    pub size: FontSize,
    /// **An override, and nothing writes one today.** A label takes its
    /// element's *stroke* colour — see
    /// [`ElementStyle::text_color`](ElementStyle::text_color), which is the one
    /// answer every text painter reads. This stays because a file may carry it
    /// and dropping a field silently is how a colour disappears; `None`, which
    /// is every dodo-written document, means "ask the element".
    pub color: Option<Color>,
    pub family: FontFamily,
    pub align: TextAlign,
    /// [`TextAlign`]'s twin — the panel's fourth text row. See
    /// [`VerticalAlign`] for why `Middle` is the default and why that costs no
    /// format version.
    pub vertical_align: VerticalAlign,
    pub bold: bool,
    pub italic: bool,
}

impl FontStyle {
    /// The authored size in world units — the input the LOD ladder quantises
    /// against the zoom. **Read this rather than `size.world_size()` at a call
    /// site**, so a future per-element scale has one place to land.
    pub fn world_size(&self) -> f32 {
        self.size.world_size()
    }

    /// **Where a label belongs on the thing it labels: the middle of it.**
    ///
    /// A label has no position of its own in this engine — it is laid into its
    /// carrier's box every frame and there is no offset, no anchor and nothing
    /// persisted about where it sits. Its placement *is* these two alignments,
    /// so "a label defaults to the centre of its element" is this pair of
    /// values and nothing else.
    ///
    /// Applied where a label is **born** — `FlowEditor::commit_text` — rather
    /// than baked into [`Default`], because [`TextAlign::default`] is also the
    /// default for a *standalone* text element, which reads from its left edge
    /// like any other block of prose. A label centres; a paragraph does not.
    /// Once a label exists the two rows own these fields and nothing re-centres
    /// them, which is what "a label the user has moved keeps its position"
    /// means here.
    pub fn centre_on_element(&mut self) {
        self.align = TextAlign::Center;
        self.vertical_align = VerticalAlign::Middle;
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
    /// How [`fill`](ElementStyle::fill) is drawn: solid, or as one of two line
    /// sets. Ignored by open shapes, which have no interior.
    pub fill_style: FillStyle,
    /// How hard §13's hand presses on this element. The identity in
    /// [`Sloppiness::Artist`], and inert outside [`RenderStyle::Sketch`].
    pub sloppiness: Sloppiness,
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
            fill_style: FillStyle::default(),
            sloppiness: Sloppiness::default(),
            font: FontStyle::default(),
            start_marker: ArrowMarker::None,
            end_marker: ArrowMarker::None,
        }
    }
}

impl ElementStyle {
    /// **The colour text on this element is drawn in**, or `None` for "the
    /// theme's ink".
    ///
    /// One function because there was one bug: §32 gives an element a Stroke
    /// row and no separate text colour, and every text painter read
    /// `font.color` — which no control writes. A text element was the first
    /// costume of that (`properties`' module doc, item 3) and it was fixed
    /// *there*, in one painter, for one kind; a label on a node or an edge kept
    /// drawing in the theme's foreground and changing the element's stroke did
    /// nothing to it.
    ///
    /// So the answer lives on the style rather than at three call sites: a
    /// label is part of the element it labels, it takes the element's ink, and
    /// a stroke change moves it with no second press and nothing to remember.
    /// [`FontStyle::color`] still wins where a file carries one.
    pub fn text_color(&self) -> Option<Color> {
        self.font.color.or(self.stroke.color)
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
///
/// §13 asks for the enum to stay extensible, since blueprint or presentation
/// themes may follow, so the renderer never asks *"is this Sketch?"* — it asks
/// [`DocumentSettings::sketch_request`](crate::models::DocumentSettings::sketch_request),
/// which answers with the generator's parameters or with `None`. A future
/// variant that wants perturbed geometry returns a [`SketchStyle`] of its own
/// and every painter below already honours it; one that does not — a blueprint
/// grid, say — answers `None` and adds its own field beside this one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub enum RenderStyle {
    #[default]
    Clean,
    Sketch,
}

/// §13's hand-drawn parameters: what the deterministic generator in
/// [`render::sketch`](crate::render::sketch) is allowed to do to a canonical
/// outline.
///
/// **Document data rather than a viewer preference**, for the same reason
/// [`RenderStyle`] is: a diagram drawn by hand is drawn by hand every time it
/// is opened, and the wobble a reader sees has to be the wobble its author saw.
/// That is also why [`SketchStyle::seed`] is serialized — see it.
///
/// The lengths are in **screen pixels**, because that is the space outlines are
/// built in (see [`crate::render::shapes`]) and because a hand-drawn wobble is
/// a property of the pen rather than of the document: a 2 px tremor stays a
/// 2 px tremor when you zoom in, exactly as a real one on real paper does not
/// grow.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SketchStyle {
    /// Multiplies everything else. `0.0` is a clean line drawn by the sketch
    /// path; `1.0` is the default hand; above ~3.0 a shape stops reading as
    /// what it is.
    pub roughness: f32,
    /// How far a straight segment bows away from its chord, as a multiple of
    /// [`SketchStyle::jitter`]. `0.0` keeps every straight line straight and
    /// still jitters its endpoints.
    pub bowing: f32,
    /// How many times each outline is drawn. Two is the hand-drawn look — a
    /// line gone over twice — and it is also **the multiplier on everything
    /// this costs**, which is why it is a `u8` with a documented ceiling rather
    /// than an open number.
    pub stroke_count: u8,
    /// The document's seed. Combined with each element's
    /// [`ElementId`](crate::models::ElementId) to give the stable per-element
    /// seed §13 asks for.
    ///
    /// **Serialized, and that is the point.** An element's wobble has to
    /// survive a save and a reload, or reopening a document would redraw every
    /// shape differently — which is the same failure as generating fresh random
    /// values per repaint, only slower to notice.
    pub seed: u64,
    /// The per-point random displacement in screen pixels at
    /// `roughness == 1.0`.
    pub jitter: f32,
}

impl Default for SketchStyle {
    fn default() -> SketchStyle {
        SketchStyle::DEFAULT
    }
}

impl SketchStyle {
    /// The default hand. Two strokes, a 2 px tremor and a gentle bow — enough
    /// that a rectangle reads as drawn rather than as printed, and little
    /// enough that a 24 px node is still a rectangle.
    pub const DEFAULT: SketchStyle = SketchStyle {
        roughness: 1.0,
        bowing: 1.0,
        stroke_count: 2,
        seed: 0x5EED_5EED_5EED_5EED,
        jitter: 2.0,
    };

    /// The most strokes one outline may be drawn with.
    ///
    /// A ceiling rather than a taste: every stroke is a full path — its own
    /// vertices *and* its own [`RenderBudgets::nanos_per_path`](crate::budgets::RenderBudgets::nanos_per_path)
    /// of fixed CPU — so `stroke_count` multiplies both frame budgets directly.
    /// Four is already more than any hand-drawn look needs.
    pub const MAX_STROKES: u8 = 4;

    /// **The tolerance sketch geometry is flattened at, as a multiple of the
    /// document's.**
    ///
    /// A hand-drawn line is deliberately imprecise, so flattening its bow to a
    /// quarter of a pixel is spending precision on imprecision. Measured on the
    /// M1 (see [`crate::render::sketch`]): a sketched 160×64 rectangle costs
    /// 1,104 estimated vertices at the document tolerance and **518 at 3×**,
    /// with no visible difference at any zoom the shape is legible at. It is
    /// applied through [`RenderQuality::new`], so it is part of the cache key
    /// like every other tolerance.
    pub const TOLERANCE_FACTOR: f32 = 3.0;

    /// Clamped into the range the generator and the budget were measured
    /// against. **The only constructor that should reach a document**: a
    /// `roughness` of 40 is not a rougher drawing, it is a shape nobody can
    /// recognise costing several times the vertices.
    pub fn new(roughness: f32, bowing: f32, stroke_count: u8, seed: u64, jitter: f32) -> Self {
        SketchStyle {
            roughness: roughness.clamp(0.0, 4.0),
            bowing: bowing.clamp(0.0, 4.0),
            stroke_count: stroke_count.clamp(1, SketchStyle::MAX_STROKES),
            seed,
            jitter: jitter.clamp(0.0, 8.0),
        }
    }

    /// The same hand with a different seed, which is how one document draws two
    /// elements differently without a second style.
    pub fn with_seed(mut self, seed: u64) -> SketchStyle {
        self.seed = seed;
        self
    }

    /// The strokes this style actually draws, clamped to
    /// [`SketchStyle::MAX_STROKES`]. **Read this rather than the field** — a
    /// document from disk carries whatever number was written in it.
    pub fn strokes(&self) -> u8 {
        self.stroke_count.clamp(1, SketchStyle::MAX_STROKES)
    }

    /// The tolerance sketch geometry is tessellated at, given the document's.
    /// See [`SketchStyle::TOLERANCE_FACTOR`].
    pub fn quality(&self, document: RenderQuality) -> RenderQuality {
        RenderQuality::new(document.flattening_tolerance * SketchStyle::TOLERANCE_FACTOR)
    }

    /// **The hashable form, for a geometry cache key.**
    ///
    /// Every field is in it, because every field changes the generated
    /// geometry: a cache that ignored `roughness` would serve the old hand
    /// after the style changed, which is the same class of bug as ignoring the
    /// flattening tolerance (Phase 0 §3 correction 5). Never zero — zero is
    /// [`GeometryKey`](crate::render::cache::GeometryKey)'s "not sketched", and
    /// a sketched entry must not collide with a clean one.
    pub fn cache_key(&self) -> u32 {
        let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
        let mut eat = |value: u64| {
            hash ^= value;
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        };

        // Quantised, for the same reason `RenderQuality::cache_key` quantises:
        // `f32` hashes are wrong in both directions, and a hundredth of a pixel
        // is already finer than any hand.
        eat((self.roughness * 100.0).round().max(0.0) as u64);
        eat((self.bowing * 100.0).round().max(0.0) as u64);
        eat(self.strokes() as u64);
        eat(self.seed);
        eat((self.jitter * 100.0).round().max(0.0) as u64);

        // Folded to 32 bits and forced non-zero: the key field is a `u32` so
        // that `GeometryKey` stays `Copy` and small, and 0 means clean.
        (((hash >> 32) as u32) ^ (hash as u32)) | 1
    }
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
        ArrowMarker, Color, DashPattern, EdgeRouting, ElementStyle, FontFamily, FontSize,
        FontStyle, RenderQuality, RenderStyle, StrokeStyle, TextAlign, VerticalAlign,
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

    /// The four steps are the vocabulary Phase 11's panel edits, so their world
    /// sizes are a contract rather than a taste — a document authored at `L`
    /// must not silently become a different size in a later build.
    #[test]
    fn the_four_steps_have_the_sizes_the_panel_names() {
        assert_eq!(FontSize::ALL.len(), 4);
        assert_eq!(FontSize::Small.world_size(), 12.0);
        assert_eq!(FontSize::Medium.world_size(), 16.0);
        assert_eq!(FontSize::Large.world_size(), 20.0);
        assert_eq!(FontSize::ExtraLarge.world_size(), 28.0);
        assert_eq!(FontSize::default(), FontSize::Medium);
    }

    /// The migration ladder needs this, and so does any import: a number has to
    /// land on a step, and it has to land on the same one every time.
    #[test]
    fn a_world_size_snaps_to_the_nearest_step() {
        assert_eq!(FontSize::nearest(0.0), FontSize::Small);
        assert_eq!(FontSize::nearest(12.0), FontSize::Small);
        assert_eq!(FontSize::nearest(14.0), FontSize::Small, "ties go smaller");
        assert_eq!(FontSize::nearest(15.0), FontSize::Medium);
        assert_eq!(FontSize::nearest(21.0), FontSize::Large);
        assert_eq!(FontSize::nearest(1_000.0), FontSize::ExtraLarge);
    }

    /// Alignment is arithmetic, so it is asserted with no window. The
    /// overflow case is the one worth pinning: a run wider than its box must
    /// start at the left edge rather than be centred into its neighbours.
    #[test]
    fn alignment_places_a_run_inside_its_box_and_never_outside_it() {
        assert_eq!(TextAlign::Left.offset(100.0, 40.0), 0.0);
        assert_eq!(TextAlign::Center.offset(100.0, 40.0), 30.0);
        assert_eq!(TextAlign::Right.offset(100.0, 40.0), 60.0);

        for align in TextAlign::ALL {
            assert_eq!(
                align.offset(40.0, 100.0),
                0.0,
                "{} must not push an over-wide run out of its box",
                align.name()
            );
        }
    }

    /// The vertical twin, asserted the same way and with the same overflow
    /// rule: a block taller than its box starts at the top and overflows
    /// downwards, which is the limitation `render::painter` records.
    #[test]
    fn vertical_alignment_places_a_block_inside_its_box_and_never_outside_it() {
        assert_eq!(VerticalAlign::Top.offset(100.0, 40.0), 0.0);
        assert_eq!(VerticalAlign::Middle.offset(100.0, 40.0), 30.0);
        assert_eq!(VerticalAlign::Bottom.offset(100.0, 40.0), 60.0);

        for align in VerticalAlign::ALL {
            assert_eq!(
                align.offset(40.0, 100.0),
                0.0,
                "{} must not push an over-tall block out of its box",
                align.name()
            );
        }
    }

    /// **`Middle` is what every label already displayed**, so a document
    /// written before the field existed must load unchanged. `#[serde(default)]`
    /// is what answers, and this is the assertion that the answer is the one
    /// the renderer was already drawing.
    #[test]
    fn a_font_with_no_vertical_alignment_loads_middle() {
        let font: FontStyle = serde_json::from_str(r#"{"size":"Large"}"#).unwrap();
        assert_eq!(font.vertical_align, VerticalAlign::Middle);
        assert_eq!(font.size, FontSize::Large);
        assert_eq!(
            font.align,
            TextAlign::Left,
            "the horizontal default is prose's, not a label's — see \
             FontStyle::centre_on_element"
        );
    }

    /// **The whole of "a label defaults to the centre of its element".** There
    /// is no other placement state; these two fields are it.
    #[test]
    fn centring_a_label_is_the_two_alignments_and_nothing_else() {
        let mut font = FontStyle {
            size: FontSize::Large,
            family: FontFamily::Code,
            bold: true,
            ..FontStyle::default()
        };
        let before = font.clone();
        font.centre_on_element();

        assert_eq!(font.align, TextAlign::Center);
        assert_eq!(font.vertical_align, VerticalAlign::Middle);
        assert_eq!(
            FontStyle {
                align: before.align,
                vertical_align: before.vertical_align,
                ..font.clone()
            },
            before,
            "centring a label must touch nothing but where it sits"
        );
    }

    /// **A label takes its element's ink.** The Stroke row is the only colour
    /// control a label's element has, so a stroke change has to move the label
    /// with it — with no second press, and for a node and an edge alike.
    #[test]
    fn a_label_takes_the_elements_stroke_colour() {
        let mut style = ElementStyle::default();
        assert_eq!(
            style.text_color(),
            None,
            "an unstyled element defers to the theme's ink"
        );

        let red = Color::from_rgba8(224, 49, 49, 255);
        style.stroke.color = Some(red);
        assert_eq!(style.text_color(), Some(red));

        let blue = Color::from_rgba8(25, 113, 194, 255);
        style.font.color = Some(blue);
        assert_eq!(
            style.text_color(),
            Some(blue),
            "an explicit font colour from a file still wins"
        );
    }

    /// The three families, the four sizes and the two alignments are enum
    /// variants a panel maps over, so a duplicate id would collide two buttons
    /// — the same failure the palette's `no_two_buttons_share_an_id` guards.
    #[test]
    fn every_font_choice_has_its_own_stable_name() {
        let names: Vec<&str> = FontSize::ALL
            .iter()
            .map(|it| it.name())
            .chain(FontFamily::ALL.iter().map(|it| it.name()))
            .chain(TextAlign::ALL.iter().map(|it| it.name()))
            .chain(VerticalAlign::ALL.iter().map(|it| it.name()))
            .collect();
        for (index, name) in names.iter().enumerate() {
            assert!(!names[index + 1..].contains(name), "{name} appears twice");
        }
    }

    /// Every platform's answer, asserted from whichever platform this is —
    /// the root `AGENTS.md` invariant, and the reason the host is a parameter.
    #[test]
    fn the_hand_drawn_family_names_a_face_on_every_platform() {
        use dodo_paths::HostOs;

        for host in [HostOs::MacOs, HostOs::Windows, HostOs::Unix] {
            assert!(
                !FontFamily::HandDrawn.preferred_faces(host).is_empty(),
                "{host:?} has no hand-drawn candidate at all"
            );
            assert!(
                FontFamily::Normal.preferred_faces(host).is_empty(),
                "the theme's own font is named by the theme, never here"
            );
            assert!(FontFamily::Code.preferred_faces(host).is_empty());
        }
    }

    #[test]
    fn opacity_defaults_to_opaque_when_an_older_document_omits_it() {
        // `#[serde(default)]` on a float defaults to 0.0, which would make
        // every element from a document written without the field invisible.
        let style: ElementStyle = serde_json::from_str("{}").expect("all fields are defaulted");

        assert_eq!(style.opacity, 1.0);
    }
}
