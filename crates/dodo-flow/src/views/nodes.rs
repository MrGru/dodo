//! **The rich half of the hybrid renderer** (§16, §43, §44, §47): the GPUI
//! elements a visible node gets when it is close enough to be worth them.
//!
//! # Why any of this is elements rather than paint
//!
//! Everything on this canvas *could* be painted. Phase 4 paints node bodies,
//! borders, handles and edges with no elements at all, and it is fast. What
//! paint cannot give is §47's list: keyboard focus, a hover state, a cursor
//! that changes, a context menu, selectable and editable text, a button. Those
//! are what a real node needs and what a rectangle on a canvas can never be.
//!
//! So the split is not "fast path and slow path". It is **"the few things you
//! are working with are real UI, and the thousands you are looking past are
//! pixels"**, and the ladder in [`crate::render::lod`] decides which is which.
//!
//! # The counts, which are the whole argument
//!
//! Phase 0 measured ~1,600 rich interactive elements holding 60 fps — `div`s
//! with a background, a border, a label and a hover style, at 0.45 ms to build
//! the tree. The requirements assumed ~70 visible nodes, so there is roughly
//! 20× headroom. What matters is that the count comes from
//! [`RenderSnapshot::rich`] — the *visible* set, bounded by
//! [`RenderBudgets::max_rich_elements`](crate::budgets::RenderBudgets::max_rich_elements)
//! — and never from the document. `render::snapshot`'s
//! `a_hundred_thousand_nodes_produce_tens_of_elements` is the assertion.
//!
//! # Absolute positioning, and why there is no layout
//!
//! Every element here is `absolute()` at a pane-relative offset the snapshot
//! computed. GPUI's layout engine is never asked where a node goes, because the
//! answer is the viewport transform and it already exists — running Taffy over
//! a thousand absolutely-positioned siblings to reach a conclusion
//! [`Viewport`](crate::geometry::Viewport) already knows is pure cost.
//!
//! # §44, as a rule this module cannot break
//!
//! *"Do not create a heavy component hierarchy for every inactive element's
//! controls. Only selected/hovered/editing objects require detailed
//! controls."* [`handles`] reads [`RenderSnapshot::interactive_handles`], and
//! [`selection_box`] and [`resize_grips`] read [`RenderSnapshot::overlay`]; the
//! snapshot populates both for **one** node. Detailed property controls live in
//! the contextual [`crate::views::properties`] panel instead of being duplicated
//! over the node. There is no path from here to "a handle element per visible
//! node", because the snapshot never offers one.

use gpui::{
    AnyElement, App, Div, Hsla, InteractiveElement, IntoElement, ParentElement, Styled, div, px,
    relative,
};
use gpui_component::ActiveTheme;

use crate::{
    geometry::{Rect, ResizeCorner},
    render::{
        registry::{AccentRole, NodeGlyph, NodeVisual},
        scene::GRAPH_NODE_RADIUS,
        snapshot::{InteractiveHandle, RenderSnapshot, RichNode},
    },
    runtime::GraphWorld,
};

/// The accent bar's width, in screen pixels. Constant on screen, like every
/// other piece of chrome here — a bar that scaled with the zoom would be a
/// hairline at 0.6 and a slab at 3.0.
const ACCENT_BAR_PIXELS: f32 = 3.0;

/// A handle element's hit size in screen pixels. Larger than the painted dot's
/// 9 px diameter, because a target you can hit slightly outside is right and
/// one that is smaller than it looks is not.
const HANDLE_HIT_PIXELS: f32 = 14.0;

/// How far outside the element the selection ring sits, in screen pixels.
///
/// A named constant rather than a literal because §12's grips are placed on the
/// same rectangle: a ring and a set of grips that disagreed by three pixels
/// would put the handles a user aims at off the box they can see.
const SELECTION_RING_INSET: f32 = 3.0;

