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
//! # The strings arrived four phases early, and the key hints came free
//!
//! Phase 7.5 shipped this strip with no labels, no tooltips and therefore no
//! way to learn that `r` is the rectangle, and recorded that as Phase 8's gap.
//! Phase 9 closed it instead, because the alternative was three more phases of
//! English placeholders to find and remove — the whole argument is in
//! [`dodo_i18n::flow`]'s module doc.
//!
//! Each button now carries a tooltip, and **the keystroke beside it is not a
//! string**: `gpui-component`'s `Tooltip::action` looks the binding up from the
//! action and [`KEY_CONTEXT`](crate::views::flow::KEY_CONTEXT), so the hint is
//! rendered from [`commands::keys`](crate::commands::keys)'s real table. A
//! rebind moves the hint with it and there is nothing to keep in step — which
//! is the same reason the glyphs are drawn by the canvas's own outline builders
//! rather than by an icon set.
//!
//! # The two actions beside the tools
//!
//! **Delete** and **the tool lock** sit past a divider, because they are not
//! tools: neither changes what the next press means. They are here rather than
//! in a second strip because a toolbar the user has to find twice is worse than
//! one with a divider in it, and because the lock is *about* the tools — with
//! it on, finishing a drawing keeps the tool instead of returning to Select.
//!
//! The lock reuses the active-tool look: filled means locked. A second glyph
//! for the unlocked state would be a second thing to draw, and the state it
//! reports is exactly the one "this button is on" already means.
//!
//! Delete is drawn muted and does nothing when the selection is empty. Muted
//! rather than absent, so the control does not move under the pointer, and
//! muted rather than a hidden no-op, so a user who clicks it learns why nothing
//! happened.

use dodo_i18n::{flow, t};
use gpui::{
    App, Bounds, Entity, Hsla, InteractiveElement, IntoElement, MouseButton, ParentElement, Pixels,
    StatefulInteractiveElement, Styled, canvas, div, prelude::FluentBuilder, px,
};
use gpui_component::{ActiveTheme, tooltip::Tooltip};

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
    views::{
        FlowView,
        flow::KEY_CONTEXT,
        keymap::{Delete, SelectTool, ToggleToolLock},
    },
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

/// **Everything the palette draws**, as one enum.
///
/// The tools borrow their geometry from the canvas; the three that are not
/// tools are built here. One type rather than two so [`strokes`] stays the
/// single place a button's picture is decided, and so
/// [`tests::every_glyph_draws_something`] can walk every button there is —
/// which is what catches a new one added with no `match` arm, the way a blank
/// button would otherwise reach the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Glyph {
    Tool(CanvasTool),
    /// The Delete action.
    Trash,
    /// The tool lock, in either state — see the module doc for why one glyph
    /// covers both.
    Lock,
}

impl Glyph {
    /// Every glyph the palette can draw, for the tests. In palette order.
    #[cfg(test)]
    const ALL: &'static [Glyph] = &[Glyph::Trash, Glyph::Lock];

    /// A short stable name, for the element id. **Not user-facing** — the
    /// labels are `dodo_i18n::flow`'s.
    fn name(self) -> &'static str {
        match self {
            Glyph::Tool(tool) => tool.name(),
            Glyph::Trash => "delete",
            Glyph::Lock => "tool-lock",
        }
    }
}

/// What the palette needs to know about the canvas to draw itself.
///
/// A struct rather than three arguments because two of the three are new this
/// phase and a fourth is Phase 11's; positional `bool`s at a call site are how
/// "locked" and "can delete" end up swapped with nothing to notice it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaletteState {
    pub tool: CanvasTool,
    pub tool_locked: bool,
    /// Whether anything is selected. The Delete button is muted when not.
    pub can_delete: bool,
}

