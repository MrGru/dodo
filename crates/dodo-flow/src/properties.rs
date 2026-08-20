//! **The contextual property panel, as a vocabulary** — which sections exist,
//! which of them each kind of selection gets, and what each control's steps
//! mean in the model.
//!
//! # Why this is a module and not part of the view
//!
//! **The panel is contextual, and the differences between its four forms are
//! the specification** rather than a detail of it — it is not one panel with
//! rows greyed out:
//!
//! | | node | edge | text | image |
//! |---|:--:|:--:|:--:|:--:|
//! | Stroke | ● | ● | ● | — |
//! | Background | ● | — | — | — |
//! | Fill | ● | — | — | — |
//! | Stroke width | ● | ● | — | — |
//! | Stroke style | ● | ● | — | — |
//! | Sloppiness | ● | ● | — | — |
//! | Edges (corners) | ● | — | — | ● |
//! | Arrow type | — | ● | — | — |
//! | Arrowheads | — | ● | — | — |
//! | Font family | — | — | ● | — |
//! | Font size | — | — | ● | — |
//! | Text align | — | — | ● | — |
//! | Opacity | ● | ● | ● | ● |
//! | Layers | ● | ● | ● | ● |
//! | Actions | ● | ● | ● | ● |
//!
//! So a node gets Background, Fill and a corner style; an edge gets Arrow type
//! and Arrowheads instead; text gets Font family, size and alignment and
//! nothing about strokes; and an image is the minimal case — Edges, Opacity,
//! Layers, Actions and nothing else, because none of the rest means anything to
//! a bitmap. **Opacity, Layers and Actions are the only three rows on every
//! form.**
//!
//! That table is a *fact about the product*. It is exactly the sort of thing
//! that rots when it is spread across fifteen `if` statements in a render body,
//! and it needs no window to be checked — so it is
//! [`SelectionKind::sections`], and the test beside it states the table a
//! second time and independently. Two statements of one fact is the point: a
//! row that moves has to move twice, and a row that moves once fails.
//!
//! The same argument covers the control steps. "Bold" is a stroke width, "Round"
//! is a corner radius, "Cartoonist" is a roughness multiplier, and each has to
//! survive the round trip *back* — a panel that cannot read its own state shows
//! every button unselected, which looks like a rendering bug and is really a
//! missing inverse. Every step below has both directions and a test that they
//! compose.
//!
//! # The three decisions worth arguing about
//!
//! 1. **A mixed selection gets the intersection**, not the union. Showing a
//!    Background row for a selection holding a node and a text element would
//!    offer a control that silently applies to half of what is selected, which
//!    is the failure mode this crate has already recorded twice under other
//!    names. [`sections_for`] is where that is decided.
//! 2. **Sloppiness is disabled, not hidden, in Clean mode.** It is a real
//!    per-element property and it means nothing until the document is drawn by
//!    hand. Hiding it would make a row appear and disappear with a *document*
//!    setting rather than with the selection, which is not what "contextual"
//!    means here; storing the choice silently would be a control that ignores
//!    the user, which the phase brief forbids by name. So it is drawn muted
//!    with a tooltip that says why — which is the same answer `views::palette`
//!    already gives for Delete with nothing selected, and being consistent with
//!    dodo is worth more than being clever. [`Availability`] carries it.
//! 3. **`Image` is in the enum before images exist.** Phase 12 adds a kind, not
//!    a panel: [`SelectionKind::of_kind`] already answers `Image` for
//!    [`ElementKind::Image`], the table already has its column, and nothing here
//!    invents an image property that has not been specified.
//!
//! # A row is not finished until a painter reads it
//!
//! This is the rule the panel has now broken four times, in four costumes, and
//! it is written here because this table is where a row is *born*:
//!
//! 1. Phase 11 shipped `fill_style` and `sloppiness` stored, undoable, read
//!    back by this table — and painted by nothing.
//! 2. `stroke.dash` was the same on a **node**: §32's Stroke style row is
//!    offered for a node and for an edge, and only `render::edges` ever read
//!    it.
//! 3. A **text** element's Stroke row is the only colour control it has, it
//!    writes `stroke.color`, and every text painter read `font.color` — which
//!    nothing writes.
//! 4. Widest of the four: a node at working zoom is a *rich* node, a GPUI
//!    element, and the element painted its body from the theme. Stroke colour,
//!    fill, width, opacity, dash and hatch all reached the document and stopped
//!    — but only in Clean mode, because a hand-drawn border has no `div` form
//!    and forced the canvas to paint the body in Sketch. That is what "the
//!    properties only work in sketch mode" was.
//!
//! Each of the four passed every test in the crate, because a test on this
//! table and a test on the round trip both pass either way. **So the test for a
//! new row asserts what reaches the painter** — the primitive, its colour, its
//! cache part — and `render::scene`'s
//! `a_restyled_rich_node_reaches_the_painter_in_both_render_styles` is the
//! shape to copy.
//!
//! [`Availability`] is the honest exit when a row genuinely cannot be drawn:
//! muted, with a tooltip that says why. Sloppiness in Clean mode is the only
//! member.
//!
//! **This file names no UI framework.** The glyphs are `views::properties`'s,
//! and the strings are `dodo_i18n::flow`'s.

use crate::{
    models::{
        ArrowMarker, Color, DashPattern, EdgeRouting, ElementKind, ElementStyle, FillStyle,
        FontFamily, FontSize, ImageCrop, Sloppiness, TextAlign,
    },
    runtime::NodeShape,
};

