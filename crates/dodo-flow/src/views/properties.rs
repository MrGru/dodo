//! **The contextual property panel** — the surface that makes everything the
//! engine models editable.
//!
//! The vocabulary is [`crate::properties`]'s and the strings are
//! [`dodo_i18n::flow`]'s; what is here is the drawing, and three decisions that
//! only exist because there is a screen.
//!
//! # The glyphs are drawn by the engine that draws the property
//!
//! `views::palette` established this and it pays even better here: the
//! Sloppiness buttons are a straight line handed to
//! [`crate::render::sketch::perturb`] at each step's own
//! roughness, so **the button is the hand it selects** — change the generator
//! and the three samples change with it. The Fill buttons are the real
//! [`hatch`](mod@crate::render::hatch) generator over a small square, for the same
//! reason. Neither can drift from what it does, and neither needs an asset.
//!
//! The rest — corners, arrows, alignment, the layer arrows, the actions — are
//! hand-built [`Outline`]s of fractions of their button, exactly as the
//! palette's pointer and pan glyphs are, so they are crisp at any size and a
//! test can measure them.
//!
//! # Selected is a light fill, and a selected swatch is a ring
//!
//! Two states, and each is chosen rather than inherited. A selected *button*
//! takes a filled background; a selected *colour swatch* takes a thin outline
//! offset from it and no fill at all, because a swatch's whole job is to report
//! a colour and a highlight behind it would change the colour it reports.
//!
//! The colours are dodo's: `primary` at low opacity for the fill and `primary`
//! for the ring and the glyph, so the panel reads as part of whatever theme is
//! loaded rather than as a picture of another tool.
//!
//! It is deliberately *not* the palette strip's solid `primary` fill. A tool
//! button says "this is what the next press does" and wants to shout; a
//! property button says "this is what the selection is" and there are fifteen
//! rows of them.
//!
//! # Two controls need a text field, and they share one
//!
//! The colour swatch past the separator and the Link action both open a
//! single-line editor. One `Option` on the view rather than two, because they
//! can never be open at once and because the commit/cancel keys are the same —
//! `enter` and `escape` bubble up from `Input` exactly as they do for §9's
//! caret, which is the mechanism this borrows wholesale.

use dodo_i18n::{flow, t};
use gpui::{
    App, Bounds, Entity, Hsla, InteractiveElement, IntoElement, MouseButton, ParentElement, Pixels,
    SharedString, StatefulInteractiveElement, Styled, canvas, div, prelude::FluentBuilder, px,
};
use gpui_component::{
    ActiveTheme,
    input::{Input, InputState},
    slider::Slider,
    tooltip::Tooltip,
};

use crate::{
    commands::LayerAction,
    geometry::{Rect, Vec2},
    models::{
        Color, ElementStyle, FillStyle, FontFamily, FontSize, RenderQuality, SketchStyle,
        Sloppiness, TextAlign,
    },
    properties::{
        ArrowEnd, ArrowKind, Availability, BACKGROUND_SWATCHES, ControlState, CornerStyle,
        CropChoice, ElementAction, PanelSection, STROKE_SWATCHES, StrokeDashStep, StrokeWidthStep,
        hex,
    },
    render::{
        hatch,
        painter::{build_path, from_hsla, to_hsla},
        plan::PathPaint,
        shapes::{self, Outline},
        sketch,
    },
    views::{FlowView, flow::TYPING_CONTEXT, palette},
};

/// One control button's side, in screen pixels. Wider than the palette's,
/// because a line sample has to *read* as thin, bold or extra bold and 28 px of
/// box with 7 px of inset leaves 14 px to say it in.
const BUTTON_PIXELS: f32 = 32.0;

/// The inset a glyph keeps inside its button.
const GLYPH_INSET: f32 = 8.0;

/// A colour swatch's side. Smaller than a control button so a row of five plus
/// a separator plus the current colour fits the panel's width.
const SWATCH_PIXELS: f32 = 26.0;

/// The panel's width. Fixed rather than content-sized: every row is a fixed
/// number of fixed-width buttons, so a content-sized card would be as wide as
/// its widest row and the section labels would sit under a ragged edge.
pub const PANEL_PIXELS: f32 = 214.0;

/// The tallest the panel may get before it scrolls. It is **a single scrolling
/// column** rather than a wrapped or paged one: a node's panel is ten sections
/// and does not fit a short window, and a row that moved between columns as the
/// window resized would be a row nobody could find twice.
const MAX_PANEL_PIXELS: f32 = 620.0;

/// The stroke width the glyphs are drawn at, in screen pixels.
const GLYPH_STROKE: f32 = 1.4;

/// The tolerance the glyphs are flattened at — tighter than the canvas's
/// balanced default, for the reason `views::palette` gives: a 16 px glyph has
/// no room to hide a facet, and there are a few dozen of them once per frame.
const GLYPH_QUALITY: RenderQuality = RenderQuality::PRECISE;

/// The hand the three Sloppiness samples are drawn with.
///
/// A [`SketchStyle`] of the panel's own rather than the document's, so the
/// three buttons look the same whatever a particular drawing's roughness is —
/// they are showing the *steps*, and a document already set to Cartoonist must
/// not draw all three of them wildly.
const SAMPLE_HAND: SketchStyle = SketchStyle {
    roughness: 1.0,
    bowing: 2.0,
    stroke_count: 1,
    seed: 0x_C0FF_EE00_1234_5678,
    jitter: 1.6,
};

/// What the panel needs to know about the canvas to draw itself.
///
/// A struct rather than eight arguments, for the reason
/// [`crate::views::palette::PaletteState`] gives: a call
/// site full of positional `bool`s is how "has a link" and "can delete" end up
/// swapped with nothing to notice it.
#[derive(Debug, Clone, PartialEq)]
pub struct PanelState {
    /// The rows this selection gets, from [`crate::properties::sections_for`].
    /// Empty means no panel at all — the caller draws nothing.
    pub sections: Vec<PanelSection>,
    /// Every control's position, read out of the selection's style.
    pub controls: ControlState,
    /// The Arrow type row's position. Not part of [`ControlState`] because it
    /// comes from the edge's routing rather than from a style.
    pub arrow: ArrowKind,
    /// Whether the Sloppiness row can be used — see [`Availability`].
    pub sloppiness: Availability,
    /// Whether the selection carries a hyperlink, which is what fills the Link
    /// button.
    pub has_link: bool,
    /// **What the Crop button would do** (§10), or `None` when it would do
    /// nothing — in which case it is drawn muted with a tooltip that says why,
    /// exactly as Sloppiness is in clean mode. See
    /// [`crop_choice`](crate::properties::crop_choice).
    pub crop: Option<CropChoice>,
    /// **The Actions row's buttons**, from
    /// [`ElementAction::for_kind`](crate::properties::ElementAction::for_kind).
    ///
    /// Carried rather than recomputed in the render body, because it is the one
    /// row whose *list* depends on what is selected — an image gets Crop and
    /// nothing else does — and deciding that in a `render` is precisely what
    /// `crate::properties` exists to prevent.
    pub actions: &'static [ElementAction],
}

