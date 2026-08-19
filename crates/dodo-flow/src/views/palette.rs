//! **The tool palette** (§45): the strip that lets a user say what the next
//! press means.
//!
//! # Why this exists at all, which is the interesting part
//!
//! §45 was folded into Phase 2's interaction state machine and never given a
//! home. Seven phases later the canvas could pan, zoom, drag, select, connect,
//! simplify, sketch and undo, and **a user still could not create a single
//! element**. Nothing was broken; there was simply no control that said "now I
//! am drawing a rectangle", and no test could have noticed because no test
//! asks what a person can accomplish. It was found by opening the window.
//!
//! # The glyphs are drawn by the canvas's own outline builders
//!
//! Each button paints its tool's shape through
//! [`shapes::outline_for_node`](crate::render::shapes::outline_for_node) and
//! [`build_path`](crate::render::painter::build_path) — the same two functions
//! that draw the element the button creates. That is not a flourish; it is what
//! makes the palette need **no icon assets and no strings**:
//!
//! - **No assets.** `gpui-component`'s icon set has no square, circle, diamond,
//!   pointer or hand, so an icon palette would mean eight new SVGs in dodo's
//!   `assets/`, an `AppIcon` variant each, and a dependency from this crate on
//!   `dodo-app-icon` — a lot of new surface for eight shapes the crate can
//!   already draw.
//! - **No strings.** Every user-visible string in dodo goes through `dodo-i18n`
//!   (the root `AGENTS.md` invariant), and the canvas's translations are Phase
//!   8's work. A labelled or tooltipped palette now would be English literals
//!   that phase then has to find and remove. A drawn glyph needs no
//!   translation.
//!
//! It also has a property an icon set does not: **the button cannot drift from
//! what it makes.** Change how an arrow is drawn and its button changes with
//! it, because there is one outline builder and both call it.
//!
//! The two navigating tools have no element to borrow a shape from, so
//! [`pointer_glyph`] and [`pan_glyph`] build theirs here — still as an
//! [`Outline`], so they are ordinary geometry a test can measure.
//!
//! # What is deliberately missing, and what it costs
//!
//! **No labels, no tooltips, and therefore no discoverable key hints.** A user
//! who has not read the doc comment cannot learn from the palette that `r` is
//! the rectangle. That is a real gap and it is Phase 8's to close, in the same
//! pass that gives the canvas its translated strings; the shapes are legible
//! enough to pick from without one, which is why the phase is usable without
//! it.

use gpui::{
    App, Bounds, Entity, Hsla, InteractiveElement, IntoElement, MouseButton, ParentElement, Pixels,
    Styled, canvas, div, prelude::FluentBuilder, px,
};
use gpui_component::ActiveTheme;

use crate::{
    geometry::{Rect, Vec2},
    interaction::CanvasTool,
    models::{Color, RenderQuality},
    render::{
        painter::{build_path, from_hsla, to_hsla},
        plan::PathPaint,
        shapes::{self, Outline},
    },
    runtime::NodeShape,
    views::FlowView,
};

/// One button's side, in screen pixels. Matches `gpui-component`'s `small`
/// button height, so a palette sits beside the launcher's own buttons without
/// looking like a different control.
const BUTTON_PIXELS: f32 = 28.0;

/// The inset the glyph keeps inside its button.
const GLYPH_INSET: f32 = 7.0;

/// The glyph's stroke width, in screen pixels.
const GLYPH_STROKE: f32 = 1.5;

/// The tolerance the glyphs are flattened at. Tighter than the canvas's
/// balanced default because a 14 px glyph has no room to hide a facet, and it
/// costs nothing: eight paths of a few dozen vertices, once per frame the
/// palette is on screen.
const GLYPH_QUALITY: RenderQuality = RenderQuality::PRECISE;

/// **The palette** (§45), as an element the canvas positions over itself.
///
/// Takes the view entity rather than a `Context<FlowView>` because a click
/// handler is handed an `&mut App` — `gpui-component-recipes` records the same
/// constraint for `Button::on_click`, and the launcher's style toggle already
/// captures its entity for it.
pub fn palette(view: Entity<FlowView>, active: CanvasTool, cx: &App) -> impl IntoElement {
    div()
        .flex()
        .gap(px(2.0))
        .p(px(3.0))
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().popover)
        .children(
            CanvasTool::ALL
                .iter()
                .map(|tool| button(*tool, active, view.clone(), cx)),
        )
}

fn button(
    tool: CanvasTool,
    active: CanvasTool,
    view: Entity<FlowView>,
    cx: &App,
) -> impl IntoElement {
    let selected = tool == active;
    let ink = if selected {
        cx.theme().primary_foreground
    } else {
        cx.theme().foreground
    };

    div()
        .id(tool.name())
        .size(px(BUTTON_PIXELS))
        .flex()
        .items_center()
        .justify_center()
        .rounded(cx.theme().radius)
        .when(selected, |this| this.bg(cx.theme().primary))
        .child(glyph(tool, ink))
        // A mouse-down rather than a click: the canvas's own listeners are
        // registered on the whole window and gated on its hitbox, so a press
        // that reaches this element must not also be read as a press on the
        // canvas underneath it.
        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
            view.update(cx, |this, cx| this.set_tool(tool, window, cx));
        })
}