/// What is selected, as far as the panel is concerned.
///
/// Four forms, one per row of the module doc's table. Not a count and not a
/// set — a selection holding several elements is several of these, and
/// [`sections_for`] folds them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SelectionKind {
    /// A shape or a graph node: everything with a body and a border.
    Node,
    /// A connection between two nodes.
    Edge,
    /// §9's standalone text element.
    Text,
    /// §10's embedded raster image. **Nothing produces one yet** — see the
    /// module doc.
    Image,
}

impl SelectionKind {
    pub const ALL: &'static [SelectionKind] = &[
        SelectionKind::Node,
        SelectionKind::Edge,
        SelectionKind::Text,
        SelectionKind::Image,
    ];

    /// Which panel a node's kind gets.
    ///
    /// From [`ElementKind`] rather than from [`NodeShape`], because the shape is
    /// the *renderer's* projection and answers `Other` for everything it cannot
    /// draw — an image and a frame would be indistinguishable through it, which
    /// is precisely the confusion Phase 7.5 recorded.
    pub fn of_kind(kind: &ElementKind) -> SelectionKind {
        match kind {
            ElementKind::Text => SelectionKind::Text,
            ElementKind::Image => SelectionKind::Image,
            _ => SelectionKind::Node,
        }
    }

    /// The same question asked of a body the renderer already projected. Here
    /// for the one caller that has a [`NodeShape`] and no kind; it cannot tell
    /// an image from a frame and says so by answering [`Node`](SelectionKind::Node)
    /// for both.
    pub fn of_shape(shape: NodeShape) -> SelectionKind {
        match shape {
            NodeShape::Text => SelectionKind::Text,
            _ => SelectionKind::Node,
        }
    }

    /// **The table in the module doc, as the code that draws from it.**
    ///
    /// In panel order, top to bottom. The test beside this states the same
    /// table independently, so a row that moves here without moving there is a
    /// failure rather than a redraw.
    pub fn sections(self) -> &'static [PanelSection] {
        use PanelSection::*;
        match self {
            SelectionKind::Node => &[
                Stroke,
                Background,
                Fill,
                StrokeWidth,
                StrokeStyle,
                Sloppiness_,
                Corners,
                Opacity,
                Layers,
                Actions,
            ],
            SelectionKind::Edge => &[
                Stroke,
                StrokeWidth,
                StrokeStyle,
                Sloppiness_,
                ArrowType,
                Arrowheads,
                Opacity,
                Layers,
                Actions,
            ],
            SelectionKind::Text => &[
                Stroke,
                FontFamilyRow,
                FontSizeRow,
                TextAlignRow,
                Opacity,
                Layers,
                Actions,
            ],
            SelectionKind::Image => &[Corners, Opacity, Layers, Actions],
        }
    }

    /// A short, stable name. **Not user-facing.**
    pub const fn name(self) -> &'static str {
        match self {
            SelectionKind::Node => "node",
            SelectionKind::Edge => "edge",
            SelectionKind::Text => "text",
            SelectionKind::Image => "image",
        }
    }
}

/// One row of the panel.
///
/// The three trailing underscores and `Row` suffixes are deliberate: the names
/// that would collide are `models`' own types, and a section is not a value —
/// `PanelSection::FontSizeRow` is *the row that edits* [`FontSize`], and reading
/// them as the same thing is how a control ends up writing the wrong field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PanelSection {
    /// Five preset colours, a separator, and the current one.
    Stroke,
    /// Five fills, the first of them transparent.
    Background,
    /// Hachure / cross-hatch / solid.
    Fill,
    /// Thin / bold / extra bold, drawn as line samples.
    StrokeWidth,
    /// Solid / dashed / dotted.
    StrokeStyle,
    /// Three increasingly wobbly strokes. See [`Availability`].
    Sloppiness_,
    /// Sharp / round corners — the existing corner radius.
    Corners,
    /// Straight / curved / elbow, over [`EdgeRouting`].
    ArrowType,
    /// The start and end markers, as two toggles.
    Arrowheads,
    /// Hand-drawn / normal / code.
    FontFamilyRow,
    /// S / M / L / XL.
    FontSizeRow,
    /// Left / centre / right.
    TextAlignRow,
    /// A slider, 0 to 100.
    Opacity,
    /// The four depth buttons.
    Layers,
    /// Duplicate, delete, link — and, on an edge, edit points.
    Actions,
}

impl PanelSection {
    /// A short, stable name, for element ids and tests. **Not user-facing** —
    /// the labels are `dodo_i18n::flow`'s.
    pub const fn name(self) -> &'static str {
        match self {
            PanelSection::Stroke => "stroke",
            PanelSection::Background => "background",
            PanelSection::Fill => "fill",
            PanelSection::StrokeWidth => "stroke-width",
            PanelSection::StrokeStyle => "stroke-style",
            PanelSection::Sloppiness_ => "sloppiness",
            PanelSection::Corners => "edges",
            PanelSection::ArrowType => "arrow-type",
            PanelSection::Arrowheads => "arrowheads",
            PanelSection::FontFamilyRow => "font-family",
            PanelSection::FontSizeRow => "font-size",
            PanelSection::TextAlignRow => "text-align",
            PanelSection::Opacity => "opacity",
            PanelSection::Layers => "layers",
            PanelSection::Actions => "actions",
        }
    }
}