/// **The palette** (§45), as an element the canvas positions over itself.
///
/// Takes the view entity rather than a `Context<FlowView>` because a click
/// handler is handed an `&mut App` — `gpui-component-recipes` records the same
/// constraint for `Button::on_click`, and the launcher's style toggle already
/// captures its entity for it.
pub fn palette(view: Entity<FlowView>, state: PaletteState, cx: &App) -> impl IntoElement {
    div()
        .flex()
        .items_center()
        .gap(px(2.0))
        .p(px(3.0))
        .rounded(cx.theme().radius)
        .border_1()
        .border_color(cx.theme().border)
        .bg(cx.theme().popover)
        .children(
            CanvasTool::ALL
                .iter()
                .map(|tool| tool_button(*tool, state.tool, view.clone(), cx)),
        )
        .child(divider(cx))
        .child(lock_button(state.tool_locked, view.clone(), cx))
        .child(delete_button(state.can_delete, view, cx))
}

/// The hairline between the tools and the actions beside them.
fn divider(cx: &App) -> impl IntoElement {
    div()
        .w(px(1.0))
        .h(px(BUTTON_PIXELS - 10.0))
        .mx(px(3.0))
        .bg(cx.theme().border)
}

/// One tool's button.
fn tool_button(
    tool: CanvasTool,
    active: CanvasTool,
    view: Entity<FlowView>,
    cx: &App,
) -> impl IntoElement {
    shell(Glyph::Tool(tool), tool == active, true, cx)
        .tooltip(move |window, cx| {
            Tooltip::new(t(label_for(tool), cx))
                .action(&SelectTool { tool }, Some(KEY_CONTEXT))
                .build(window, cx)
        })
        // A mouse-down rather than a click: the canvas's own listeners are
        // registered on the whole window and gated on its hitbox, so a press
        // that reaches this element must not also be read as a press on the
        // canvas underneath it.
        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
            view.update(cx, |this, cx| this.set_tool(tool, window, cx));
        })
}

/// The tool lock, as a toggle: filled means the tool survives a drawing.
fn lock_button(locked: bool, view: Entity<FlowView>, cx: &App) -> impl IntoElement {
    shell(Glyph::Lock, locked, true, cx)
        .tooltip(move |window, cx| {
            Tooltip::new(t(flow::Text::KeepToolActive, cx))
                .action(&ToggleToolLock, Some(KEY_CONTEXT))
                .build(window, cx)
        })
        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
            view.update(cx, |this, cx| this.set_tool_locked(!locked, window, cx));
        })
}

/// The Delete action. Muted and inert with nothing selected — see the module
/// doc for why it is not hidden instead.
fn delete_button(enabled: bool, view: Entity<FlowView>, cx: &App) -> impl IntoElement {
    shell(Glyph::Trash, false, enabled, cx)
        .tooltip(move |window, cx| {
            Tooltip::new(t(flow::Text::Delete, cx))
                .action(&Delete, Some(KEY_CONTEXT))
                .build(window, cx)
        })
        .on_mouse_down(MouseButton::Left, move |_, window, cx| {
            if !enabled {
                return;
            }
            view.update(cx, |this, cx| this.delete_selection(window, cx));
        })
}

/// The button every entry above is: a fixed square, the active fill, and the
/// glyph inside it.
///
/// The id is the glyph's stable *name* rather than its label — an id must not
/// change when the language does, which is exactly why `dodo-i18n-text`'s rule
/// exempts it.
fn shell(glyph: Glyph, selected: bool, enabled: bool, cx: &App) -> gpui::Stateful<gpui::Div> {
    let ink = match (selected, enabled) {
        (true, _) => cx.theme().primary_foreground,
        (false, true) => cx.theme().foreground,
        (false, false) => cx.theme().muted_foreground,
    };

    div()
        .id(glyph.name())
        .size(px(BUTTON_PIXELS))
        .flex()
        .items_center()
        .justify_center()
        .rounded(cx.theme().radius)
        .when(selected, |this| this.bg(cx.theme().primary))
        .child(paint(glyph, ink))
}