/// One tool's glyph, painted rather than laid out.
///
/// A bare `canvas()` with no hitbox: the button's own `div` is what takes the
/// press, and a second hitbox here would be one more thing to keep in step.
fn glyph(tool: CanvasTool, ink: Hsla) -> impl IntoElement {
    let color = from_hsla(ink);
    canvas(
        |_, _, _| (),
        move |bounds: Bounds<Pixels>, (), window, _| {
            for (outline, paint) in strokes(tool, inset(bounds), color) {
                if let Some(path) = build_path(&outline, paint, GLYPH_QUALITY.flattening_tolerance)
                {
                    window.paint_path(path, to_hsla(paint.color()));
                }
            }
        },
    )
    .size(px(BUTTON_PIXELS))
}

/// The glyph's box: the button's bounds, inset and made pane-relative.
///
/// A canvas paint closure is handed **window** coordinates while
/// `Window::paint_path` also takes window coordinates, so unlike
/// `render::painter`'s anchor there is nothing to subtract — the outline is
/// built where it is painted.
fn inset(bounds: Bounds<Pixels>) -> Rect {
    let origin = Vec2::new(
        bounds.origin.x.as_f32() + GLYPH_INSET,
        bounds.origin.y.as_f32() + GLYPH_INSET,
    );
    let side = (bounds.size.width.as_f32() - GLYPH_INSET * 2.0).max(1.0);
    let height = (bounds.size.height.as_f32() - GLYPH_INSET * 2.0).max(1.0);
    Rect::new(origin, Vec2::new(side, height))
}

/// **Every path one tool's glyph is made of.**
///
/// A `Vec` because the graph-node glyph is three (a body and two handle dots) —
/// eight allocations per frame the palette is drawn, over a control that exists
/// once. The canvas's own no-allocation rules (§40 rule 14) are about the
/// per-element loops, not about eight buttons.
fn strokes(tool: CanvasTool, box_: Rect, ink: Color) -> Vec<(Outline, PathPaint)> {
    let stroke = PathPaint::Stroke {
        color: ink,
        width: GLYPH_STROKE,
    };
    let fill = PathPaint::Fill(ink);

    // The shape tools borrow their own geometry, which is the point of the
    // module: a button cannot draw something other than what it creates.
    let body = |shape: NodeShape| shapes::outline_for_node(shape, box_, 3.0);

    match tool {
        CanvasTool::Select => vec![(pointer_glyph(box_), fill)],
        CanvasTool::Hand => vec![(pan_glyph(box_), fill)],
        CanvasTool::Rectangle => body(NodeShape::Rectangle)
            .map(|outline| vec![(outline, stroke)])
            .unwrap_or_default(),
        CanvasTool::Diamond => body(NodeShape::Diamond)
            .map(|outline| vec![(outline, stroke)])
            .unwrap_or_default(),
        CanvasTool::Ellipse => body(NodeShape::Ellipse)
            .map(|outline| vec![(outline, stroke)])
            .unwrap_or_default(),
        CanvasTool::Arrow => body(NodeShape::Arrow)
            .map(|outline| vec![(outline, stroke)])
            .unwrap_or_default(),
        CanvasTool::Line => body(NodeShape::Line)
            .map(|outline| vec![(outline, stroke)])
            .unwrap_or_default(),
        // A rounded body and the two handle dots it is born with — the one
        // glyph that has to say more than its outline does, because a rounded
        // rectangle alone would read as the rectangle tool.
        CanvasTool::GraphNode => {
            let body_box = Rect::new(
                box_.origin + Vec2::new(box_.size.x * 0.22, box_.size.y * 0.15),
                Vec2::new(box_.size.x * 0.56, box_.size.y * 0.7),
            );
            let dot = box_.size.y * 0.14;
            let mid = box_.origin.y + box_.size.y * 0.5;
            let left = Rect::new(
                Vec2::new(box_.origin.x, mid - dot * 0.5),
                Vec2::new(dot, dot),
            );
            let right = Rect::new(
                Vec2::new(box_.origin.x + box_.size.x - dot, mid - dot * 0.5),
                Vec2::new(dot, dot),
            );

            let mut paths = Vec::with_capacity(3);
            if let Some(outline) = shapes::outline_for_node(NodeShape::GraphNode, body_box, 3.0) {
                paths.push((outline, stroke));
            }
            paths.push((shapes::ellipse(left), fill));
            paths.push((shapes::ellipse(right), fill));
            paths
        }
    }
}