impl PanelState {
    pub fn is_empty(&self) -> bool {
        self.sections.is_empty()
    }
}

/// Which of the two single-line editors is open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptKind {
    /// The swatch past the Stroke row's separator.
    StrokeColor,
    /// The swatch past the Background row's separator.
    BackgroundColor,
    /// The Link action.
    Link,
}

impl PromptKind {
    pub fn placeholder(self) -> flow::Text {
        match self {
            PromptKind::StrokeColor | PromptKind::BackgroundColor => flow::Text::ColorPlaceholder,
            PromptKind::Link => flow::Text::LinkPlaceholder,
        }
    }
}

/// **The panel**, as an element the canvas positions over itself.
///
/// Takes the view entity rather than a `Context<FlowView>` for the same reason
/// the palette does: a click handler is handed an `&mut App`.
///
/// **[`occlude`](gpui::InteractiveElement::occlude) is what keeps the panel
/// open when it is used.** Without it every press on a control was delivered
/// twice — once here, applying the edit, and once to the canvas underneath,
/// where it landed on empty canvas, started a rubber band and committed an
/// empty replacing selection on the release. The selection is what the panel is
/// drawn *from*, so the panel vanished on the first press: the edit had already
/// been applied, which is why it looked like the panel closing rather than like
/// the press going somewhere else. See [`views::flow`](crate::views::flow)'s
/// module doc for the mechanism.
pub fn panel(
    view: Entity<FlowView>,
    state: &PanelState,
    prompt: Option<(PromptKind, &Entity<InputState>)>,
    opacity: &Entity<gpui_component::slider::SliderState>,
    cx: &App,
) -> impl IntoElement {
    let rows: Vec<gpui::AnyElement> = state
        .sections
        .iter()
        .map(|section| row(*section, state, view.clone(), opacity, cx))
        .collect();

    div()
        .occlude()
        .flex()
        .flex_col()
        .gap(px(4.0))
        .child(
            div()
                .id("flow-properties")
                .w(px(PANEL_PIXELS))
                .max_h(px(MAX_PANEL_PIXELS))
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .gap(px(12.0))
                .p(px(12.0))
                .rounded(cx.theme().radius)
                .border_1()
                .border_color(cx.theme().border)
                .bg(cx.theme().popover)
                .children(rows),
        )
        .children(prompt.map(|(kind, input)| prompt_row(kind, input, cx)))
}

/// The single-line editor the colour swatch and the Link action share.
///
/// A card of its own below the panel rather than a row inside it: the panel
/// scrolls, and an editor that scrolled out from under the cursor while
/// somebody was typing into it would be its own bug report.
///
/// **[`TYPING_CONTEXT`] is what makes it typable at all.** A hex code is
/// `#aabbcc` and a link is a sentence of letters; without this the canvas's
/// bare-letter bindings — which are on the dispatch path, because this element
/// is a descendant of the canvas's root — would consume `a`, `d`, `o`, `l`, `n`
/// and the rest as tool activations and take the focus away on the first one.
/// See [`TYPING_CONTEXT`]'s own doc for the whole diagnosis.
fn prompt_row(kind: PromptKind, input: &Entity<InputState>, cx: &App) -> impl IntoElement {
    div()
        .key_context(TYPING_CONTEXT)
        .w(px(PANEL_PIXELS))
        .p(px(6.0))
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().primary)
        .bg(cx.theme().popover)
        .child(Input::new(input).w_full())
        .id(match kind {
            PromptKind::Link => "flow-prompt-link",
            _ => "flow-prompt-color",
        })
}