/// **The sections a whole selection gets**: the intersection of its kinds'.
///
/// The intersection rather than the union — see decision 1 in the module doc.
/// Order is [`SelectionKind::Node`]'s where the two agree and each kind's own
/// otherwise, which comes out right because the four forms are one table with
/// rows removed rather than four different orders.
///
/// An empty selection gets nothing, and the caller draws no panel at all rather
/// than an empty card.
pub fn sections_for(kinds: &[SelectionKind]) -> Vec<PanelSection> {
    let Some((first, rest)) = kinds.split_first() else {
        return Vec::new();
    };

    first
        .sections()
        .iter()
        .copied()
        .filter(|section| rest.iter().all(|kind| kind.sections().contains(section)))
        .collect()
}

/// **What is selected, read off a world** — the step between §28's selection
/// and [`sections_for`].
///
/// Here rather than in the view, and that is the whole point: "select an edge
/// and the panel loses Background and gains Arrow type" is a claim about the
/// product — it is the whole of what "contextual" means here — and a version of
/// it that lives in a `render` body can only be checked by opening a window.
/// This is three lines and it makes the claim a test.
///
/// Tombstoned elements are skipped, for the reason
/// [`FlowEditor::link_at`](crate::commands::FlowEditor::link_at) skips them: a
/// removed element is still in the selection set until something clears it, and
/// a panel that offered to restyle one would be offering to edit what nobody
/// can see.
pub fn selection_kinds(world: &crate::runtime::GraphWorld) -> Vec<SelectionKind> {
    let selection = world.selection();
    selection
        .nodes()
        .iter()
        .filter(|&&node| world.node_is_live(node))
        .map(|&node| SelectionKind::of_kind(world.nodes().kind(node)))
        .chain(
            selection
                .edges()
                .iter()
                .filter(|&&edge| world.edge_is_live(edge))
                .map(|_| SelectionKind::Edge),
        )
        .collect()
}

/// Whether a control can be used, and — when it cannot — the reason, so the
/// panel can say it rather than ignore the press.
///
/// Two states rather than a `bool` for exactly one reason: a `bool` has no room
/// for the *why*, and a disabled control with no explanation is
/// indistinguishable from a broken one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Availability {
    Live,
    /// The control edits a real property that this document's render style
    /// cannot show. See decision 2 in the module doc.
    NeedsSketchMode,
}

impl Availability {
    pub fn is_live(self) -> bool {
        self == Availability::Live
    }

    /// What the Sloppiness row's state is, given whether the document is drawn
    /// by hand.
    pub fn of_sloppiness(sketching: bool) -> Availability {
        if sketching {
            Availability::Live
        } else {
            Availability::NeedsSketchMode
        }
    }
}

// ---- the stepped controls -------------------------------------------------

/// Thin / bold / extra bold, in world units.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum StrokeWidthStep {
    #[default]
    Thin,
    Bold,
    ExtraBold,
}

impl StrokeWidthStep {
    pub const ALL: &'static [StrokeWidthStep] = &[
        StrokeWidthStep::Thin,
        StrokeWidthStep::Bold,
        StrokeWidthStep::ExtraBold,
    ];

    /// **Thin is `1.0`, which is [`StrokeStyle`](crate::models::StrokeStyle)'s
    /// own default**, so a shape drawn before this control existed reads back as
    /// Thin rather than as nothing selected.
    pub const fn width(self) -> f32 {
        match self {
            StrokeWidthStep::Thin => 1.0,
            StrokeWidthStep::Bold => 2.0,
            StrokeWidthStep::ExtraBold => 4.0,
        }
    }

    /// The step nearest a stored width — the inverse the panel needs to show
    /// which button is on. Nearest rather than exact, because a width can also
    /// arrive from a file another tool wrote.
    pub fn of(width: f32) -> StrokeWidthStep {
        *StrokeWidthStep::ALL
            .iter()
            .min_by(|a, b| {
                (a.width() - width)
                    .abs()
                    .total_cmp(&(b.width() - width).abs())
            })
            .expect("the list is never empty")
    }

    pub const fn name(self) -> &'static str {
        match self {
            StrokeWidthStep::Thin => "thin",
            StrokeWidthStep::Bold => "bold",
            StrokeWidthStep::ExtraBold => "extra-bold",
        }
    }
}

/// Solid / dashed / dotted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum StrokeDashStep {
    #[default]
    Solid,
    Dashed,
    Dotted,
}

impl StrokeDashStep {
    pub const ALL: &'static [StrokeDashStep] = &[
        StrokeDashStep::Solid,
        StrokeDashStep::Dashed,
        StrokeDashStep::Dotted,
    ];

    /// The pattern, in screen pixels — which is what
    /// [`DashPattern`] holds, and why a dotted line
    /// stays dotted rather than turning solid when you zoom out.
    pub fn pattern(self) -> DashPattern {
        match self {
            StrokeDashStep::Solid => DashPattern::solid(),
            StrokeDashStep::Dashed => DashPattern::new(vec![8.0, 6.0]),
            StrokeDashStep::Dotted => DashPattern::new(vec![1.5, 4.0]),
        }
    }

    /// Which step a stored pattern reads as. **Solid is decided by
    /// `is_solid` and the other two by the length of the "on" run**, rather
    /// than by comparing whole patterns: a file may hold a four-entry pattern
    /// this control cannot express, and the honest answer is still "that is a
    /// dashed line" rather than "nothing is selected".
    pub fn of(dash: &DashPattern) -> StrokeDashStep {
        match dash.spec() {
            None => StrokeDashStep::Solid,
            Some((on, _)) if on <= 3.0 => StrokeDashStep::Dotted,
            Some(_) => StrokeDashStep::Dashed,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            StrokeDashStep::Solid => "solid",
            StrokeDashStep::Dashed => "dashed",
            StrokeDashStep::Dotted => "dotted",
        }
    }
}