/// The Select tool's glyph: the classic pointer arrow, filled.
///
/// Hand-built because there is no element shaped like a cursor. Proportions are
/// the ones every pointer icon uses — a 40°-ish head and a tail down the right
/// side — expressed as fractions of the box so it is crisp at any button size.
pub fn pointer_glyph(box_: Rect) -> Outline {
    let (o, s) = (box_.origin, box_.size);
    let at = |x: f32, y: f32| Vec2::new(o.x + s.x * x, o.y + s.y * y);

    let mut outline = Outline::with_capacity(8);
    outline
        .move_to(at(0.16, 0.0))
        .line_to(at(0.16, 0.92))
        .line_to(at(0.40, 0.68))
        .line_to(at(0.56, 1.0))
        .line_to(at(0.72, 0.92))
        .line_to(at(0.56, 0.62))
        .line_to(at(0.84, 0.60))
        .close();
    outline
}

/// The Hand tool's glyph: a four-way move arrow.
///
/// A move cross rather than a drawn hand. A hand is an outline with eight
/// curves in it and reads as a smudge at 14 px; the four-way arrow is the other
/// universal pan cursor, and it is twelve straight lines.
pub fn pan_glyph(box_: Rect) -> Outline {
    let (o, s) = (box_.origin, box_.size);
    let at = |x: f32, y: f32| Vec2::new(o.x + s.x * x, o.y + s.y * y);

    // The arm's half-width and the head's half-width, as fractions.
    let (arm, head) = (0.12, 0.26);
    let (lo, hi) = (0.5 - arm, 0.5 + arm);
    let (hlo, hhi) = (0.5 - head, 0.5 + head);
    let tip = 0.22;

    let mut outline = Outline::with_capacity(17);
    outline
        .move_to(at(0.5, 0.0))
        .line_to(at(hhi, tip))
        .line_to(at(hi, tip))
        .line_to(at(hi, lo))
        .line_to(at(1.0 - tip, lo))
        .line_to(at(1.0 - tip, hlo))
        .line_to(at(1.0, 0.5))
        .line_to(at(1.0 - tip, hhi))
        .line_to(at(1.0 - tip, hi))
        .line_to(at(hi, hi))
        .line_to(at(hi, 1.0 - tip))
        .line_to(at(hhi, 1.0 - tip))
        .line_to(at(0.5, 1.0))
        .line_to(at(hlo, 1.0 - tip))
        .line_to(at(lo, 1.0 - tip))
        .line_to(at(lo, hi))
        .line_to(at(tip, hi))
        .line_to(at(tip, hhi))
        .line_to(at(0.0, 0.5))
        .line_to(at(tip, hlo))
        .line_to(at(tip, lo))
        .line_to(at(lo, lo))
        .line_to(at(lo, tip))
        .line_to(at(hlo, tip))
        .close();
    outline
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOX: Rect = Rect::new(Vec2::new(10.0, 20.0), Vec2::new(14.0, 14.0));

    /// **Every tool's button draws something.** A blank button is exactly the
    /// dead control this phase exists to remove, and an empty outline is how
    /// one would happen — a missing `match` arm returns `Vec::new()` without
    /// complaint.
    #[test]
    fn every_tool_has_a_glyph() {
        for tool in CanvasTool::ALL {
            let paths = strokes(*tool, BOX, Color::rgb(1.0, 1.0, 1.0));
            assert!(!paths.is_empty(), "{} draws nothing", tool.name());
            for (outline, _) in &paths {
                assert!(!outline.is_empty(), "{} draws an empty path", tool.name());
            }
        }
    }

    /// A glyph that escaped its button would overlap its neighbours. Every
    /// outline is built from fractions of the box, so this is the assertion
    /// that the fractions stayed inside `0.0..=1.0`.
    #[test]
    fn no_glyph_leaves_its_button() {
        for tool in CanvasTool::ALL {
            for (outline, _) in strokes(*tool, BOX, Color::rgb(1.0, 1.0, 1.0)) {
                let bounds = outline.bounds().expect("a non-empty outline has bounds");
                assert!(
                    bounds.min().x >= BOX.min().x - 0.01
                        && bounds.min().y >= BOX.min().y - 0.01
                        && bounds.max().x <= BOX.max().x + 0.01
                        && bounds.max().y <= BOX.max().y + 0.01,
                    "{} draws outside its button: {bounds:?} in {BOX:?}",
                    tool.name()
                );
            }
        }
    }

    /// The two hand-built glyphs are closed polygons, because both are filled —
    /// an unclosed fill is a shape lyon completes for you, differently from how
    /// it was drawn.
    #[test]
    fn the_hand_built_glyphs_are_closed() {
        for outline in [pointer_glyph(BOX), pan_glyph(BOX)] {
            assert!(matches!(
                outline.commands().last(),
                Some(shapes::SubpathCommand::Close)
            ));
        }
    }
}