/// One section: its label, and the controls under it.
fn row(
    section: PanelSection,
    state: &PanelState,
    view: Entity<FlowView>,
    opacity: &Entity<gpui_component::slider::SliderState>,
    cx: &App,
) -> gpui::AnyElement {
    let controls = &state.controls;
    let body: gpui::AnyElement = match section {
        PanelSection::Stroke => color_row(
            &STROKE_SWATCHES,
            controls.stroke,
            PromptKind::StrokeColor,
            view,
            cx,
        )
        .into_any_element(),
        PanelSection::Background => color_row(
            &BACKGROUND_SWATCHES,
            controls.background,
            PromptKind::BackgroundColor,
            view,
            cx,
        )
        .into_any_element(),
        PanelSection::Fill => choices(
            FillStyle::ALL,
            |it| *it == controls.fill_style,
            |it| PanelGlyph::Fill(*it),
            |it| match it {
                FillStyle::Hachure => flow::Text::FillHachure,
                FillStyle::CrossHatch => flow::Text::FillCrossHatch,
                FillStyle::Solid => flow::Text::FillSolid,
            },
            |it| Change::Fill(*it),
            Availability::Live,
            view,
            cx,
        ),
        PanelSection::StrokeWidth => choices(
            StrokeWidthStep::ALL,
            |it| *it == controls.width,
            |it| PanelGlyph::Width(*it),
            |it| match it {
                StrokeWidthStep::Thin => flow::Text::StrokeWidthThin,
                StrokeWidthStep::Bold => flow::Text::StrokeWidthBold,
                StrokeWidthStep::ExtraBold => flow::Text::StrokeWidthExtraBold,
            },
            |it| Change::Width(*it),
            Availability::Live,
            view,
            cx,
        ),
        PanelSection::StrokeStyle => choices(
            StrokeDashStep::ALL,
            |it| *it == controls.dash,
            |it| PanelGlyph::Dash(*it),
            |it| match it {
                StrokeDashStep::Solid => flow::Text::StrokeStyleSolid,
                StrokeDashStep::Dashed => flow::Text::StrokeStyleDashed,
                StrokeDashStep::Dotted => flow::Text::StrokeStyleDotted,
            },
            |it| Change::Dash(*it),
            Availability::Live,
            view,
            cx,
        ),
        PanelSection::Sloppiness_ => choices(
            Sloppiness::ALL,
            |it| *it == controls.sloppiness,
            |it| PanelGlyph::Sloppy(*it),
            |it| match it {
                Sloppiness::Architect => flow::Text::SloppinessArchitect,
                Sloppiness::Artist => flow::Text::SloppinessArtist,
                Sloppiness::Cartoonist => flow::Text::SloppinessCartoonist,
            },
            |it| Change::Sloppiness(*it),
            state.sloppiness,
            view,
            cx,
        ),
        PanelSection::Corners => choices(
            CornerStyle::ALL,
            |it| *it == controls.corners,
            |it| PanelGlyph::Corner(*it),
            |it| match it {
                CornerStyle::Sharp => flow::Text::EdgesSharp,
                CornerStyle::Round => flow::Text::EdgesRound,
            },
            |it| Change::Corners(*it),
            Availability::Live,
            view,
            cx,
        ),
        PanelSection::ArrowType => choices(
            ArrowKind::ALL,
            |it| *it == state.arrow,
            |it| PanelGlyph::Arrow(*it),
            |it| match it {
                ArrowKind::Straight => flow::Text::ArrowStraight,
                ArrowKind::Curved => flow::Text::ArrowCurved,
                ArrowKind::Elbow => flow::Text::ArrowElbow,
            },
            |it| Change::Arrow(*it),
            Availability::Live,
            view,
            cx,
        ),
        PanelSection::Arrowheads => {
            let (start, end) = (controls.start_arrowhead, controls.end_arrowhead);
            choices(
                ArrowEnd::ALL,
                move |it| match it {
                    ArrowEnd::Start => start,
                    ArrowEnd::End => end,
                },
                |it| PanelGlyph::Head(*it),
                |it| match it {
                    ArrowEnd::Start => flow::Text::ArrowheadStart,
                    ArrowEnd::End => flow::Text::ArrowheadEnd,
                },
                move |it| {
                    Change::Arrowhead(
                        *it,
                        !match it {
                            ArrowEnd::Start => start,
                            ArrowEnd::End => end,
                        },
                    )
                },
                Availability::Live,
                view,
                cx,
            )
        }
        PanelSection::FontFamilyRow => choices(
            FontFamily::ALL,
            |it| *it == controls.font_family,
            |it| PanelGlyph::Family(*it),
            |it| match it {
                FontFamily::HandDrawn => flow::Text::FontHandDrawn,
                FontFamily::Normal => flow::Text::FontNormal,
                FontFamily::Code => flow::Text::FontCode,
            },
            |it| Change::Family(*it),
            Availability::Live,
            view,
            cx,
        ),
        PanelSection::FontSizeRow => font_size_row(controls.font_size, view, cx),
        PanelSection::TextAlignRow => choices(
            TextAlign::ALL,
            |it| *it == controls.align,
            |it| PanelGlyph::Align(*it),
            |it| match it {
                TextAlign::Left => flow::Text::AlignLeft,
                TextAlign::Center => flow::Text::AlignCenter,
                TextAlign::Right => flow::Text::AlignRight,
            },
            |it| Change::Align(*it),
            Availability::Live,
            view,
            cx,
        ),
        PanelSection::Opacity => opacity_row(opacity, cx),
        PanelSection::Layers => choices(
            LayerAction::ALL,
            |_| false,
            |it| PanelGlyph::Layer(*it),
            |it| match it {
                LayerAction::SendToBack => flow::Text::LayerSendToBack,
                LayerAction::SendBackward => flow::Text::LayerSendBackward,
                LayerAction::BringForward => flow::Text::LayerBringForward,
                LayerAction::BringToFront => flow::Text::LayerBringToFront,
            },
            |it| Change::Layer(*it),
            Availability::Live,
            view,
            cx,
        ),
        // **The one row whose button list depends on the selection's kind**
        // (Phase 12): an image gets a fourth, Crop. The list is
        // `ElementAction::for_kind`'s and is not restated here, for the reason
        // the whole of `crate::properties` exists.
        PanelSection::Actions => {
            let linked = state.has_link;
            let crop = state.crop;
            div()
                .flex()
                .items_center()
                .gap(px(6.0))
                .children(state.actions.iter().map(|action| {
                    let (label, availability) = match action {
                        ElementAction::Duplicate => {
                            (flow::Text::ActionDuplicate, Availability::Live)
                        }
                        ElementAction::Delete => (flow::Text::Delete, Availability::Live),
                        // Two labels and a muted third state, all from one
                        // pure answer — see `properties::crop_choice`.
                        ElementAction::Crop => match crop {
                            Some(CropChoice::ToFrame) => {
                                (flow::Text::ActionCropToFrame, Availability::Live)
                            }
                            Some(CropChoice::Reset) => {
                                (flow::Text::ActionCropWhole, Availability::Live)
                            }
                            None => (flow::Text::CropNeedsFrame, Availability::NeedsSketchMode),
                        },
                        _ => (flow::Text::ActionLink, Availability::Live),
                    };
                    button(
                        PanelGlyph::Action(*action),
                        linked && *action == ElementAction::Link,
                        availability,
                        label,
                        Change::Action(*action),
                        view.clone(),
                        cx,
                    )
                }))
                .into_any_element()
        }
    };

    div()
        .flex()
        .flex_col()
        .gap(px(6.0))
        .child(
            div()
                .text_size(px(11.0))
                .text_color(cx.theme().muted_foreground)
                .child(t(label_for(section), cx)),
        )
        .child(body)
        .into_any_element()
}

/// One section's translated label.
///
/// A `match` here rather than a method on [`PanelSection`], because that type
/// is below the UI-framework line and naming a catalogue there would put a
/// *string* in the pure layer — the same boundary `views::palette` crosses the
/// same way.
fn label_for(section: PanelSection) -> flow::Text {
    match section {
        PanelSection::Stroke => flow::Text::SectionStroke,
        PanelSection::Background => flow::Text::SectionBackground,
        PanelSection::Fill => flow::Text::SectionFill,
        PanelSection::StrokeWidth => flow::Text::SectionStrokeWidth,
        PanelSection::StrokeStyle => flow::Text::SectionStrokeStyle,
        PanelSection::Sloppiness_ => flow::Text::SectionSloppiness,
        PanelSection::Corners => flow::Text::SectionEdges,
        PanelSection::ArrowType => flow::Text::SectionArrowType,
        PanelSection::Arrowheads => flow::Text::SectionArrowheads,
        PanelSection::FontFamilyRow => flow::Text::SectionFontFamily,
        PanelSection::FontSizeRow => flow::Text::SectionFontSize,
        PanelSection::TextAlignRow => flow::Text::SectionTextAlign,
        PanelSection::Opacity => flow::Text::SectionOpacity,
        PanelSection::Layers => flow::Text::SectionLayers,
        PanelSection::Actions => flow::Text::SectionActions,
    }
}

/// One row's write, boxed so that fifteen different closures have one type.
///
/// A named alias rather than the type spelled out, because it appears in a
/// signature, in a test and in the view — and `Option<Box<dyn Fn(&mut
/// ElementStyle) + '_>>` read three times is three chances to get the lifetime
/// wrong.
pub type StyleEdit<'a> = Box<dyn Fn(&mut ElementStyle) + 'a>;