/// Sharp or round corners — the panel's **Edges** row, over the existing
/// corner radius.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum CornerStyle {
    #[default]
    Sharp,
    Round,
}

impl CornerStyle {
    pub const ALL: &'static [CornerStyle] = &[CornerStyle::Sharp, CornerStyle::Round];

    /// The radius a round corner gets, in world units. Large enough to read as
    /// deliberate at a normal shape size and small enough that a 40-unit node
    /// is still a rectangle.
    pub const ROUND_RADIUS: f32 = 12.0;

    pub const fn radius(self) -> f32 {
        match self {
            CornerStyle::Sharp => 0.0,
            CornerStyle::Round => CornerStyle::ROUND_RADIUS,
        }
    }

    /// Any radius at all reads as Round. A threshold rather than an equality,
    /// because a graph node's body carries a radius of its own and a file may
    /// carry any number.
    pub fn of(radius: f32) -> CornerStyle {
        if radius > 0.0 {
            CornerStyle::Round
        } else {
            CornerStyle::Sharp
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            CornerStyle::Sharp => "sharp",
            CornerStyle::Round => "round",
        }
    }
}

/// Straight / curved / elbow — the panel's **Arrow type** row, over §8's
/// [`EdgeRouting`].
///
/// Three buttons over five routings, and the two that have no button are not
/// lost: `SimpleBezier` reads back as Curved and `SmoothStep` as Elbow, so a
/// document that names one keeps it until somebody presses a different button.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ArrowKind {
    Straight,
    #[default]
    Curved,
    Elbow,
}

impl ArrowKind {
    pub const ALL: &'static [ArrowKind] =
        &[ArrowKind::Straight, ArrowKind::Curved, ArrowKind::Elbow];

    pub const fn routing(self) -> EdgeRouting {
        match self {
            ArrowKind::Straight => EdgeRouting::Straight,
            ArrowKind::Curved => EdgeRouting::Bezier,
            ArrowKind::Elbow => EdgeRouting::Step,
        }
    }

    pub const fn of(routing: EdgeRouting) -> ArrowKind {
        match routing {
            EdgeRouting::Straight => ArrowKind::Straight,
            EdgeRouting::Bezier | EdgeRouting::SimpleBezier => ArrowKind::Curved,
            EdgeRouting::Step | EdgeRouting::SmoothStep => ArrowKind::Elbow,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            ArrowKind::Straight => "straight",
            ArrowKind::Curved => "curved",
            ArrowKind::Elbow => "elbow",
        }
    }
}

/// Which end of an edge an arrowhead toggle belongs to.
///
/// **The two are toggles rather than pickers**, and that is the specified
/// control: one button per end, each showing whether that end has a head. A
/// picker over five [`ArrowMarker`]s would be a popover this panel has nowhere
/// to put, for a choice almost nobody makes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArrowEnd {
    Start,
    End,
}

impl ArrowEnd {
    pub const ALL: &'static [ArrowEnd] = &[ArrowEnd::Start, ArrowEnd::End];

    /// The marker this end holds in a style.
    pub fn marker(self, style: &ElementStyle) -> ArrowMarker {
        match self {
            ArrowEnd::Start => style.start_marker,
            ArrowEnd::End => style.end_marker,
        }
    }

    /// Writes the toggled state. `on` is [`ArrowMarker::Arrow`], off is
    /// [`ArrowMarker::None`] — and a style that already names a *different*
    /// marker keeps it when toggled on, so a document written with a diamond
    /// head does not lose it to a press meaning "yes, an arrowhead".
    pub fn set(self, style: &mut ElementStyle, on: bool) {
        let slot = match self {
            ArrowEnd::Start => &mut style.start_marker,
            ArrowEnd::End => &mut style.end_marker,
        };
        *slot = match (on, *slot) {
            (false, _) => ArrowMarker::None,
            (true, ArrowMarker::None) => ArrowMarker::Arrow,
            (true, kept) => kept,
        };
    }

    pub const fn name(self) -> &'static str {
        match self {
            ArrowEnd::Start => "arrowhead-start",
            ArrowEnd::End => "arrowhead-end",
        }
    }
}

/// One of the three action buttons every panel has, plus the two that belong to
/// one kind each.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ElementAction {
    Duplicate,
    /// **Phase 9's**, reused rather than rewritten.
    Delete,
    /// Set, change or clear the element's hyperlink.
    Link,
    /// An edge only: edit the route's waypoints. **Deferred** — see
    /// [`ElementAction::for_kind`].
    EditPoints,
    /// An image only: adjust which part of the source is shown. Phase 12's.
    Crop,
}

impl ElementAction {
    /// The actions a panel shows, left to right: duplicate, delete, link.
    ///
    /// **`EditPoints` is not here**, and its absence is the honest kind. An
    /// edge in this engine stores two endpoints and a routing, never a point
    /// list — §7's waypoints are a change to the document model, which
    /// `render::shapes::line`'s doc already records as the same gap that makes
    /// a free arrow point down its own diagonal. A button that opened nothing
    /// would be the "control that produces nothing" failure Phase 7.5 caught.
    ///
    /// **`Crop` joined the image's row in Phase 12**, which is what that phase
    /// had to be true before: it is the fourth button on an image panel and on
    /// no other, because it is the one action whose meaning is a property of
    /// the kind rather than of the element.
    pub fn for_kind(kind: SelectionKind) -> &'static [ElementAction] {
        match kind {
            SelectionKind::Image => &[
                ElementAction::Duplicate,
                ElementAction::Delete,
                ElementAction::Link,
                ElementAction::Crop,
            ],
            _ => &[
                ElementAction::Duplicate,
                ElementAction::Delete,
                ElementAction::Link,
            ],
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            ElementAction::Duplicate => "duplicate",
            ElementAction::Delete => "delete",
            ElementAction::Link => "link",
            ElementAction::EditPoints => "edit-points",
            ElementAction::Crop => "crop",
        }
    }
}