/// One resize grip's side, in screen pixels. Smaller than a connection handle's
/// target, because a corner is aimed at deliberately — and matched to
/// [`HitTolerance::GRIP_SCREEN_RADIUS`](crate::runtime::HitTolerance::GRIP_SCREEN_RADIUS),
/// which is what decides whether a press on one lands.
const GRIP_PIXELS: f32 = 9.0;

/// **Every rich node's element, for one frame.**
///
/// Returns a `Vec` rather than an iterator because GPUI's `children` wants one,
/// and it is sized from the snapshot so the allocation is one per frame over a
/// list that is tens long — not the per-element allocation §40 rule 10 is
/// about.
pub fn nodes(snapshot: &RenderSnapshot, world: &GraphWorld, cx: &App) -> Vec<AnyElement> {
    snapshot
        .rich()
        .iter()
        .map(|rich| node(rich, world, cx))
        .collect()
}

/// One node.
///
/// The label is read from the store **through the index** rather than carried
/// on the snapshot, which is what keeps §24's "compact IDs rather than cloned
/// metadata" true all the way to the element tree.
///
/// # The element does not draw the body, and that is the fix for a real bug
///
/// It used to, in Clean mode: `theme.border` for the border, `theme.secondary`
/// for the fill, one pixel wide or two when selected. None of those is the
/// element's own [`ElementStyle`](crate::models::ElementStyle), so Phase 11's
/// property panel wrote a stroke colour, a fill, a width, an opacity, a dash
/// and a hatch that **nothing on screen read** — for exactly the nodes a person
/// works with, since a rectangle at working zoom is a rich node. In Sketch mode
/// the same edits showed, because a hand-drawn border has no `div` form and the
/// canvas had to paint the body; the captain saw that as "properties only apply
/// in Sketch mode".
///
/// So the body belongs to the canvas in both modes now
/// ([`plan_rich_bodies`](crate::render::scene)), and this element keeps only
/// what an element is *for*: an id, a cursor, focus, a hover the canvas paints
/// the ring for, an accent bar, a glyph and a label. A `div` cannot express a
/// dash, a hatch, an opacity or a hand, so having it express the two properties
/// it *can* was never a smaller version of the same thing — it was a second
/// renderer that quietly disagreed with the first.
fn node(rich: &RichNode, world: &GraphWorld, cx: &App) -> AnyElement {
    let accent = accent_color(rich.visual.accent, cx);
    let style = world.nodes().style(rich.node);
    // The clip for the accent bar and the label, not a border: the painted
    // corner is the canvas's, from the same `corner_radius` at the frame's own
    // zoom. A floor of `GRAPH_NODE_RADIUS` so an unstyled node's bar is not a
    // square peg in a rounded body.
    let radius = style.corner_radius.max(GRAPH_NODE_RADIUS);
    let body = placed(rich.screen)
        // §47: an id is what makes hover, focus and a context menu possible at
        // all. It is derived from the runtime index, which is stable for the
        // life of the document.
        .id(("flow-node", rich.node.raw() as usize))
        .rounded(px(radius))
        .overflow_hidden()
        .cursor_pointer();

    decorated(body, rich, world, accent, cx)
}