/// **Every edit a panel button makes**, as one enum.
///
/// One type rather than a closure per button, so that
/// [`FlowView::apply_panel_change`](crate::views::FlowView::apply_panel_change)
/// is the single place a press becomes a command — and so that a button which
/// forgot to be wired is a missing `match` arm rather than a control that does
/// nothing.
#[derive(Debug, Clone, PartialEq)]
pub enum Change {
    Stroke(Option<Color>),
    Background(Option<Color>),
    Fill(FillStyle),
    Width(StrokeWidthStep),
    Dash(StrokeDashStep),
    Sloppiness(Sloppiness),
    Corners(CornerStyle),
    Arrow(ArrowKind),
    Arrowhead(ArrowEnd, bool),
    Family(FontFamily),
    Size(FontSize),
    Align(TextAlign),
    /// A whole-number percent, `0..=100`.
    Opacity(u8),
    Layer(LayerAction),
    Action(ElementAction),
    /// Open one of the two single-line editors.
    Prompt(PromptKind),
}

impl Change {
    /// **The style write this change is**, or `None` for a change that is not a
    /// style at all (a layer press, an action, a prompt).
    ///
    /// Splitting it here rather than in the view is what keeps
    /// `restyle_selection`'s closure honest: everything in this arm is one
    /// field of one [`ElementStyle`], so a mixed selection gets the same write
    /// on every element and the whole thing is one undo step.
    pub fn as_style_edit(&self) -> Option<StyleEdit<'_>> {
        match self {
            Change::Stroke(color) => Some(Box::new(move |style: &mut ElementStyle| {
                style.stroke.color = *color
            })),
            Change::Background(color) => Some(Box::new(move |style: &mut ElementStyle| {
                style.fill = *color
            })),
            Change::Fill(fill) => Some(Box::new(move |style: &mut ElementStyle| {
                style.fill_style = *fill
            })),
            Change::Width(step) => Some(Box::new(move |style: &mut ElementStyle| {
                style.stroke.width = step.width()
            })),
            Change::Dash(step) => Some(Box::new(move |style: &mut ElementStyle| {
                style.stroke.dash = step.pattern()
            })),
            Change::Sloppiness(step) => Some(Box::new(move |style: &mut ElementStyle| {
                style.sloppiness = *step
            })),
            Change::Corners(step) => Some(Box::new(move |style: &mut ElementStyle| {
                style.corner_radius = step.radius()
            })),
            Change::Arrowhead(end, on) => Some(Box::new(move |style: &mut ElementStyle| {
                end.set(style, *on)
            })),
            Change::Family(family) => Some(Box::new(move |style: &mut ElementStyle| {
                style.font.family = *family
            })),
            Change::Size(size) => Some(Box::new(move |style: &mut ElementStyle| {
                style.font.size = *size
            })),
            Change::Align(align) => Some(Box::new(move |style: &mut ElementStyle| {
                style.font.align = *align
            })),
            Change::Opacity(percent) => Some(Box::new(move |style: &mut ElementStyle| {
                style.opacity = crate::properties::opacity_of(*percent)
            })),
            Change::Arrow(_) | Change::Layer(_) | Change::Action(_) | Change::Prompt(_) => None,
        }
    }
}

/// A row of buttons over one enum: the glyph, the tooltip, the selected test
/// and the change each one makes.
///
/// Generic over the value type so that a row cannot be given the wrong list —
/// the four functions all take the same `&T`, and the compiler checks that the
/// glyph, the label and the change all describe the same button.
#[allow(clippy::too_many_arguments)]
fn choices<T>(
    values: &'static [T],
    selected: impl Fn(&T) -> bool,
    glyph: impl Fn(&T) -> PanelGlyph,
    label: impl Fn(&T) -> flow::Text,
    change: impl Fn(&T) -> Change,
    availability: Availability,
    view: Entity<FlowView>,
    cx: &App,
) -> gpui::AnyElement {
    div()
        .flex()
        .items_center()
        .gap(px(6.0))
        .children(values.iter().map(|value| {
            button(
                glyph(value),
                selected(value),
                availability,
                label(value),
                change(value),
                view.clone(),
                cx,
            )
        }))
        .into_any_element()
}

/// The four size buttons, which are the one row whose glyph is a *letter*.
///
/// The four sizes are drawn *as* the letters S, M, L and XL, and as text rather
/// than as an outline: a letterform built from cubics would be a worse `S` than
/// the theme's own font already is. The letters come from the catalogue for the
/// reason its doc gives.
fn font_size_row(current: FontSize, view: Entity<FlowView>, cx: &App) -> gpui::AnyElement {
    div()
        .flex()
        .items_center()
        .gap(px(6.0))
        .children(FontSize::ALL.iter().map(|size| {
            let (selected, ink, fill) = states(*size == current, Availability::Live, cx);
            let change = Change::Size(*size);
            let view = view.clone();
            div()
                .id(SharedString::from(format!("flow-size-{}", size.name())))
                .size(px(BUTTON_PIXELS))
                .flex()
                .items_center()
                .justify_center()
                .rounded(cx.theme().radius)
                .bg(fill)
                .text_color(ink)
                .text_size(px(match size {
                    FontSize::Small => 11.0,
                    FontSize::Medium => 13.0,
                    FontSize::Large => 15.0,
                    FontSize::ExtraLarge => 15.0,
                }))
                .child(t(size_label(*size), cx))
                .tooltip({
                    let label = size_label(*size);
                    move |window, cx| Tooltip::new(t(label.clone(), cx)).build(window, cx)
                })
                .on_mouse_down(MouseButton::Left, move |_, window, cx| {
                    let change = change.clone();
                    view.update(cx, |this, cx| this.apply_panel_change(change, window, cx));
                })
                .when(selected, |this| this)
        }))
        .into_any_element()
}

fn size_label(size: FontSize) -> flow::Text {
    match size {
        FontSize::Small => flow::Text::FontSizeSmall,
        FontSize::Medium => flow::Text::FontSizeMedium,
        FontSize::Large => flow::Text::FontSizeLarge,
        FontSize::ExtraLarge => flow::Text::FontSizeExtraLarge,
    }
}

/// The opacity slider, with its two endpoints labelled.
///
/// **The endpoint labels are numerals rather than catalogue entries**, and that
/// is the one deliberate exception in this file: `0` and `100` are the slider's
/// own bounds rendered as numbers, in the same class as a row count or a
/// coordinate. They are formatted from the bounds, so a slider given a
/// different range labels itself correctly rather than lying in two languages.
fn opacity_row(slider: &Entity<gpui_component::slider::SliderState>, cx: &App) -> gpui::AnyElement {
    let endpoint = |value: u8| {
        div()
            .text_size(px(10.0))
            .text_color(cx.theme().muted_foreground)
            .child(SharedString::from(value.to_string()))
    };

    div()
        .flex()
        .flex_col()
        .gap(px(2.0))
        .child(Slider::new(slider).w_full())
        .child(
            div()
                .flex()
                .justify_between()
                .child(endpoint(0))
                .child(endpoint(100)),
        )
        .into_any_element()
}