/// **What one press of the Crop button does**, given the picture, the crop and
/// the frame it is shown in (§10).
///
/// # Why crop is an action and not a mode
///
/// The brief asks for crop *metadata* rather than a rewrite of the pixels, and
/// the panel gives it one button. A modal crop editor — grips inside the
/// picture, a dimmed surround, a confirm — is a second interaction model on a
/// canvas that already has one, for an operation the existing resize gesture
/// can express completely:
///
/// 1. **Shift-drag a corner.** An image resize keeps its proportions by
///    default, so shift is what asks for a free one, and the picture visibly
///    stretches into the frame the user is drawing.
/// 2. **Press Crop.** The stretch becomes a *window* on the source: the frame
///    stays exactly where it was drawn, the picture inside it is no longer
///    distorted, and the bytes are untouched.
///
/// So the button's meaning depends on what the element is showing, and this is
/// the whole of that rule. It is a pure function of three numbers and it names
/// no UI framework, so the panel's label, the editor's edit and the test all
/// read the same answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CropChoice {
    /// The frame is a different shape from the picture in it, so the picture is
    /// stretched: crop to the frame, keeping the middle.
    ToFrame,
    /// The frame already matches, and part of the picture is hidden: show the
    /// whole of it again.
    ///
    /// **The frame's height changes with it**, because a full picture in a
    /// cropped frame would simply be stretched the other way. The width is what
    /// is kept — a user who has placed an image has chosen how much horizontal
    /// room it takes.
    Reset,
}

impl CropChoice {
    /// A short stable name, for a test or an element id. **Not user-facing.**
    pub const fn name(self) -> &'static str {
        match self {
            CropChoice::ToFrame => "crop-to-frame",
            CropChoice::Reset => "crop-reset",
        }
    }
}

/// How far a frame's aspect may differ from the picture's before the picture
/// counts as stretched — a fraction of the ratio itself.
///
/// Not zero, and that matters: a frame produced by an aspect-locked resize
/// lands within a rounding error of the ratio it was locked to, and an exact
/// comparison would offer to "crop" it to a window a hundredth of a percent
/// narrower. One per cent is far below what an eye reads as a distortion and
/// far above `f32`'s noise over a drag.
pub const CROP_ASPECT_TOLERANCE: f32 = 0.01;

/// **What the Crop button would do**, or `None` when it would do nothing — in
/// which case the panel draws it muted with a tooltip, exactly as it does for
/// Sloppiness in clean mode and Delete with nothing selected.
///
/// `source` is the resource's own width/height, `frame` is the element's, and
/// `crop` is what it is showing now.
pub fn crop_choice(source: f32, frame: f32, crop: ImageCrop) -> Option<CropChoice> {
    if !(source.is_finite() && source > 0.0) || !(frame.is_finite() && frame > 0.0) {
        return None;
    }

    let shown = crop.aspect(source);
    if ((shown / frame) - 1.0).abs() > CROP_ASPECT_TOLERANCE {
        return Some(CropChoice::ToFrame);
    }
    (!crop.is_full()).then_some(CropChoice::Reset)
}

// ---- colour ---------------------------------------------------------------

/// The five stroke presets, left to right.
///
/// Excalidraw's palette, deliberately and exactly. Not imitation for its own
/// sake: a user coming from that tool recognises the row, and these five are
/// already known to read on both a light and a dark canvas — a real constraint
/// here, because dodo has both and a document must not carry a palette of its
/// own (see [`ElementStyle`]'s `Option<Color>`).
pub const STROKE_SWATCHES: [Color; 5] = [
    Color::rgb(0.118, 0.118, 0.118), // #1e1e1e
    Color::rgb(0.878, 0.192, 0.192), // #e03131
    Color::rgb(0.184, 0.620, 0.267), // #2f9e44
    Color::rgb(0.098, 0.443, 0.761), // #1971c2
    Color::rgb(0.941, 0.549, 0.000), // #f08c00
];

/// The five background presets. **The first is transparent**, and the panel
/// draws it as a checkerboard rather than as an empty square — an empty square
/// is what a missing swatch looks like.
pub const BACKGROUND_SWATCHES: [Color; 5] = [
    Color::TRANSPARENT,
    Color::rgb(1.000, 0.788, 0.788), // #ffc9c9
    Color::rgb(0.698, 0.949, 0.733), // #b2f2bb
    Color::rgb(0.647, 0.847, 1.000), // #a5d8ff
    Color::rgb(1.000, 0.925, 0.600), // #ffec99
];