/// The accent bar and the label, on whichever body the caller styled.
///
/// Split out so the sketch path and the clean path share every child element
/// and differ only in the two properties that a hand-drawn body owns — the
/// border and the fill.
fn decorated(
    body: gpui::Stateful<Div>,
    rich: &RichNode,
    world: &GraphWorld,
    accent: Hsla,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    let mut children: Vec<AnyElement> = Vec::new();
    if rich.visual.shows_accent_bar {
        children.push(
            div()
                .absolute()
                .left_0()
                .top_0()
                .bottom_0()
                .w(px(ACCENT_BAR_PIXELS))
                .bg(accent)
                .into_any_element(),
        );
    }

    if let (Some(font_size), Some(label)) = (
        rich.label_font_size,
        world.nodes().cold(rich.node).label.as_ref(),
    ) {
        children.push(
            div()
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .gap(px(6.0))
                .px(px(10.0))
                .child(glyph(rich.visual, accent))
                .child(
                    div()
                        .flex_1()
                        // Without this a long label pushes the row wider
                        // than the node instead of wrapping inside it — the
                        // `min_w_0` rule dodo's other tools also live by.
                        .min_w_0()
                        .text_size(px(font_size))
                        .text_color(theme.foreground)
                        // **Wrapping, not truncation** (Phase 10.5), because a
                        // rich node and a canvas node are the same element seen
                        // through two zoom rungs and they must not disagree
                        // about what a label does. `truncate()` — which is
                        // `overflow_hidden` + `whitespace_nowrap` +
                        // `text_ellipsis` — kept the ellipsis behaviour here
                        // while `render::painter` wrapped, so a sentence read
                        // as "a long lab…" at 100 % and as three lines at 60 %.
                        // Only the clipping is kept: it is what stops an
                        // unbroken word from reaching a neighbour, and GPUI's
                        // wrapper already breaks one that cannot fit.
                        .overflow_hidden()
                        .child(label.to_string()),
                )
                .into_any_element(),
        );
    }

    body.children(children).into_any_element()
}

/// A node's mark, as a small shape rather than an asset.
///
/// `render::registry` deliberately answers with a [`NodeGlyph`] and not an icon
/// path, so the engine carries no files and no strings; this is the one place
/// that decides what each one looks like, and a build with no assets still
/// draws every node.
fn glyph(visual: NodeVisual, accent: Hsla) -> Div {
    let mark = div().w(px(8.0)).h(px(8.0)).bg(accent);

    match visual.glyph {
        NodeGlyph::None => div().w(px(0.0)),
        NodeGlyph::Dot => div().flex().items_center().child(mark.rounded_full()),
        // A source and a sink read as arrows into and out of the node: a
        // half-rounded mark, mirrored.
        NodeGlyph::Inbound => div().flex().items_center().child(mark.rounded_r(px(4.0))),
        NodeGlyph::Outbound => div().flex().items_center().child(mark.rounded_l(px(4.0))),
        NodeGlyph::Process => div().flex().items_center().child(mark.rounded(px(2.0))),
        // A diamond is a square turned 45°, and a `div` cannot be turned — so
        // the decision node's mark is a chevron-ish sliver rather than a
        // pretend diamond. The *body* of a decision node is a real diamond,
        // painted on the canvas; see `render::snapshot`.
        NodeGlyph::Decision => div()
            .flex()
            .items_center()
            .child(div().w(px(4.0)).h(px(10.0)).bg(accent).rounded(px(1.0))),
        NodeGlyph::Note => div()
            .flex()
            .items_center()
            .child(div().w(px(8.0)).h(px(6.0)).bg(accent).rounded_t(px(2.0))),
    }
}

/// **§44's interactive handles**, for the selected or hovered node only.
///
/// The snapshot is what bounds this: it fills
/// [`RenderSnapshot::interactive_handles`] for one node, so there is no way to
/// reach "a handle element per visible node" from here.
pub fn handles(snapshot: &RenderSnapshot, cx: &App) -> Vec<AnyElement> {
    let theme = cx.theme();
    snapshot
        .interactive_handles()
        .iter()
        .map(|handle| self::handle(handle, theme.primary, theme.background))
        .collect()
}

fn handle(handle: &InteractiveHandle, fill: Hsla, ring: Hsla) -> AnyElement {
    let size = HANDLE_HIT_PIXELS;
    div()
        .absolute()
        .left(px(handle.center.x - size * 0.5))
        .top(px(handle.center.y - size * 0.5))
        .w(px(size))
        .h(px(size))
        .flex()
        .items_center()
        .justify_center()
        // The hit area is larger than the dot, so the element is transparent
        // and the dot is its child. A 14 px target with a 9 px mark is the
        // shape every graph editor uses.
        .id(("flow-handle", handle.handle.raw() as usize))
        .cursor_crosshair()
        .child(
            div()
                .w(px(9.0))
                .h(px(9.0))
                .rounded_full()
                .bg(fill)
                .border(px(1.5))
                .border_color(ring),
        )
        .into_any_element()
}