/// A colour row: five presets, a hairline, and the current colour past it.
fn color_row(
    swatches: &'static [Color; 5],
    current: Option<Color>,
    prompt: PromptKind,
    view: Entity<FlowView>,
    cx: &App,
) -> gpui::AnyElement {
    let stroke = prompt == PromptKind::StrokeColor;
    let change = |color: Color| {
        if stroke {
            Change::Stroke(Some(color))
        } else {
            Change::Background(Some(color))
        }
    };

    div()
        .flex()
        .items_center()
        .gap(px(5.0))
        .children(swatches.iter().map(|color| {
            swatch(
                *color,
                current == Some(*color),
                SharedString::from(format!("flow-{}-{}", prompt_name(prompt), hex(*color))),
                Some(change(*color)),
                view.clone(),
                cx,
            )
        }))
        .child(
            div()
                .w(px(1.0))
                .h(px(SWATCH_PIXELS - 8.0))
                .mx(px(3.0))
                .bg(cx.theme().border),
        )
        // **The current colour, apart, and it is a control rather than a
        // readout**: pressing it opens the hex editor, which is what makes the
        // panel's colours genuinely editable rather than a choice of five.
        .child(swatch(
            current.unwrap_or(Color::TRANSPARENT),
            false,
            SharedString::from(format!("flow-{}-current", prompt_name(prompt))),
            Some(Change::Prompt(prompt)),
            view,
            cx,
        ))
        .into_any_element()
}

fn prompt_name(prompt: PromptKind) -> &'static str {
    match prompt {
        PromptKind::StrokeColor => "stroke",
        PromptKind::BackgroundColor => "background",
        PromptKind::Link => "link",
    }
}

/// One colour swatch.
///
/// **Selected is a ring, not a fill**: a thin outline offset from the swatch,
/// which is the only way to show "this one" on a control whose whole job is to
/// show a colour. A filled highlight behind it would change the colour the
/// swatch is reporting.
///
/// **Transparent is a checkerboard**, drawn as four quarters rather than left
/// empty — an empty square is what a missing swatch looks like.
fn swatch(
    color: Color,
    selected: bool,
    id: SharedString,
    change: Option<Change>,
    view: Entity<FlowView>,
    cx: &App,
) -> impl IntoElement {
    let label = if color.a <= 0.0 && change.is_none() {
        t(flow::Text::ColorFromTheme, cx)
    } else {
        SharedString::from(hex(color))
    };

    div()
        .id(id)
        .size(px(SWATCH_PIXELS))
        .p(px(2.0))
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(if selected {
            cx.theme().primary
        } else {
            gpui::transparent_black()
        })
        .child(
            div()
                .size_full()
                .rounded(px(3.0))
                .when(color.a > 0.0, |this| this.bg(to_hsla(color)))
                .when(color.a <= 0.0, |this| this.child(checkerboard(cx))),
        )
        .tooltip(move |window, cx| Tooltip::new(label.clone()).build(window, cx))
        .when_some(change, |this, change| {
            this.on_mouse_down(MouseButton::Left, move |_, window, cx| {
                let change = change.clone();
                view.update(cx, |this, cx| this.apply_panel_change(change, window, cx));
            })
        })
}

/// The transparent swatch's checkerboard: four quarters, two of them tinted.
fn checkerboard(cx: &App) -> impl IntoElement {
    let tint = cx.theme().muted_foreground.opacity(0.35);
    let quarter = |on: bool| div().w(px(9.0)).h(px(9.0)).when(on, |this| this.bg(tint));

    div()
        .size_full()
        .flex()
        .flex_wrap()
        .child(quarter(true))
        .child(quarter(false))
        .child(quarter(false))
        .child(quarter(true))
}

/// The three colours a button is drawn in, given its state.
///
/// One function so the selected look is decided once — see the module doc for
/// why it is a light fill rather than the palette strip's solid one.
fn states(selected: bool, availability: Availability, cx: &App) -> (bool, Hsla, Hsla) {
    let live = availability.is_live();
    let ink = match (selected, live) {
        (_, false) => cx.theme().muted_foreground.opacity(0.5),
        (true, true) => cx.theme().primary,
        (false, true) => cx.theme().foreground,
    };
    let fill = if selected && live {
        cx.theme().primary.opacity(0.16)
    } else {
        cx.theme().secondary.opacity(0.5)
    };
    (selected, ink, fill)
}

/// One control button.
#[allow(clippy::too_many_arguments)]
fn button(
    glyph: PanelGlyph,
    selected: bool,
    availability: Availability,
    label: flow::Text,
    change: Change,
    view: Entity<FlowView>,
    cx: &App,
) -> impl IntoElement {
    let (_, ink, fill) = states(selected, availability, cx);
    let live = availability.is_live();

    div()
        .id(SharedString::from(format!("flow-prop-{}", glyph.name())))
        .size(px(BUTTON_PIXELS))
        .flex()
        .items_center()
        .justify_center()
        .rounded(cx.theme().radius)
        .bg(fill)
        .child(paint(glyph, ink))
        .tooltip(move |window, cx| {
            // A disabled control says *why* it is disabled. See
            // `properties::Availability`: a muted button with no explanation is
            // indistinguishable from a broken one.
            let text = if live {
                t(label.clone(), cx)
            } else {
                t(flow::Text::SloppinessNeedsSketch, cx)
            };
            Tooltip::new(text).build(window, cx)
        })
        // A mouse-down rather than a click, for the reason `views::palette`
        // gives. What stops the press also reaching the canvas is the
        // `occlude()` on the panel — see `panel`.
        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
            if !live {
                return;
            }
            let change = change.clone();
            view.update(cx, |this, cx| this.apply_panel_change(change, window, cx));
        })
}

// ---- the glyphs -----------------------------------------------------------

/// Everything the panel draws, as one enum — the same shape
/// `views::palette::Glyph` has and for the same reason: `strokes` stays the
/// single place a button's picture is decided, and a test can walk every button
/// there is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelGlyph {
    Width(StrokeWidthStep),
    Dash(StrokeDashStep),
    Sloppy(Sloppiness),
    Corner(CornerStyle),
    Fill(FillStyle),
    Arrow(ArrowKind),
    Head(ArrowEnd),
    Family(FontFamily),
    Align(TextAlign),
    Layer(LayerAction),
    Action(ElementAction),
}

impl PanelGlyph {
    /// A short, stable name for the element id. **Not user-facing.**
    pub fn name(self) -> String {
        match self {
            PanelGlyph::Width(it) => format!("width-{}", it.name()),
            PanelGlyph::Dash(it) => format!("dash-{}", it.name()),
            PanelGlyph::Sloppy(it) => format!("sloppy-{}", it.name()),
            PanelGlyph::Corner(it) => format!("corner-{}", it.name()),
            PanelGlyph::Fill(it) => format!("fill-{}", it.name()),
            PanelGlyph::Arrow(it) => format!("arrow-{}", it.name()),
            PanelGlyph::Head(it) => it.name().to_owned(),
            PanelGlyph::Family(it) => format!("font-{}", it.name()),
            PanelGlyph::Align(it) => format!("align-{}", it.name()),
            PanelGlyph::Layer(it) => it.name().to_owned(),
            PanelGlyph::Action(it) => format!("action-{}", it.name()),
        }
    }