/// One tool's translated name.
///
/// A `match` rather than a method on [`CanvasTool`], because that type is below
/// the UI-framework line and naming a catalogue there would put a *string* in
/// the pure layer — the same boundary `lib.rs`'s
/// `the_pure_layers_name_no_ui_framework` guards, arriving from the other side.
fn label_for(tool: CanvasTool) -> flow::Text {
    match tool {
        CanvasTool::Select => flow::Text::ToolSelect,
        CanvasTool::Hand => flow::Text::ToolHand,
        CanvasTool::Rectangle => flow::Text::ToolRectangle,
        CanvasTool::Diamond => flow::Text::ToolDiamond,
        CanvasTool::Ellipse => flow::Text::ToolEllipse,
        CanvasTool::Arrow => flow::Text::ToolArrow,
        CanvasTool::Line => flow::Text::ToolLine,
        CanvasTool::GraphNode => flow::Text::ToolGraphNode,
    }
}

/// One glyph, painted rather than laid out.
///
/// A bare `canvas()` with no hitbox: the button's own `div` is what takes the
/// press, and a second hitbox here would be one more thing to keep in step.
fn paint(glyph: Glyph, ink: Hsla) -> impl IntoElement {
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

/// **Every path one glyph is made of.**
///
/// A `Vec` because several are more than one path — eleven allocations per
/// frame the palette is drawn, over a control that exists once. The canvas's
/// own no-allocation rules (§40 rule 14) are about the per-element loops, not
/// about a toolbar.
fn strokes(glyph: Glyph, box_: Rect, ink: Color) -> Vec<(Outline, PathPaint)> {
    let stroke = PathPaint::Stroke {
        color: ink,
        width: GLYPH_STROKE,
    };
    let fill = PathPaint::Fill(ink);

    // The shape tools borrow their own geometry, which is the point of the
    // module: a button cannot draw something other than what it creates.
    let body = |shape: NodeShape| shapes::outline_for_node(shape, box_, 3.0);

    let tool = match glyph {
        Glyph::Tool(tool) => tool,
        Glyph::Trash => return trash_glyph(box_).map(|outline| (outline, stroke)).collect(),
        Glyph::Lock => return lock_glyph(box_).map(|outline| (outline, stroke)).collect(),
    };

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

/// The Delete action's glyph: a lidded bin, stroked.
///
/// Three subpaths rather than one closed outline — the lid crosses the body and
/// a single path would have to double back through it, which reads as a smudge
/// at 14 px. Every coordinate is a fraction of the box, so it is crisp at any
/// button size.
pub fn trash_glyph(box_: Rect) -> impl Iterator<Item = Outline> {
    let (o, s) = (box_.origin, box_.size);
    let at = move |x: f32, y: f32| Vec2::new(o.x + s.x * x, o.y + s.y * y);

    // The lid, with the handle standing on it.
    let mut lid = Outline::with_capacity(6);
    lid.move_to(at(0.06, 0.22))
        .line_to(at(0.94, 0.22))
        .move_to(at(0.36, 0.22))
        .line_to(at(0.40, 0.06))
        .line_to(at(0.60, 0.06))
        .line_to(at(0.64, 0.22));

    // The tapered body.
    let mut body = Outline::with_capacity(4);
    body.move_to(at(0.16, 0.22))
        .line_to(at(0.23, 0.96))
        .line_to(at(0.77, 0.96))
        .line_to(at(0.84, 0.22));

    // The two ribs, which are what make it read as a bin rather than a cup.
    let mut ribs = Outline::with_capacity(4);
    ribs.move_to(at(0.40, 0.36))
        .line_to(at(0.43, 0.84))
        .move_to(at(0.60, 0.36))
        .line_to(at(0.57, 0.84));

    [lid, body, ribs].into_iter()
}

/// The tool lock's glyph: a padlock, stroked.
///
/// Two subpaths, the shackle and the body. The shackle is a pair of cubics
/// rather than an arc because [`Outline`] has no arc command — a canvas that
/// only ever needed cubics is one fewer primitive for the flattener and the
/// vertex estimate to know about.
pub fn lock_glyph(box_: Rect) -> impl Iterator<Item = Outline> {
    let (o, s) = (box_.origin, box_.size);
    let at = move |x: f32, y: f32| Vec2::new(o.x + s.x * x, o.y + s.y * y);

    // The shackle: up the left side, over the top, down the right.
    let mut shackle = Outline::with_capacity(3);
    shackle
        .move_to(at(0.26, 0.46))
        .line_to(at(0.26, 0.30))
        .cubic_to(at(0.26, 0.04), at(0.74, 0.04), at(0.74, 0.30))
        .line_to(at(0.74, 0.46));

    let mut body = Outline::with_capacity(5);
    body.move_to(at(0.12, 0.46))
        .line_to(at(0.88, 0.46))
        .line_to(at(0.88, 0.96))
        .line_to(at(0.12, 0.96))
        .close();

    [shackle, body].into_iter()
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

    /// Every button the palette can draw, tools and actions alike.
    fn every_glyph() -> Vec<Glyph> {
        CanvasTool::ALL
            .iter()
            .map(|tool| Glyph::Tool(*tool))
            .chain(Glyph::ALL.iter().copied())
            .collect()
    }

    /// **Every button draws something.** A blank button is exactly the dead
    /// control this palette exists to remove, and an empty outline is how one
    /// would happen — a missing `match` arm returns `Vec::new()` without
    /// complaint.
    #[test]
    fn every_glyph_draws_something() {
        for glyph in every_glyph() {
            let paths = strokes(glyph, BOX, Color::rgb(1.0, 1.0, 1.0));
            assert!(!paths.is_empty(), "{} draws nothing", glyph.name());
            for (outline, _) in &paths {
                assert!(!outline.is_empty(), "{} draws an empty path", glyph.name());
            }
        }
    }

    /// A glyph that escaped its button would overlap its neighbours. Every
    /// outline is built from fractions of the box, so this is the assertion
    /// that the fractions stayed inside `0.0..=1.0`.
    #[test]
    fn no_glyph_leaves_its_button() {
        for glyph in every_glyph() {
            for (outline, _) in strokes(glyph, BOX, Color::rgb(1.0, 1.0, 1.0)) {
                let bounds = outline.bounds().expect("a non-empty outline has bounds");
                assert!(
                    bounds.min().x >= BOX.min().x - 0.01
                        && bounds.min().y >= BOX.min().y - 0.01
                        && bounds.max().x <= BOX.max().x + 0.01
                        && bounds.max().y <= BOX.max().y + 0.01,
                    "{} draws outside its button: {bounds:?} in {BOX:?}",
                    glyph.name()
                );
            }
        }
    }

    /// **Every button's id is its own.** They are element ids, so two buttons
    /// sharing one is a GPUI state collision rather than a cosmetic clash —
    /// and the actions were added beside eight tools that already had names.
    #[test]
    fn no_two_buttons_share_an_id() {
        let names: Vec<&str> = every_glyph().iter().map(|glyph| glyph.name()).collect();
        for (index, name) in names.iter().enumerate() {
            assert!(
                !names[index + 1..].contains(name),
                "two palette buttons are both called {name}"
            );
        }
    }

    /// **Every tool's tooltip is a real string in the catalogue.** The mapping
    /// is a `match`, so the compiler already refuses a missing arm; what it
    /// cannot see is two tools pointing at the same variant, which would put
    /// "Rectangle" under the diamond and read as a translation bug.
    #[test]
    fn every_tool_has_its_own_label() {
        let labels: Vec<flow::Text> = CanvasTool::ALL
            .iter()
            .map(|tool| label_for(*tool))
            .collect();
        for (index, label) in labels.iter().enumerate() {
            assert!(
                !labels[index + 1..].contains(label),
                "two tools share the label {label:?}"
            );
        }
    }

    /// The two hand-built *filled* glyphs are closed polygons — an unclosed
    /// fill is a shape lyon completes for you, differently from how it was
    /// drawn. The stroked ones (trash, lock) are deliberately open.
    #[test]
    fn the_filled_glyphs_are_closed() {
        for outline in [pointer_glyph(BOX), pan_glyph(BOX)] {
            assert!(matches!(
                outline.commands().last(),
                Some(shapes::SubpathCommand::Close)
            ));
        }
    }
}