/// **§12's resize grips**: one small square at each corner of the selection
/// ring, for the one element the ring is around.
///
/// Drawn from the same [`SnapshotOverlay`](crate::render::snapshot::SnapshotOverlay)
/// the ring is, which is what keeps the *drawn* grips and the *hit-tested* ones
/// the same set — `views::flow`'s `target_at` asks the world for a grip only on
/// `snapshot.overlay()`'s node, so a frame that draws none can never resolve
/// one. Two lists built from two sources would be an invisible control stealing
/// presses, which is this crate's most-repeated failure in a new costume.
///
/// They are **not** hit-tested by GPUI: like every other canvas control, the
/// press is resolved against the world in one place. These are pixels with a
/// cursor on them.
pub fn resize_grips(snapshot: &RenderSnapshot, cx: &App) -> Vec<AnyElement> {
    let Some(overlay) = snapshot.overlay() else {
        return Vec::new();
    };
    // An element a few pixels across has no room for four grips, and drawing
    // them would cover the thing they resize.
    if !overlay.shows_resize_grips {
        return Vec::new();
    }

    let theme = cx.theme();
    let ring = overlay.screen.inflate(SELECTION_RING_INSET);

    ResizeCorner::ALL
        .iter()
        .copied()
        .map(|corner| {
            let center = corner.of(ring);
            div()
                .absolute()
                .left(px(center.x - GRIP_PIXELS * 0.5))
                .top(px(center.y - GRIP_PIXELS * 0.5))
                .w(px(GRIP_PIXELS))
                .h(px(GRIP_PIXELS))
                .rounded(px(2.0))
                .bg(theme.background)
                .border(px(1.5))
                .border_color(theme.selection)
                .into_any_element()
        })
        .collect()
}

/// §44's bounding box for the selected element: a ring outside the node, so it
/// reads as a selection rather than as a thicker border.
pub fn selection_box(snapshot: &RenderSnapshot, cx: &App) -> Option<AnyElement> {
    let overlay = snapshot.overlay()?;
    let ring = overlay.screen.inflate(SELECTION_RING_INSET);

    Some(
        placed(ring)
            .rounded(px(9.0))
            .border(px(1.0))
            .border_color(cx.theme().selection)
            .into_any_element(),
    )
}

/// An absolutely-positioned `div` at a pane-relative rectangle.
///
/// One place, so no element here can accidentally be laid out instead of
/// placed. See the module doc for why nothing is laid out.
fn placed(rect: Rect) -> Div {
    let rect = rect.normalized();
    div()
        .absolute()
        .left(px(rect.origin.x))
        .top(px(rect.origin.y))
        .w(px(rect.size.x))
        .h(px(rect.size.y))
}

/// A [`AccentRole`] against the active theme.
///
/// The registry answers in roles rather than colours precisely so this
/// conversion exists in one place and a theme change moves every node at once —
/// see `render::registry`.
pub fn accent_color(role: AccentRole, cx: &App) -> Hsla {
    let theme = cx.theme();
    match role {
        AccentRole::Neutral => theme.muted_foreground,
        AccentRole::Info => theme.info,
        AccentRole::Success => theme.success,
        AccentRole::Warning => theme.warning,
        AccentRole::Danger => theme.danger,
    }
}

/// The layer every rich element is placed in: absolutely positioned, filling
/// the pane, and **not hit-testable itself** — a full-pane element that
/// swallowed clicks would take every press away from the canvas underneath.
pub fn layer() -> Div {
    div().absolute().inset_0().w(relative(1.0)).h(relative(1.0))
}

/// The one element the overlay layer needs from a snapshot with nothing
/// selected: nothing at all.
pub fn is_empty(snapshot: &RenderSnapshot) -> bool {
    snapshot.rich().is_empty()
        && snapshot.interactive_handles().is_empty()
        && snapshot.overlay().is_none()
}