/// **A colour as the `#rrggbb` a hover tooltip shows**, or `#rrggbbaa` when it
/// is not fully opaque.
///
/// Six digits for the common case rather than eight always: the tooltip in the
/// reference reads `#b2f2bb`, and a user who wanted to copy that into another
/// tool would have to trim two characters off `#b2f2bbff` every time.
pub fn hex(color: Color) -> String {
    let byte = |value: f32| (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    let (r, g, b, a) = (byte(color.r), byte(color.g), byte(color.b), byte(color.a));
    if a == 255 {
        format!("#{r:02x}{g:02x}{b:02x}")
    } else {
        format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
    }
}

/// **The inverse**, so a typed colour is as good as a preset one.
///
/// Accepts `#rgb`, `#rrggbb` and `#rrggbbaa`, with or without the `#`, in
/// either case. `None` for anything else — including an empty string, which is
/// what a half-typed value looks like and must not be read as black.
pub fn parse_hex(text: &str) -> Option<Color> {
    let digits = text.trim().trim_start_matches('#');
    if !digits.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }

    let pair = |at: usize| u8::from_str_radix(digits.get(at..at + 2)?, 16).ok();
    let single = |at: usize| {
        let value = u8::from_str_radix(digits.get(at..at + 1)?, 16).ok()?;
        // `#abc` is `#aabbcc`, which is the shorthand every other tool uses:
        // the digit is repeated, not shifted, so `#fff` is white rather than
        // `#f0f0f0`.
        Some(value * 17)
    };

    match digits.len() {
        3 => Some(Color::from_rgba8(single(0)?, single(1)?, single(2)?, 255)),
        6 => Some(Color::from_rgba8(pair(0)?, pair(2)?, pair(4)?, 255)),
        8 => Some(Color::from_rgba8(pair(0)?, pair(2)?, pair(4)?, pair(6)?)),
        _ => None,
    }
}

// ---- what the panel reads back --------------------------------------------

/// **Every control's current position, read out of one style.**
///
/// One struct rather than fifteen calls at the render site, so "which button is
/// filled?" is answered once per frame from one style and every row asks the
/// same object. It also makes the round trip testable in one place: a panel that
/// cannot read its own writes shows nothing selected, which reads as a bug in
/// the drawing rather than as a missing inverse.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ControlState {
    pub width: StrokeWidthStep,
    pub dash: StrokeDashStep,
    pub sloppiness: Sloppiness,
    pub corners: CornerStyle,
    pub fill_style: FillStyle,
    pub font_family: FontFamily,
    pub font_size: FontSize,
    pub align: TextAlign,
    /// `0..=100`, which is what the slider's endpoints are labelled with.
    pub opacity_percent: u8,
    pub stroke: Option<Color>,
    pub background: Option<Color>,
    pub start_arrowhead: bool,
    pub end_arrowhead: bool,
}

impl ControlState {
    pub fn of(style: &ElementStyle) -> ControlState {
        ControlState {
            width: StrokeWidthStep::of(style.stroke.width),
            dash: StrokeDashStep::of(&style.stroke.dash),
            sloppiness: style.sloppiness,
            corners: CornerStyle::of(style.corner_radius),
            fill_style: style.fill_style,
            font_family: style.font.family,
            font_size: style.font.size,
            align: style.font.align,
            opacity_percent: percent_of(style.opacity),
            stroke: style.stroke.color,
            background: style.fill,
            start_arrowhead: style.start_marker != ArrowMarker::None,
            end_arrowhead: style.end_marker != ArrowMarker::None,
        }
    }
}

/// A `0.0..=1.0` opacity as the whole percent the slider shows.
pub fn percent_of(opacity: f32) -> u8 {
    (opacity.clamp(0.0, 1.0) * 100.0).round() as u8
}