    /// Every glyph there is, for the tests. Built from each control's own `ALL`
    /// so there is no second list to keep in step.
    #[cfg(test)]
    pub fn every() -> Vec<PanelGlyph> {
        StrokeWidthStep::ALL
            .iter()
            .map(|it| PanelGlyph::Width(*it))
            .chain(StrokeDashStep::ALL.iter().map(|it| PanelGlyph::Dash(*it)))
            .chain(Sloppiness::ALL.iter().map(|it| PanelGlyph::Sloppy(*it)))
            .chain(CornerStyle::ALL.iter().map(|it| PanelGlyph::Corner(*it)))
            .chain(FillStyle::ALL.iter().map(|it| PanelGlyph::Fill(*it)))
            .chain(ArrowKind::ALL.iter().map(|it| PanelGlyph::Arrow(*it)))
            .chain(ArrowEnd::ALL.iter().map(|it| PanelGlyph::Head(*it)))
            .chain(FontFamily::ALL.iter().map(|it| PanelGlyph::Family(*it)))
            .chain(TextAlign::ALL.iter().map(|it| PanelGlyph::Align(*it)))
            .chain(LayerAction::ALL.iter().map(|it| PanelGlyph::Layer(*it)))
            .chain(
                ElementAction::for_kind(crate::properties::SelectionKind::Node)
                    .iter()
                    .map(|it| PanelGlyph::Action(*it)),
            )
            .collect()
    }
}

/// One glyph, painted rather than laid out. See `views::palette::paint`.
fn paint(glyph: PanelGlyph, ink: Hsla) -> impl IntoElement {
    let color = from_hsla(ink);
    canvas(
        |_, _, _| (),
        move |bounds: Bounds<Pixels>, (), window, _| {
            for (outline, paint) in strokes(glyph, inset(bounds), color) {
                if let Some(path) = build_path(&outline, paint, GLYPH_QUALITY.flattening_tolerance)
                {
                    window.paint_path(path, to_hsla(paint.color()));
                }
            }
        },
    )
    .size(px(BUTTON_PIXELS))
}

/// The glyph's box: the button's bounds, inset and left in window coordinates.
/// See `views::palette::inset` for why nothing is subtracted.
fn inset(bounds: Bounds<Pixels>) -> Rect {
    let origin = Vec2::new(
        bounds.origin.x.as_f32() + GLYPH_INSET,
        bounds.origin.y.as_f32() + GLYPH_INSET,
    );
    Rect::new(
        origin,
        Vec2::new(
            (bounds.size.width.as_f32() - GLYPH_INSET * 2.0).max(1.0),
            (bounds.size.height.as_f32() - GLYPH_INSET * 2.0).max(1.0),
        ),
    )
}

/// **Every path one glyph is made of.**
///
/// A `Vec` per button per frame, for the reason `views::palette::strokes` gives:
/// the canvas's no-allocation rules are about its per-element loops, not about a
/// panel that exists once.
fn strokes(glyph: PanelGlyph, box_: Rect, ink: Color) -> Vec<(Outline, PathPaint)> {
    let stroke = |width: f32| PathPaint::Stroke { color: ink, width };
    let thin = stroke(GLYPH_STROKE);
    let fill = PathPaint::Fill(ink);
    let (o, s) = (box_.origin, box_.size);
    let at = move |x: f32, y: f32| Vec2::new(o.x + s.x * x, o.y + s.y * y);

    match glyph {
        // A level line at the step's own weight. The three weights are the
        // control's whole message, so they are exaggerated against the world
        // widths — 1, 2 and 4 world units are 1, 2 and 4 pixels at 100 % zoom
        // and three near-identical hairlines in a 16 px glyph.
        PanelGlyph::Width(step) => {
            let mut line = Outline::with_capacity(2);
            line.move_to(at(0.0, 0.5)).line_to(at(1.0, 0.5));
            let width = match step {
                StrokeWidthStep::Thin => 1.2,
                StrokeWidthStep::Bold => 2.4,
                StrokeWidthStep::ExtraBold => 4.0,
            };
            vec![(line, stroke(width))]
        }

        // The dash pattern as separate subpaths rather than as a dashed paint:
        // `PathPaint::Stroke` has no dash, and drawing the segments is what the
        // sample is anyway.
        PanelGlyph::Dash(step) => {
            let segments: &[(f32, f32)] = match step {
                StrokeDashStep::Solid => &[(0.0, 1.0)],
                StrokeDashStep::Dashed => &[(0.0, 0.22), (0.39, 0.61), (0.78, 1.0)],
                StrokeDashStep::Dotted => &[
                    (0.0, 0.09),
                    (0.23, 0.32),
                    (0.46, 0.55),
                    (0.68, 0.77),
                    (0.91, 1.0),
                ],
            };
            let mut line = Outline::with_capacity(segments.len() * 2);
            for (from, to) in segments {
                line.move_to(at(*from, 0.5)).line_to(at(*to, 0.5));
            }
            vec![(line, stroke(2.0))]
        }

        // **The sample is the generator.** A straight line handed to §13's hand
        // at this step's own roughness, so the three buttons cannot drift from
        // the three things they select.
        PanelGlyph::Sloppy(step) => {
            let mut line = Outline::with_capacity(2);
            line.move_to(at(0.0, 0.55)).line_to(at(1.0, 0.45));
            let hand = SketchStyle {
                roughness: SAMPLE_HAND.roughness * step.roughness_scale(),
                ..SAMPLE_HAND
            };
            vec![(
                sketch::perturb(&line, &hand, SAMPLE_HAND.seed, 0),
                stroke(1.8),
            )]
        }

        // A corner bracket, solid on two sides and dotted on the other two.
        // The dots are what make it read as a corner *of something* rather than
        // as an L.
        PanelGlyph::Corner(step) => {
            let mut corner = Outline::with_capacity(4);
            corner.move_to(at(0.0, 0.62));
            match step {
                CornerStyle::Sharp => {
                    corner.line_to(at(0.0, 0.0)).line_to(at(0.62, 0.0));
                }
                CornerStyle::Round => {
                    corner
                        .line_to(at(0.0, 0.24))
                        .cubic_to(at(0.0, 0.0), at(0.0, 0.0), at(0.24, 0.0))
                        .line_to(at(0.62, 0.0));
                }
            }

            let mut dots = Outline::with_capacity(12);
            for step in 0..3 {
                let t = 0.3 + step as f32 * 0.3;
                dots.move_to(at(1.0, t)).line_to(at(1.0, t + 0.08));
                dots.move_to(at(t, 1.0)).line_to(at(t + 0.08, 1.0));
            }
            dots.move_to(at(1.0, 0.0)).line_to(at(1.0, 0.08));
            dots.move_to(at(0.0, 1.0)).line_to(at(0.08, 1.0));

            vec![(corner, stroke(1.8)), (dots, stroke(1.4))]
        }

        // **The sample is the generator**, again: the real hatch lines over a
        // small square, so a change to the spacing or the angle shows up on the
        // button that selects it.
        PanelGlyph::Fill(style) => {
            let square = shapes::rectangle(box_);
            match style {
                FillStyle::Solid => vec![(square, fill)],
                _ => {
                    let lines = hatch::hatch(&square, style, box_.size.x * 0.34);
                    vec![(square, thin), (lines, stroke(1.2))]
                }
            }
        }

        PanelGlyph::Arrow(kind) => {
            let mut line = Outline::with_capacity(4);
            match kind {
                ArrowKind::Straight => {
                    line.move_to(at(0.0, 1.0)).line_to(at(0.86, 0.14));
                }
                ArrowKind::Curved => {
                    line.move_to(at(0.0, 1.0)).cubic_to(
                        at(0.06, 0.34),
                        at(0.42, 0.14),
                        at(0.8, 0.28),
                    );
                }
                ArrowKind::Elbow => {
                    line.move_to(at(0.06, 1.0))
                        .line_to(at(0.06, 0.2))
                        .line_to(at(0.86, 0.2));
                }
            }
            let head = match kind {
                ArrowKind::Straight => arrowhead(at(0.86, 0.14), at(0.0, 1.0), s.x * 0.3),
                ArrowKind::Curved => arrowhead(at(0.8, 0.28), at(0.42, 0.14), s.x * 0.26),
                ArrowKind::Elbow => arrowhead(at(0.86, 0.2), at(0.06, 0.2), s.x * 0.3),
            };
            vec![(line, stroke(1.6)), (head, stroke(1.6))]
        }

        // Which *end* the toggle belongs to, drawn as a line with the head on
        // that end. Whether the head is *on* is the button's selected state —
        // one picture, two states, which is what a toggle is.
        PanelGlyph::Head(end) => {
            let mut line = Outline::with_capacity(2);
            line.move_to(at(0.06, 0.5)).line_to(at(0.94, 0.5));
            let head = match end {
                ArrowEnd::Start => arrowhead(at(0.06, 0.5), at(0.94, 0.5), s.x * 0.36),
                ArrowEnd::End => arrowhead(at(0.94, 0.5), at(0.06, 0.5), s.x * 0.36),
            };
            vec![(line, stroke(1.6)), (head, stroke(1.6))]
        }

        PanelGlyph::Family(family) => match family {
            // A pencil: a slanted shaft with a point at the bottom-left.
            FontFamily::HandDrawn => {
                let mut pencil = Outline::with_capacity(6);
                pencil
                    .move_to(at(0.0, 1.0))
                    .line_to(at(0.16, 0.72))
                    .line_to(at(0.86, 0.0))
                    .line_to(at(1.0, 0.16))
                    .line_to(at(0.3, 0.88))
                    .close();
                vec![(pencil, stroke(1.5))]
            }
            FontFamily::Normal => vec![(palette::text_glyph(box_), stroke(1.5))],
            // `</>`, which is what every editor uses for this.
            FontFamily::Code => {
                let mut marks = Outline::with_capacity(8);
                marks
                    .move_to(at(0.3, 0.16))
                    .line_to(at(0.0, 0.5))
                    .line_to(at(0.3, 0.84))
                    .move_to(at(0.7, 0.16))
                    .line_to(at(1.0, 0.5))
                    .line_to(at(0.7, 0.84))
                    .move_to(at(0.58, 0.06))
                    .line_to(at(0.42, 0.94));
                vec![(marks, stroke(1.5))]
            }
        },

        // Three lines of a paragraph, the middle one short, pushed to whichever
        // side the alignment names.
        PanelGlyph::Align(align) => {
            let mut lines = Outline::with_capacity(8);
            for (index, y) in [0.08_f32, 0.42, 0.76].into_iter().enumerate() {
                let length = if index == 1 { 0.62 } else { 1.0 };
                let from = match align {
                    TextAlign::Left => 0.0,
                    TextAlign::Center => (1.0 - length) * 0.5,
                    TextAlign::Right => 1.0 - length,
                };
                lines.move_to(at(from, y)).line_to(at(from + length, y));
            }
            vec![(lines, stroke(1.8))]
        }

        // An arrow, plus a bar at the end it travels to for the two that go all
        // the way. The bar is what tells "to back" from "backward" at 16 px.
        PanelGlyph::Layer(action) => {
            let down = matches!(action, LayerAction::SendToBack | LayerAction::SendBackward);
            let (tail, tip) = if down {
                (at(0.5, 0.08), at(0.5, 0.82))
            } else {
                (at(0.5, 0.92), at(0.5, 0.18))
            };

            let mut shaft = Outline::with_capacity(2);
            shaft.move_to(tail).line_to(tip);
            let mut paths = vec![
                (shaft, stroke(1.6)),
                (arrowhead(tip, tail, s.x * 0.34), stroke(1.6)),
            ];

            if matches!(action, LayerAction::SendToBack | LayerAction::BringToFront) {
                let y = if down { 1.0 } else { 0.0 };
                let mut bar = Outline::with_capacity(2);
                bar.move_to(at(0.06, y)).line_to(at(0.94, y));
                paths.push((bar, stroke(1.8)));
            }
            paths
        }

        PanelGlyph::Action(action) => match action {
            // Two offset rounded squares — the universal copy mark.
            ElementAction::Duplicate => {
                let back = Rect::new(o + Vec2::new(s.x * 0.24, 0.0), s * 0.76);
                let front = Rect::new(o + Vec2::new(0.0, s.y * 0.24), s * 0.76);
                vec![
                    (shapes::rounded_rectangle(back, s.x * 0.14), thin),
                    (shapes::rounded_rectangle(front, s.x * 0.14), thin),
                ]
            }
            ElementAction::Delete => palette::trash_glyph(box_)
                .map(|outline| (outline, thin))
                .collect(),
            // A chain of two links: two rounded ends and the bar between them.
            _ => {
                let mut chain = Outline::with_capacity(10);
                chain
                    .move_to(at(0.38, 0.28))
                    .cubic_to(at(0.62, 0.0), at(1.0, 0.16), at(0.86, 0.44))
                    .line_to(at(0.72, 0.58))
                    .move_to(at(0.62, 0.72))
                    .cubic_to(at(0.38, 1.0), at(0.0, 0.84), at(0.14, 0.56))
                    .line_to(at(0.28, 0.42))
                    .move_to(at(0.36, 0.64))
                    .line_to(at(0.64, 0.36));
                vec![(chain, stroke(1.5))]
            }
        },
    }
}