/// And back. Separate from the multiplication so the two are one pair rather
/// than two conversions written at two call sites.
pub fn opacity_of(percent: u8) -> f32 {
    percent.min(100) as f32 / 100.0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The panel's table, written out again rather than read off the code
    /// above.**
    ///
    /// `●` where a section appears. Two independent statements of one fact, so
    /// a row that moves in one has to move in the other — which is the whole
    /// value of writing it twice, and the reason this test is a table and not a
    /// loop over `sections()`.
    #[test]
    fn each_kind_gets_exactly_the_sections_the_reference_specifies() {
        use PanelSection::*;

        let table: &[(PanelSection, [bool; 4])] = &[
            //                    node   edge   text   image
            (Stroke, [true, true, true, false]),
            (Background, [true, false, false, false]),
            (Fill, [true, false, false, false]),
            (StrokeWidth, [true, true, false, false]),
            (StrokeStyle, [true, true, false, false]),
            (Sloppiness_, [true, true, false, false]),
            (Corners, [true, false, false, true]),
            (ArrowType, [false, true, false, false]),
            (Arrowheads, [false, true, false, false]),
            (FontFamilyRow, [false, false, true, false]),
            (FontSizeRow, [false, false, true, false]),
            (TextAlignRow, [false, false, true, false]),
            (Opacity, [true, true, true, true]),
            (Layers, [true, true, true, true]),
            (Actions, [true, true, true, true]),
        ];

        for (column, kind) in SelectionKind::ALL.iter().enumerate() {
            let sections = kind.sections();
            for (section, columns) in table {
                assert_eq!(
                    sections.contains(section),
                    columns[column],
                    "{}'s {} row",
                    kind.name(),
                    section.name()
                );
            }
            assert_eq!(
                sections.len(),
                table.iter().filter(|(_, columns)| columns[column]).count(),
                "{} draws a row the table does not list",
                kind.name()
            );
        }
    }

    /// The panel's whole shape in one line: **Opacity, Layers and Actions are
    /// the only three sections on every form.**
    #[test]
    fn only_three_sections_are_on_every_panel() {
        let everywhere: Vec<PanelSection> = SelectionKind::ALL[0]
            .sections()
            .iter()
            .copied()
            .filter(|section| {
                SelectionKind::ALL
                    .iter()
                    .all(|kind| kind.sections().contains(section))
            })
            .collect();

        assert_eq!(
            everywhere,
            vec![
                PanelSection::Opacity,
                PanelSection::Layers,
                PanelSection::Actions
            ]
        );
    }

    /// The image panel is the minimal case, and it is minimal on purpose: none
    /// of stroke, background, fill, width, style or sloppiness means anything to
    /// a bitmap.
    #[test]
    fn the_image_panel_is_edges_opacity_layers_and_actions() {
        assert_eq!(
            SelectionKind::Image.sections(),
            &[
                PanelSection::Corners,
                PanelSection::Opacity,
                PanelSection::Layers,
                PanelSection::Actions
            ]
        );
    }

    /// Decision 1: a control that applies to half of what is selected is worse
    /// than a control that is not there.
    #[test]
    fn a_mixed_selection_gets_the_intersection_of_its_kinds() {
        assert_eq!(
            sections_for(&[SelectionKind::Node, SelectionKind::Text]),
            vec![
                PanelSection::Stroke,
                PanelSection::Opacity,
                PanelSection::Layers,
                PanelSection::Actions
            ]
        );
        assert_eq!(
            sections_for(&[SelectionKind::Node, SelectionKind::Image]),
            vec![
                PanelSection::Corners,
                PanelSection::Opacity,
                PanelSection::Layers,
                PanelSection::Actions
            ]
        );
        assert_eq!(
            sections_for(&[SelectionKind::Node]),
            SelectionKind::Node.sections().to_vec()
        );
        assert!(sections_for(&[]).is_empty());
    }

    /// **Every stepped control reads back what it wrote.** A panel that cannot
    /// is a panel with every button unselected, which looks like a drawing bug.
    #[test]
    fn every_stepped_control_round_trips_through_the_model() {
        for step in StrokeWidthStep::ALL {
            assert_eq!(StrokeWidthStep::of(step.width()), *step);
        }
        for step in StrokeDashStep::ALL {
            assert_eq!(StrokeDashStep::of(&step.pattern()), *step);
        }
        for step in CornerStyle::ALL {
            assert_eq!(CornerStyle::of(step.radius()), *step);
        }
        for kind in ArrowKind::ALL {
            assert_eq!(ArrowKind::of(kind.routing()), *kind);
        }
        for percent in 0..=100u8 {
            assert_eq!(percent_of(opacity_of(percent)), percent);
        }
    }

    /// The two routings with no button of their own must still read as
    /// *something*, or an edge that has one shows an Arrow type row with
    /// nothing selected.
    #[test]
    fn the_routings_with_no_button_read_as_their_nearest_one() {
        assert_eq!(ArrowKind::of(EdgeRouting::SimpleBezier), ArrowKind::Curved);
        assert_eq!(ArrowKind::of(EdgeRouting::SmoothStep), ArrowKind::Elbow);
    }

    /// A default style has to light up one button in every row — a fresh shape
    /// showing an empty panel is the same failure as a missing inverse.
    #[test]
    fn a_default_style_reads_as_a_position_in_every_row() {
        let state = ControlState::of(&ElementStyle::default());

        assert_eq!(state.width, StrokeWidthStep::Thin);
        assert_eq!(state.dash, StrokeDashStep::Solid);
        assert_eq!(state.corners, CornerStyle::Sharp);
        assert_eq!(state.fill_style, FillStyle::Solid);
        assert_eq!(state.sloppiness, Sloppiness::Artist);
        assert_eq!(state.opacity_percent, 100);
        assert!(!state.start_arrowhead && !state.end_arrowhead);
    }

    /// Toggling an arrowhead on must not replace a marker the document already
    /// names — a diamond head is still an arrowhead.
    #[test]
    fn toggling_an_arrowhead_keeps_a_marker_the_document_chose() {
        let mut style = ElementStyle {
            end_marker: ArrowMarker::Diamond,
            ..ElementStyle::default()
        };

        ArrowEnd::End.set(&mut style, false);
        assert_eq!(style.end_marker, ArrowMarker::None);
        ArrowEnd::End.set(&mut style, true);
        assert_eq!(style.end_marker, ArrowMarker::Arrow);

        style.end_marker = ArrowMarker::Diamond;
        ArrowEnd::End.set(&mut style, true);
        assert_eq!(style.end_marker, ArrowMarker::Diamond);
    }

    #[test]
    fn hex_renders_the_six_digit_form_unless_there_is_alpha() {
        assert_eq!(hex(BACKGROUND_SWATCHES[2]), "#b2f2bb");
        assert_eq!(hex(STROKE_SWATCHES[0]), "#1e1e1e");
        assert_eq!(hex(Color::TRANSPARENT), "#00000000");
    }

    #[test]
    fn hex_parses_the_three_forms_and_refuses_everything_else() {
        assert_eq!(parse_hex("#fff"), Some(Color::WHITE));
        assert_eq!(parse_hex("000"), Some(Color::BLACK));
        assert_eq!(
            parse_hex("#b2f2bb"),
            Some(Color::from_rgba8(178, 242, 187, 255))
        );
        assert_eq!(parse_hex("  #B2F2BB  "), parse_hex("#b2f2bb"));
        assert_eq!(parse_hex("#00000000"), Some(Color::from_rgba8(0, 0, 0, 0)));

        assert_eq!(parse_hex(""), None);
        assert_eq!(parse_hex("#"), None);
        assert_eq!(parse_hex("#12345"), None);
        assert_eq!(parse_hex("rebeccapurple"), None);
    }

    /// **Every preset swatch round trips through its own tooltip.** The tooltip
    /// is what a user copies out, and a hex that did not parse back would be a
    /// value the panel can show and not accept.
    #[test]
    fn every_preset_round_trips_through_its_hex() {
        for color in STROKE_SWATCHES.iter().chain(&BACKGROUND_SWATCHES) {
            let parsed = parse_hex(&hex(*color)).expect("a rendered hex parses");
            for (a, b) in [
                (parsed.r, color.r),
                (parsed.g, color.g),
                (parsed.b, color.b),
                (parsed.a, color.a),
            ] {
                assert!((a - b).abs() < 1.0 / 255.0, "{:?} became {parsed:?}", color);
            }
        }
    }

    /// Decision 2, as an assertion rather than as prose.
    #[test]
    fn sloppiness_is_unavailable_rather_than_absent_in_clean_mode() {
        assert!(Availability::of_sloppiness(true).is_live());
        assert_eq!(
            Availability::of_sloppiness(false),
            Availability::NeedsSketchMode
        );
        // The row is still on the panel; it is the control that is muted.
        assert!(
            SelectionKind::Node
                .sections()
                .contains(&PanelSection::Sloppiness_)
        );
    }

    /// Decision 3: Phase 12 registers a kind, it does not build a panel.
    #[test]
    fn an_image_element_already_maps_to_the_image_panel() {
        assert_eq!(
            SelectionKind::of_kind(&ElementKind::Image),
            SelectionKind::Image
        );
        assert_eq!(
            SelectionKind::of_kind(&ElementKind::Text),
            SelectionKind::Text
        );
        assert_eq!(
            SelectionKind::of_kind(&ElementKind::Shape(crate::models::ShapeKind::Ellipse)),
            SelectionKind::Node
        );
        assert_eq!(
            SelectionKind::of_shape(NodeShape::Text),
            SelectionKind::Text
        );
    }

    /// Names are element ids. Two rows sharing one is a GPUI state collision.
    #[test]
    fn no_two_sections_or_steps_share_a_name() {
        let mut names: Vec<&str> = SelectionKind::ALL
            .iter()
            .flat_map(|kind| kind.sections())
            .map(|section| section.name())
            .collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), 15);

        let mut buttons: Vec<&str> = StrokeWidthStep::ALL
            .iter()
            .map(|it| it.name())
            .chain(StrokeDashStep::ALL.iter().map(|it| it.name()))
            .chain(CornerStyle::ALL.iter().map(|it| it.name()))
            .chain(ArrowKind::ALL.iter().map(|it| it.name()))
            .chain(ArrowEnd::ALL.iter().map(|it| it.name()))
            .chain(
                SelectionKind::ALL
                    .iter()
                    .flat_map(|kind| ElementAction::for_kind(*kind))
                    .map(|it| it.name()),
            )
            .collect();
        buttons.sort_unstable();
        buttons.dedup();
        // The action rows of two kinds share their first three buttons, so the
        // list is deduplicated before it is counted; what must be unique is a
        // name meaning two different things, and `ElementAction::name` is one
        // function over one enum.
        let count = buttons.len();
        buttons.dedup();
        assert_eq!(buttons.len(), count, "two buttons share a name");
    }

    /// **The image row's fourth button**, stated on its own: Crop is an image's
    /// and nobody else's, and the three every panel has are still there.
    #[test]
    fn only_an_image_offers_crop() {
        assert_eq!(
            ElementAction::for_kind(SelectionKind::Image),
            &[
                ElementAction::Duplicate,
                ElementAction::Delete,
                ElementAction::Link,
                ElementAction::Crop,
            ]
        );

        for kind in [
            SelectionKind::Node,
            SelectionKind::Edge,
            SelectionKind::Text,
        ] {
            assert!(
                !ElementAction::for_kind(kind).contains(&ElementAction::Crop),
                "{kind:?} offers Crop"
            );
        }

        // And Edit points is still absent everywhere, because an edge still has
        // no waypoints — see `ElementAction::for_kind`.
        for kind in SelectionKind::ALL {
            assert!(!ElementAction::for_kind(*kind).contains(&ElementAction::EditPoints));
        }
    }

    /// **The Crop button's three states**, which are the whole of what "an
    /// action, not a mode" means here.
    #[test]
    fn crop_offers_the_frame_then_the_whole_picture_then_nothing() {
        // A 2:1 picture in a 2:1 frame, uncropped: there is nothing to do, and
        // the panel mutes the button rather than pretending.
        assert_eq!(crop_choice(2.0, 2.0, ImageCrop::FULL), None);

        // Stretched into a square frame: crop to it.
        assert_eq!(
            crop_choice(2.0, 1.0, ImageCrop::FULL),
            Some(CropChoice::ToFrame)
        );

        // Once cropped, the frame matches and the picture is partly hidden, so
        // the same button offers the whole picture back.
        let cropped = ImageCrop::FULL.cropped_to_aspect(2.0, 1.0);
        assert_eq!(
            crop_choice(2.0, 1.0, cropped),
            Some(CropChoice::Reset),
            "{cropped:?}"
        );

        // A frame within the tolerance of the shown aspect is not "stretched".
        // Without this, every aspect-locked drag would leave the button
        // offering to crop away a hundredth of a percent.
        assert_eq!(crop_choice(2.0, 2.0 * 1.001, ImageCrop::FULL), None);
    }

    /// A resource with no usable dimensions cannot be cropped, and answering
    /// `Some` would divide by it.
    #[test]
    fn crop_refuses_a_ratio_it_cannot_use() {
        assert_eq!(crop_choice(0.0, 1.0, ImageCrop::FULL), None);
        assert_eq!(crop_choice(f32::NAN, 1.0, ImageCrop::FULL), None);
        assert_eq!(crop_choice(1.0, f32::INFINITY, ImageCrop::FULL), None);
    }
}