/// The two barbs of an arrowhead at `tip`, pointing away from `from`.
///
/// Here rather than borrowed from [`crate::geometry::arrow`]: that module builds
/// world-space decorations for a real route and takes an
/// [`EdgeRoute`](crate::geometry::EdgeRoute); a glyph has two points and a size.
fn arrowhead(tip: Vec2, from: Vec2, length: f32) -> Outline {
    let span = tip - from;
    let length_of = span.length();
    // A degenerate direction is a real case — two glyph points can coincide
    // while the fractions above are being tuned — and pointing right is the
    // answer that leaves a visible head rather than a NaN.
    let direction = if length_of > f32::EPSILON {
        span * (1.0 / length_of)
    } else {
        Vec2::new(1.0, 0.0)
    };
    let normal = Vec2::new(-direction.y, direction.x);
    let base = tip - direction * length;

    let mut head = Outline::with_capacity(3);
    head.move_to(base + normal * length * 0.5)
        .line_to(tip)
        .line_to(base - normal * length * 0.5);
    head
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOX: Rect = Rect::new(Vec2::new(10.0, 20.0), Vec2::new(16.0, 16.0));

    /// **Every button draws something.** A blank button is exactly the dead
    /// control the palette's own version of this test exists to catch, and an
    /// empty outline is how one would happen.
    #[test]
    fn every_glyph_draws_something() {
        for glyph in PanelGlyph::every() {
            let paths = strokes(glyph, BOX, Color::rgb(1.0, 1.0, 1.0));
            assert!(!paths.is_empty(), "{} draws nothing", glyph.name());
            for (outline, _) in &paths {
                assert!(!outline.is_empty(), "{} draws an empty path", glyph.name());
            }
        }
    }

    /// A glyph that escaped its button would overlap its neighbours. Every
    /// outline is fractions of the box, so this asserts the fractions stayed
    /// inside `0.0..=1.0` — including the ones the *generators* produce, which
    /// is the interesting half: a hand and a hatch are both free to wander.
    #[test]
    fn no_glyph_leaves_its_button() {
        // The sketch hand jitters by up to `jitter` screen pixels in each
        // direction, so the Sloppiness samples are allowed that much slack and
        // nothing else is.
        let slack = SAMPLE_HAND.jitter * Sloppiness::Cartoonist.roughness_scale() + 0.5;

        for glyph in PanelGlyph::every() {
            let allowance = match glyph {
                PanelGlyph::Sloppy(_) => slack,
                _ => 0.01,
            };
            for (outline, _) in strokes(glyph, BOX, Color::rgb(1.0, 1.0, 1.0)) {
                let bounds = outline.bounds().expect("a non-empty outline has bounds");
                assert!(
                    bounds.min().x >= BOX.min().x - allowance
                        && bounds.min().y >= BOX.min().y - allowance
                        && bounds.max().x <= BOX.max().x + allowance
                        && bounds.max().y <= BOX.max().y + allowance,
                    "{} draws outside its button: {bounds:?} in {BOX:?}",
                    glyph.name()
                );
            }
        }
    }

    /// **Every button's id is its own.** They are element ids, so two buttons
    /// sharing one is a GPUI state collision rather than a cosmetic clash — and
    /// this panel has forty of them where the palette has eleven.
    #[test]
    fn no_two_buttons_share_an_id() {
        let names: Vec<String> = PanelGlyph::every()
            .iter()
            .map(|glyph| glyph.name())
            .collect();
        for (index, name) in names.iter().enumerate() {
            assert!(
                !names[index + 1..].contains(name),
                "two panel buttons are both called {name}"
            );
        }
    }

    /// **Every section has its own label.** The mapping is a `match`, so the
    /// compiler already refuses a missing arm; what it cannot see is two rows
    /// pointing at the same string, which would put "Stroke" over the fill
    /// buttons and read as a translation bug.
    #[test]
    fn every_section_has_its_own_label() {
        let labels: Vec<flow::Text> = crate::properties::SelectionKind::ALL
            .iter()
            .flat_map(|kind| kind.sections())
            .map(|section| label_for(*section))
            .collect();
        let mut unique = labels.clone();
        unique.dedup();

        for (index, label) in labels.iter().enumerate() {
            let duplicates = labels[index + 1..].iter().filter(|it| *it == label).count();
            // A section appears on more than one panel, so a repeat is expected
            // — what is not is two *different* sections sharing a label.
            let sections: Vec<PanelSection> = crate::properties::SelectionKind::ALL
                .iter()
                .flat_map(|kind| kind.sections())
                .filter(|section| label_for(**section) == *label)
                .copied()
                .collect();
            let first = sections[0];
            assert!(
                sections.iter().all(|section| *section == first),
                "{duplicates} sections share one label: {sections:?}"
            );
        }
    }

    /// **Every change the panel can make is either a style write or explicitly
    /// something else.** The `match` in `as_style_edit` is exhaustive, so this
    /// is really an assertion that the four non-style variants are the four
    /// that were meant — a style row that answered `None` would be a control
    /// that silently does nothing.
    #[test]
    fn every_style_row_produces_a_style_write() {
        let style_changes = [
            Change::Stroke(Some(Color::BLACK)),
            Change::Background(None),
            Change::Fill(FillStyle::Hachure),
            Change::Width(StrokeWidthStep::Bold),
            Change::Dash(StrokeDashStep::Dotted),
            Change::Sloppiness(Sloppiness::Cartoonist),
            Change::Corners(CornerStyle::Round),
            Change::Arrowhead(ArrowEnd::End, true),
            Change::Family(FontFamily::Code),
            Change::Size(FontSize::Large),
            Change::Align(TextAlign::Right),
            Change::Opacity(50),
        ];
        for change in &style_changes {
            assert!(
                change.as_style_edit().is_some(),
                "{change:?} draws a control and writes nothing"
            );
        }

        for change in [
            Change::Arrow(ArrowKind::Elbow),
            Change::Layer(LayerAction::BringToFront),
            Change::Action(ElementAction::Duplicate),
            Change::Prompt(PromptKind::Link),
        ] {
            assert!(
                change.as_style_edit().is_none(),
                "{change:?} is not a style"
            );
        }
    }

    /// The one row whose change has to be applied to be checked: an opacity
    /// percent has to survive the trip through a style and back, or the slider
    /// jumps under the pointer.
    #[test]
    fn a_style_write_lands_where_the_control_reads_it_back() {
        for change in [
            Change::Width(StrokeWidthStep::ExtraBold),
            Change::Dash(StrokeDashStep::Dashed),
            Change::Corners(CornerStyle::Round),
            Change::Opacity(37),
        ] {
            let mut style = ElementStyle::default();
            change.as_style_edit().expect("a style write")(&mut style);
            let state = ControlState::of(&style);

            match change {
                Change::Width(step) => assert_eq!(state.width, step),
                Change::Dash(step) => assert_eq!(state.dash, step),
                Change::Corners(step) => assert_eq!(state.corners, step),
                Change::Opacity(percent) => assert_eq!(state.opacity_percent, percent),
                _ => unreachable!(),
            }
        }
    }
}
