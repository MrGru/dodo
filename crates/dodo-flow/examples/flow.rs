//! Opens one window containing nothing but the Flow Canvas.
//!
//! ```sh
//! cargo run -p dodo-flow --example flow --locked
//! ```
//!
//! This mounts the real view and adds only the dialog and asset wiring that the
//! full app supplies around it — the shape every dodo feature crate's launcher
//! takes, and `crates/dodo-docker/examples/docker.rs` is the reference. Assets
//! are read from the repository, so editing an SVG needs a restart, not a
//! rebuild.
//!
//! **This is the only way to run the canvas today.** The sidebar row lands
//! last, deliberately, so nobody meets a half-built tool inside dodo.
//!
//! # What you can do in the window
//!
//! **Start at the palette, top left.** It is §45's tool strip and it decides
//! what a left press means; the eight buttons are Select, Hand, Rectangle,
//! Diamond, Ellipse, Arrow, Line and Graph node, in that order, each drawn as
//! the shape it creates. The active one is filled.
//!
//! | Tool gesture | Effect |
//! |---|---|
//! | click a palette button, or press its letter | pick that tool up — **it is armed and nothing is created**; the next press-drag on the canvas is what draws |
//! | with a shape tool, drag on the canvas | draw the element inside the box you drag — the preview is the real shape, not an outline of it |
//! | with a shape tool, **click** | place it at its default size, centred where you clicked |
//! | hold shift while drawing | square the box: a square, a circle, a regular diamond, a 45° line |
//! | finish a drawing | **back to Select**, so the thing you just drew can be moved — Excalidraw's own default |
//! | click the padlock, or press `q` | lock the tool: finishing a drawing keeps it, so six rectangles need one trip to the palette |
//! | `Esc` | abandon what is being drawn **and** go back to Select, lock or no lock |
//! | `Cmd+Z` / `Ctrl+Z` after drawing | remove it — **one press per element**, however long the drag was |
//!
//! ## Deleting
//!
//! | Gesture | Effect |
//! |---|---|
//! | select something, then `Delete` or `Backspace` | remove it |
//! | click the bin in the palette | the same thing — one method, two doors |
//! | delete a node | **its edges go with it**; an edge with one end nowhere has no geometry |
//! | `Cmd+Z` / `Ctrl+Z` | put it all back, node, edges and selection, in one press |
//!
//! The bin is drawn muted when nothing is selected, and clicking it then does
//! nothing. That is the state, not a bug.
//!
//! A creating tool draws over whatever is underneath, so a rectangle dragged
//! across a node is a rectangle and not a node drag. A graph node is born with
//! a source handle on its right and a target on its left, so it can be
//! connected the moment it exists; a drawn shape gets neither, because §4
//! refuses an edge to one anyway.
//!
//! **Two things about the Arrow and Line tools are worth knowing before you
//! wonder whether they are broken.** An arrow always points from the top-left
//! of the box you drew to its bottom-right — a node stores an origin and a
//! size and not a pair of endpoints, so the diagonal is the only direction it
//! can have, and drawing one leftwards still gives you an arrow pointing right.
//! And a linear element is grabbed by its whole bounding box rather than by the
//! line itself, so a long diagonal is selected from its empty corners too.
//! `render::shapes` and `runtime::hit` carry both, with the shape of the fix.
//!
//! # §9's text, and the six things to look at
//!
//! The document opens with a text row below the routing showcase: the four
//! sizes the property panel offers, each in a different family and alignment,
//! plus a labelled edge under them. Six things are worth checking by hand,
//! because no test in this crate can: shaping and hit-testing both need a live
//! window, and an unattended one presents its first frame and stops.
//!
//! 1. **Double-click a node, an edge and a text element, type, press `Enter`,
//!    then double-click the same thing again.** The field must come up holding
//!    what is there. An editor that opened blank would look identical until the
//!    second visit, and then quietly replace a sentence with a word.
//! 2. **Drag a labelled node.** Its own text moves with it, and the label on
//!    every edge attached to it rides the route as the route bends.
//! 3. **Empty a text element** — select all, delete, `Enter`. It goes, because
//!    a text element with no glyphs is invisible; one `Cmd+Z` brings it back
//!    with its words.
//! 4. **Pick the Hand-drawn family.** On a machine with none of the platform's
//!    hand-drawn faces installed it looks exactly like Normal, and that is the
//!    honest behaviour rather than a bug — dodo ships no font of its own. See
//!    [`FontFamily::preferred_faces`](dodo_flow::models::FontFamily::preferred_faces).
//! 5. **Click an edge — once, on the line** (Phase 10.5). It becomes the
//!    selection on its own, so `Delete` removes it and one `Cmd+Z` puts it
//!    back; shift-click a node as well and both go together. The two halves to
//!    watch for are that a press on *empty* canvas still starts a rubber band,
//!    and that the six-pixel band around a route stays the same width on screen
//!    when you zoom right in and right out.
//! 6. **Type a sentence into a node** (Phase 10.5). It wraps onto as many lines
//!    as it needs, at the node's own width, and stays wrapped while you drag
//!    the node about. Do it at 100 % zoom *and* zoomed out past the rung where
//!    a node stops being a rich element — the two paths are different code and
//!    must read the same. Long enough and it will overflow the node top and
//!    bottom; that is the recorded limitation, not a bug — `render::painter`
//!    says why clipping it would be worse.
//!
//! **The limitation that is left here.** dodo ships no hand-drawn face, so
//! choosing that family may change nothing on screen — item 4 above. The two
//! Phase 10 recorded beside it, an unclickable edge and single-line text, are
//! closed.
//!
//! # §32's property panel, and the eight things only a person can check
//!
//! The window opens with a node already selected, so the panel is on screen
//! from the first frame, under the tool palette on the left. **This is the most
//! visual phase the canvas has had**, and an unattended GPUI window on macOS
//! presents its first frame and then stops — so what a test can say is that the
//! right rows are chosen, that every control writes what it claims and that the
//! frame is planned in the right order. Whether it *reads* right is yours.
//!
//! 1. **Select a node, then an edge, then a text element.** The panel has to
//!    change under you: Background, Fill and Edges for the node; Arrow type and
//!    Arrowheads instead for the edge; Font family, size and alignment for the
//!    text, with nothing about strokes. Opacity, Layers and Actions stay on all
//!    three. Then select a node *and* a text element together — you should get
//!    only the rows that mean something to both.
//! 2. **Press a colour swatch and watch the canvas.** It must change under the
//!    press, not on the next click somewhere. Then press the swatch past the
//!    separator: a hex field opens holding the current value, `Enter` applies it
//!    and `Esc` abandons it. Type nonsense into it — `#12345` — and it must
//!    refuse rather than apply black.
//! 3. **Hover a swatch.** The hex is in the tooltip; the transparent one is a
//!    checkerboard rather than an empty square.
//! 4. **Drag the opacity slider the whole way and let go, then press `Cmd+Z`
//!    once.** One press has to put it back where it started — not sixty, and not
//!    to somewhere in the middle of the drag.
//! 5. **Draw a rectangle over an ellipse and use the four Layers buttons.** This
//!    is the phase's hardest claim and the one most likely to be subtly wrong:
//!    the two shapes are drawn by different halves of the renderer, and the
//!    ordering is honoured by moving one of them between the halves. Watch for a
//!    node that *changes appearance* when it goes behind something — losing its
//!    accent bar and its hover highlight is the recorded price and is expected;
//!    losing its label or its border is not. **A text element will not go behind
//!    a shape's fill**, which is recorded rather than a bug.
//! 6. **Switch to hand-drawn (`s`) and use the Sloppiness row.** Three visibly
//!    different hands, and each element keeps its own — set two shapes to two
//!    steps and they must stay different. Switch back to Clean and the row goes
//!    muted with a tooltip saying why, rather than silently doing nothing.
//! 7. **Set Fill to hachure and to cross-hatch.** The interior fills with lines
//!    in the background colour rather than flooding, the lines stop at the
//!    border on every shape (try the ellipse and the diamond, which is where a
//!    scanline goes wrong), and zooming right in coarsens the hatch instead of
//!    hanging.
//! 8. **Duplicate, then link.** Duplicate offsets the copy and selects it, so a
//!    second press walks it across the canvas; the Link button opens a field,
//!    and `Cmd`/`Ctrl`-clicking the element afterwards opens the URL in your
//!    browser. `Cmd+Z` has to undo each of these in one press.
//!
//! **And the one thing to check that is not a feature**: pan and zoom around
//! with something selected. The panel is chrome and must not move with the
//! document, and the canvas must stay as responsive as it was in Phase 10 —
//! `DODO_FLOW_REPORT=1` prints the first frame's batch count, which should
//! still be **1**.
//!
//! # §10's pictures, and the seven things only a person can check
//!
//! **Two of them are already in the document**, at the bottom of the left-hand
//! column below the labelled edge — pan down, or press the zoom-to-fit button
//! and look at the lower left. They show **one** picture: the left element
//! whole, the right one squashed, at 65 % opacity with rounded corners. That
//! pairing is the phase's own rule made visible — the document holds one copy of
//! the bytes however many elements show it.
//!
//! 1. **Insert one.** The picture button in the palette, or `i`. Pick a PNG or a
//!    JPEG; it lands in the middle of the view at its own pixel size, shrunk to
//!    fit if it is large, and already selected. Pick the *same file* twice: two
//!    elements, one copy of the bytes. Pick something that is not an image and
//!    dodo has to say so rather than doing nothing.
//! 2. **Drag a corner.** An image resizes with its proportions kept, whatever
//!    the pointer does — that is §10's aspect lock, and it is the opposite
//!    default from a shape. **Hold shift and the lock comes off**, which is the
//!    one place shift means "release" rather than "constrain" and is worth
//!    feeling once. The grips are the four small squares on the selection ring;
//!    they appear only for a single selection and only when the element is big
//!    enough for a toolbar.
//! 3. **Crop it.** Squash the frame with a shift-drag, then press Crop in the
//!    panel's Actions row: the picture stops being stretched and what you see is
//!    a window on the middle of it. Press it again and the whole picture comes
//!    back, with the frame's height following. With nothing to do the button is
//!    muted and its tooltip says which gesture makes something — the same
//!    answer Sloppiness gives in clean mode.
//! 4. **Send it behind a shape.** Draw an ellipse over a picture: it is over it
//!    already, because everything drawn as a path is. Now press Bring to front
//!    on the picture — **the whole image layer moves**, so it goes over the
//!    ellipse. *Expected to look odd*: with two pictures on opposite sides of
//!    one ellipse, only the lower one is honoured, because the layer moves as a
//!    whole. `render::plan`'s doc carries it.
//! 5. **Duplicate it, then delete it, then undo everything.** Each of these is
//!    one press of `Cmd+Z` — the insert, the move, the resize, the crop, the
//!    duplicate and the delete — and the picture has to come back with its crop.
//! 6. **Drag the opacity slider on a picture.** It has to fade as you drag, and
//!    come back in one press of undo.
//! 7. **Set Edges to Round on the uncropped picture and on the cropped one.**
//!    *Expected to look odd, and recorded*: the uncropped one gets rounded
//!    corners and the **cropped one does not**. A crop is a clip, and GPUI's
//!    clip is a rectangle with no radii; `views::images` says so where it is
//!    caused.
//!
//! **Hover a palette button and it tells you what it is and which key it
//! answers to.** The label comes from `dodo_i18n::flow`, in whichever language
//! dodo is set to; the keystroke beside it is looked up from the real binding
//! table, so it is right by construction rather than by being kept in step.
//! The letters, for reference:
//!
//! | `v` | `h` | `r` | `d` | `o` | `a` | `l` | `n` | `t` | `q` | `i` |
//! |---|---|---|---|---|---|---|---|---|---|---|
//! | select | hand | rectangle | diamond | ellipse | arrow | line | node | text | lock | image |
//!
//! ## Everything else, which is the Select tool's
//!
//! | Gesture | Effect |
//! |---|---|
//! | drag with the middle button, or hold space and drag | pan, under any tool |
//! | pick up the Hand tool, or press `h` | pan with the left button too |
//! | two-finger trackpad swipe | pan |
//! | trackpad pinch | zoom, anchored at the pointer |
//! | Cmd or Ctrl + scroll wheel | zoom, anchored at the pointer |
//! | drag a node's body with the left button | move it — and **only its own edges reroute** |
//! | drag out of a handle dot | a connection preview follows the pointer |
//! | drop it on a handle or a node body | connect, if §4's rules allow it |
//! | drop it on empty canvas | cancel |
//! | **click an edge's line** | it becomes the selection — `Delete` then removes it |
//! | **shift-click a node or an edge** | add it to the selection instead of replacing it |
//! | drag on empty space with the left button | rubber band — **it selects on release** |
//! | shift + drag on empty space | add the band's contents to the selection |
//! | **double-click a node** | edit its text, seeded with whatever is already there |
//! | **double-click an edge** | edit its label — aim at the line, not at the space around it |
//! | **double-click empty canvas** | place text under the pointer and start typing |
//! | **`t`, then click or drag** | the same, with a box you chose |
//! | `Enter`, or a click anywhere | commit what you typed |
//! | `Esc` while typing | abandon it — nothing reaches the document, so nothing is undone |
//! | `Delete` / `Backspace` | remove whatever is selected, nodes and edges alike |
//! | `Esc` | abandon the drag — a moved node goes back exactly where it was, and the drag leaves no undo step |
//! | `Cmd+Z` / `Ctrl+Z` | undo — **a whole drag is one press**, however many mouse moves it took |
//! | `Cmd+Shift+Z` / `Ctrl+Shift+Z` / `Ctrl+Y` | redo |
//!
//! Every gesture above needing the Select tool is what `v` or `Esc` gets you
//! back to. The keys are `commands::keys`'s table rather than constants in a
//! handler (§26), so they are the same on every platform and the whole table is
//! asserted from any machine.
//!
//! **If a key does nothing, suspect focus first.** GPUI dispatches a key event
//! down the focus path and every canvas binding is scoped to
//! `FlowView::KEY_CONTEXT`, so a canvas that does not hold the focus has all of
//! them silently dead — that was true from Phase 2 to Phase 7 and nothing
//! reported it. Clicking the canvas, or any palette button, takes the focus
//! back.
//!
//! A connection is refused silently on the canvas: an input handle will not
//! take a second edge past its limit, a source will not connect to a source,
//! and a node will not connect to itself. `DODO_FLOW_TRACE_INPUT=1` prints the
//! reason. Colouring the handle by that reason is Phase 5's, where handles
//! become interactive elements.
//!
//! # Environment switches
//!
//! | Variable | Effect |
//! |---|---|
//! | `DODO_FLOW_NODES=n` | how many nodes the connected field holds (default 400) |
//! | `DODO_FLOW_BENCH=1` | drive continuous frames and print frame timings |
//! | `DODO_FLOW_TRACE_INPUT=1` | print every mouse, scroll and pinch event received |
//! | `DODO_FLOW_INSTRUMENT=1` | record §39's probes; read them with `FlowView::instruments` |
//! | `DODO_FLOW_REPORT=1` | print one line after the first painted frame: §15's rung, §16's element count, §23's cache |
//! | `DODO_FLOW_ZOOM=z` | open at this zoom, to see the LOD ladder without touching the trackpad |
//! | `DODO_FLOW_SKETCH=1` | open hand-drawn (§13), the same as clicking **Sketch** |
//!
//! # The launcher's own two buttons are English on purpose
//!
//! The canvas's strings go through `dodo_i18n::flow` from Phase 9 on, and
//! **this file's do not**. The Clean/Sketch toggle is a developer harness: it
//! is an `examples/` target nothing a shipped build can reach, `i18n_lint`
//! does not scan it, and giving it catalogue entries would put two strings in
//! the app's catalogue that the app never draws — the same objection
//! `commands::keys` makes to a binding no code reads. If the render-style
//! toggle becomes a real control, its strings are added then, with a caller.
//!
//! # The clean/sketch toggle
//!
//! The two buttons at the top right switch §13's render style. **It is a
//! renderer strategy, not a document type**: the click writes one field and
//! asks for a repaint — no element is created, moved or rewritten, which
//! `runtime::world`'s `switching_render_style_touches_no_element` asserts and
//! which is what makes the switch instant on a 100,000-node document. Open with
//! `DODO_FLOW_NODES=100000` and click between them to see it.
//!
//! Zoom out past 0.35 and the buttons stop making a difference: the ladder
//! degrades sketch to clean below the zoom at which a 2 px wobble is visible.
//! That is [`render::lod`]'s decision, and the same rung drops it on a scene
//! with too many visible bodies to draw by hand.
//!
//! `DODO_FLOW_BENCH` deliberately does the thing §35 forbids — it requests an
//! animation frame from every paint, so the canvas repaints as fast as the
//! platform allows — because a frame time is only measurable if frames are
//! being produced. It is the *measurement* harness, not the behaviour; without
//! it the canvas repaints only when something changes.

use std::{
    borrow::Cow,
    path::PathBuf,
    time::{Duration, Instant},
};

use dodo_flow::{
    FlowView,
    geometry::Vec2,
    models::{
        ArrowMarker, Color, DashPattern, EdgeRouting, ElementId, ElementKind, Endpoint,
        FlowDocument, FontFamily, FontSize, GraphNodeKind, Handle, HandleDirection,
        HandlePlacement, ImageFormat, ImageResource, NodeImage, NodeIndex, RenderStyle, ShapeKind,
        StrokeStyle, TextAlign, decode_base64,
    },
    render::registry::GenericKind,
};
use gpui::{
    AppContext, AssetSource, Context, Entity, IntoElement, ParentElement, QuitMode, Render,
    SharedString, Styled, Window, WindowOptions, div, px, size,
};
use gpui_component::{
    ActiveTheme, Root, Sizable,
    button::{Button, ButtonVariants},
};

struct Assets;

impl AssetSource for Assets {
    fn load(&self, path: &str) -> gpui::Result<Option<Cow<'static, [u8]>>> {
        let file = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets")
            .join(path);

        match std::fs::read(file) {
            Ok(bytes) => Ok(Some(Cow::Owned(bytes))),
            Err(_) => gpui_component_assets::Assets.load(path),
        }
    }

    fn list(&self, path: &str) -> gpui::Result<Vec<SharedString>> {
        gpui_component_assets::Assets.list(path)
    }
}

fn env_zoom() -> Option<f32> {
    std::env::var("DODO_FLOW_ZOOM")
        .ok()
        .and_then(|value| value.parse().ok())
}

fn env_count(name: &str, default: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

/// The demo document: **one row per routing style**, then the drawn shapes,
/// then a field of connected nodes.
///
/// The showcase row is the phase's own acceptance check made visible — every
/// routing of §8 with its markers, side by side, so a wrong control point or a
/// backwards arrow head is obvious rather than plausible. The field exists to
/// make the performance claim checkable rather than asserted: dragging a node
/// out of it must reroute four edges and no more, and the launcher prints how
/// many it actually rerouted.
fn demo_document(nodes: usize) -> FlowDocument {
    let mut document = FlowDocument::new();

    // ---- one row per routing, with a different marker pair each ----------
    let showcase = [
        (
            EdgeRouting::Straight,
            "straight",
            ArrowMarker::None,
            ArrowMarker::ArrowClosed,
            false,
        ),
        (
            EdgeRouting::Bezier,
            "bezier",
            ArrowMarker::Arrow,
            ArrowMarker::Arrow,
            false,
        ),
        (
            EdgeRouting::SimpleBezier,
            "simple bezier",
            ArrowMarker::Dot,
            ArrowMarker::ArrowClosed,
            false,
        ),
        (
            EdgeRouting::Step,
            "step",
            ArrowMarker::None,
            ArrowMarker::Diamond,
            false,
        ),
        (
            EdgeRouting::SmoothStep,
            "smooth step",
            ArrowMarker::None,
            ArrowMarker::ArrowClosed,
            // The expensive kind, on exactly one edge — see
            // `render::plan::PathPaint` for what a dash costs.
            true,
        ),
    ];

    for (row, (routing, label, start_marker, end_marker, dashed)) in
        showcase.into_iter().enumerate()
    {
        let y = 60.0 + row as f32 * 130.0;
        let source = graph_node(&mut document, label, Vec2::new(60.0, y));
        let target = graph_node(&mut document, "in", Vec2::new(460.0, y + 40.0));

        let edge = document.add_edge(
            Endpoint::handle(source, "out"),
            Endpoint::handle(target, "in"),
        );
        if let Some(edge) = document.edges.iter_mut().find(|e| e.id == edge) {
            edge.routing = routing;
            edge.style.start_marker = start_marker;
            edge.style.end_marker = end_marker;
            edge.style.stroke.width = 2.0;
            if dashed {
                edge.style.stroke.dash = DashPattern::new(vec![7.0, 5.0]);
            }
        }
    }

    // §4's whole-node connection mode: neither end names a handle, so the
    // router picks a floating point on the border facing the other node —
    // drag either one and watch the attachment slide around it.
    let floating_a = graph_node(&mut document, "floating", Vec2::new(60.0, 760.0));
    let floating_b = graph_node(&mut document, "floating", Vec2::new(460.0, 830.0));
    let floating = document.add_edge(Endpoint::node(floating_a), Endpoint::node(floating_b));
    if let Some(edge) = document.edges.iter_mut().find(|e| e.id == floating) {
        edge.routing = EdgeRouting::Bezier;
        edge.style.end_marker = ArrowMarker::ArrowClosed;
        edge.style.stroke.width = 2.0;
    }

    // ---- §9's text, in all three places it can live -----------------------
    //
    // **Here so the visual pass has something to open onto**, rather than
    // something to build first. Each of the three is the thing a double-click
    // edits: the label on a node, the label on an edge, and a standalone text
    // element that *is* its glyphs.
    //
    // The four sizes are the property panel's S / M / L / XL, drawn beside each
    // other so a change to `FontSize::world_size` is visible rather than merely
    // compiled. `HandDrawn` falls back to the UI font on a machine with no
    // hand-drawn face installed — see `FontFamily::preferred_faces`.
    for (index, (size, family, align)) in [
        (FontSize::Small, FontFamily::Normal, TextAlign::Left),
        (FontSize::Medium, FontFamily::Code, TextAlign::Center),
        (FontSize::Large, FontFamily::HandDrawn, TextAlign::Right),
        (FontSize::ExtraLarge, FontFamily::Normal, TextAlign::Left),
    ]
    .into_iter()
    .enumerate()
    {
        let id = document.add_node(
            ElementKind::Text,
            Vec2::new(60.0, 1_100.0 + index as f32 * 48.0),
            Vec2::new(360.0, size.world_size() * 1.4),
        );
        if let Some(node) = document.node_mut(id) {
            node.label = Some(format!(
                "{} · {} · {}",
                size.name(),
                family.name(),
                align.name()
            ));
            node.style.font.size = size;
            node.style.font.family = family;
            node.style.font.align = align;
        }
    }

    // An edge with a label on it. Drag either end and the label rides the route
    // — the position is read from the route every frame rather than stored, so
    // §19's propagation is what moves it.
    let labelled_a = graph_node(&mut document, "from", Vec2::new(60.0, 1_320.0));
    let labelled_b = graph_node(&mut document, "to", Vec2::new(460.0, 1_390.0));
    let labelled = document.add_edge(
        Endpoint::handle(labelled_a, "out"),
        Endpoint::handle(labelled_b, "in"),
    );
    if let Some(edge) = document.edges.iter_mut().find(|e| e.id == labelled) {
        edge.label = Some("double-click me".to_owned());
        edge.style.end_marker = ArrowMarker::ArrowClosed;
        edge.style.stroke.width = 2.0;
    }

    // ---- the drawn shapes, unchanged from the canvas foundation ----------
    let shapes = [
        (ShapeKind::Rectangle, 0.0, Color::rgb(0.29, 0.56, 0.85)),
        (
            ShapeKind::RoundedRectangle,
            18.0,
            Color::rgb(0.35, 0.72, 0.55),
        ),
        (ShapeKind::Ellipse, 0.0, Color::rgb(0.90, 0.60, 0.26)),
        (ShapeKind::Diamond, 0.0, Color::rgb(0.78, 0.40, 0.68)),
    ];

    for (index, (kind, radius, fill)) in shapes.into_iter().enumerate() {
        let id = document.add_node(
            ElementKind::Shape(kind),
            Vec2::new(760.0 + index as f32 * 220.0, 60.0),
            Vec2::new(180.0, 120.0),
        );
        if let Some(node) = document.node_mut(id) {
            node.style.fill = Some(fill);
            node.style.corner_radius = radius;
            node.style.stroke = StrokeStyle {
                color: Some(Color::rgba(1.0, 1.0, 1.0, 0.85)),
                width: 2.0,
                ..StrokeStyle::default()
            };
        }
    }

    // ---- §43's registry: one node per generic kind ------------------------
    //
    // Six kinds, and three of them — Process, Decision, Note — have no
    // `ElementKind` variant at all. They reach the registry by name, through
    // the same public path a third party would register against, which is what
    // makes this row a demonstration rather than a decoration. The decision
    // node comes out a diamond, and a diamond is painted on the canvas rather
    // than made an element, because a `div` cannot be one.
    for (index, kind) in GenericKind::ALL.iter().enumerate() {
        let id = document.add_node(
            kind.element_kind(),
            Vec2::new(60.0 + index as f32 * 190.0, 980.0),
            Vec2::new(170.0, 60.0),
        );
        if let Some(node) = document.node_mut(id) {
            node.label = Some(kind.id().trim_start_matches("dodo.flow.").to_owned());
            node.handles = vec![
                Handle::new("out", HandlePlacement::Right, HandleDirection::Source),
                Handle::new("in", HandlePlacement::Left, HandleDirection::Target),
            ];
        }
    }

    // ---- §10's picture, so the visual pass has one to work on ------------
    //
    // Two elements showing **one** resource, which is the phase's own rule made
    // visible: crop one, move the other, and the document still holds one copy
    // of the bytes. The second is deliberately squashed, so the Crop button
    // starts in its "crop to the frame" state and the aspect lock has something
    // to undo.
    if let Some(bytes) = decode_base64(DEMO_PICTURE) {
        let handle = document.insert_image(ImageResource::new(
            ImageFormat::Png,
            DEMO_PICTURE_SIZE.0,
            DEMO_PICTURE_SIZE.1,
            bytes,
        ));

        let whole = document.add_node(
            ElementKind::Image,
            Vec2::new(60.0, 1_540.0),
            Vec2::new(256.0, 192.0),
        );
        if let Some(node) = document.node_mut(whole) {
            node.image = Some(NodeImage::new(handle));
        }

        let squashed = document.add_node(
            ElementKind::Image,
            Vec2::new(360.0, 1_540.0),
            Vec2::new(140.0, 192.0),
        );
        if let Some(node) = document.node_mut(squashed) {
            node.image = Some(NodeImage::new(handle));
            node.style.opacity = 0.65;
            node.style.corner_radius = 10.0;
        }
    }

    // ---- the field: connected graph nodes, laid out on a grid ------------
    //
    // Every node is wired to the one after it and to the one a row below, so
    // the field is a real graph rather than a scatter — which is what makes
    // dragging a node out of it a fair test of the propagation rule.
    let columns = (nodes as f32).sqrt().ceil().max(1.0) as usize;
    let mut field = Vec::with_capacity(nodes);

    for index in 0..nodes {
        let (column, row) = (index % columns, index / columns);
        field.push(graph_node(
            &mut document,
            "node",
            Vec2::new(760.0 + column as f32 * 220.0, 260.0 + row as f32 * 110.0),
        ));
    }

    for (index, node) in field.iter().enumerate() {
        for neighbour in [index + 1, index + columns] {
            let Some(other) = field.get(neighbour) else {
                continue;
            };
            let edge = document.add_edge(
                Endpoint::handle(*node, "out"),
                Endpoint::handle(*other, "in"),
            );
            if let Some(edge) = document.edges.iter_mut().find(|e| e.id == edge) {
                edge.style.end_marker = ArrowMarker::ArrowClosed;
            }
        }
    }

    document
}

/// **The demo picture**, as the text a document would hold it as.
///
/// A 64×48 PNG — a diagonal ramp with a bright band across it, which is the
/// pattern that makes a *crop* obvious at a glance: whichever part of the band
/// is showing tells you which part of the source the element is a window on.
///
/// Carried as base64 rather than as a file next to this one, and that is worth
/// a sentence: an example that read an asset from disk would be an example that
/// stops working when the repository is not the working directory, and a binary
/// blob checked in for one launcher is a binary blob nobody can review. The
/// string below is exactly what
/// [`FlowDocument::images`](dodo_flow::models::FlowDocument::images) writes for
/// this picture, through the same [`decode_base64`] the loader uses.
const DEMO_PICTURE: &str = concat!(
    "iVBORw0KGgoAAAANSUhEUgAAAEAAAAAwCAYAAAChS3wfAAACyUlEQVR42uXSW0iUQRjG8W8h",
    "u4ioEKEIE7EDYvVgJdFBrESKxEokKRI7iVHYScQirERMooOIlRSJWIlRSGIlRiRWYkVhJduR",
    "EImK6EIiIrpYAvMlh5R5lXX3O8w3O/BnmJu5+PF4fD5fnzFwwsfCGNN/B1uYCX/Y9adnMIA4",
    "U8chtAHoRE1EaAAYE2L76PG71yshxEQgBADC/wFQP795uTEYcVOgMcDk/wDU9888AqZBU4DI",
    "oQCirz0yxPwYaAgQzQNQnz7wa1gyCxoBzBwegOp5wyMkzYYmAHEjA4jedskQyfHQAAD+AVDe",
    "Tn4NqQlwMcAC/wGoF094hLWL4VKARaMDED1ulyEykuBCgMTAAKj2Nn4NWclwEcCKwAGotrs8",
    "wpZVcAnAyuAARC3NMkROGlwAkGoOAHWriV9DXjoUBlhnHgDV2MAj7MuEogDrzQUQH9dflSEK",
    "NkFBgI3WANB9+Qq/hqLNUAgg2zoAqqaGRyjOgSIA260FEFVdkCFKd0IBgB32AFCV5/g1nNoN",
    "BwHy7AOgyit4hIp8OASw314AUdlJGeLsATgAUOgMAFVSxq+hugg2AhxyDoA6UsIj1BbDJoCj",
    "zgKICg/LEHXHYANAqRoAVP5Bfg0NJ2AhwHF1AKg9BTxCUzksAjitFoAod68M0XwGFgBUqglA",
    "bdvFr6H1PEwEqFIXgMrO5REeVMMkgItqA4gyt8oQHZdgAkCtOwDoz/Qsfg2d9QgCoM49AHSn",
    "beARuq4jQIBr7gIQpWTIEK8bEQDADXcCUMvX8Gvovo1RANx0LwCVuJpH+HgHfgK0uBtAlJAi",
    "Q3xphR8A9/QAoOKX8WvofYgRAO7rA0DNWcoj/HiEYQA69AIQzVgoQ/x6BgbgqZ4AVPQ8fg1/",
    "XmIQwHN9AajIuTyC8QoDAF69AUQRsTJE2Hv0A7wLDQBq0nQZwWN0hw4ANT5qKMJf56YFxBnh",
    "DKcAAAAASUVORK5CYII=",
);

/// The demo picture's pixel dimensions. Stated rather than decoded, because a
/// resource carries its own size — see
/// [`ImageResource`](dodo_flow::models::ImageResource) — and this file is the
/// one place that knows what it made.
const DEMO_PICTURE_SIZE: (u32, u32) = (64, 48);

/// One graph node with an output on its right and an input on its left.
///
/// Two handles rather than one is what makes the field a directed graph and
/// what gives the connection tool something to drag out of; §4's placement,
/// direction and limit all show up here.
fn graph_node(document: &mut FlowDocument, label: &str, position: Vec2) -> ElementId {
    let id = document.add_node(
        ElementKind::GraphNode(GraphNodeKind::Default),
        position,
        Vec2::new(160.0, 56.0),
    );

    if let Some(node) = document.node_mut(id) {
        node.label = Some(label.to_owned());
        node.handles = vec![
            Handle::new("out", HandlePlacement::Right, HandleDirection::Source),
            Handle::new("in", HandlePlacement::Left, HandleDirection::Target),
        ];
    }

    id
}

/// Rolling frame timings, printed from the benchmark mode.
struct FrameTimer {
    last: Instant,
    samples: Vec<Duration>,
    frames: u64,
}

impl FrameTimer {
    fn new() -> FrameTimer {
        FrameTimer {
            last: Instant::now(),
            samples: Vec::with_capacity(240),
            frames: 0,
        }
    }

    /// Records one frame and reports a summary every 120.
    fn tick(&mut self, view: &FlowView) {
        let now = Instant::now();
        self.samples.push(now - self.last);
        self.last = now;
        self.frames += 1;

        if self.samples.len() < 120 {
            return;
        }

        self.samples.sort_unstable();
        let median = self.samples[self.samples.len() / 2];
        let p95 = self.samples[self.samples.len() * 95 / 100];
        let worst = *self.samples.last().expect("just sorted a non-empty vec");
        let stats = view.last_paint_stats();
        let grid = view.last_grid_level();
        let visible = view.visible();

        println!(
            "frame {:>6} | median {:>6.2} ms ({:>5.1} fps) | p95 {:>6.2} ms | worst {:>6.2} ms \
             | visible {:>5}n/{:>5}e of {}n/{}e | quads {:>6} paths {:>5} vertices {:>8} \
             | batches {} | grid {} @{:.0}px | rerouted {} | culled {} | dropped {} | selected {}",
            self.frames,
            median.as_secs_f64() * 1000.0,
            1.0 / median.as_secs_f64(),
            p95.as_secs_f64() * 1000.0,
            worst.as_secs_f64() * 1000.0,
            // §16's rule, live: these two must stay a screenful however large
            // the document behind them is.
            visible.node_count(),
            visible.edge_count(),
            view.world().nodes().len(),
            view.world().edges().len(),
            stats.quads,
            stats.paths,
            stats.path_vertices,
            stats.path_batches,
            grid.level,
            grid.screen_spacing,
            // §19's number, live: zero while panning or idle, and the dragged
            // node's degree while a node is being dragged.
            view.rebuilt_routes(),
            stats.culled_paths,
            view.dropped_paths(),
            view.selection().len(),
        );

        self.samples.clear();
    }
}

struct FlowWindow {
    flow: Entity<FlowView>,
    timer: Option<FrameTimer>,
}

/// One half of the clean/sketch toggle.
///
/// The entity is captured rather than reached through `cx.listener`, because a
/// `Button`'s click handler is handed an `&mut App` and not a `Context<Self>` —
/// see `gpui-component-recipes`.
fn style_button(
    id: &'static str,
    label: &'static str,
    style: RenderStyle,
    flow: Entity<FlowView>,
    current: RenderStyle,
) -> Button {
    let button = Button::new(id).small().label(label);
    let button = if current == style {
        button.primary()
    } else {
        button.ghost()
    };

    button.on_click(move |_, _, cx| {
        flow.update(cx, |view, cx| view.set_render_style(style, cx));
    })
}

impl Render for FlowWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(timer) = self.timer.as_mut() {
            timer.tick(self.flow.read(cx));
            // Deliberately the idle loop §35 forbids — see the module doc.
            // `request_animation_frame` defers the notify to *after* the frame,
            // which is the only way it schedules anything: a bare `cx.notify()`
            // from inside paint marks the view dirty and never redraws.
            window.request_animation_frame();
        }

        let current = self.flow.read(cx).render_style();

        div()
            .size_full()
            .relative()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(self.flow.clone())
            .child(
                // §13's toggle. Absolutely positioned over the canvas rather
                // than in a bar beside it: the launcher's whole job is to show
                // the canvas, and the canvas measures its own pane.
                div()
                    .absolute()
                    .top(px(12.0))
                    .right(px(12.0))
                    .flex()
                    .gap(px(6.0))
                    .child(style_button(
                        "flow-style-clean",
                        "Clean",
                        RenderStyle::Clean,
                        self.flow.clone(),
                        current,
                    ))
                    .child(style_button(
                        "flow-style-sketch",
                        "Sketch",
                        RenderStyle::Sketch,
                        self.flow.clone(),
                        current,
                    )),
            )
            .children(Root::render_dialog_layer(window, cx))
    }
}

fn main() {
    let nodes = env_count("DODO_FLOW_NODES", 400);
    let benchmarking = std::env::var_os("DODO_FLOW_BENCH").is_some();

    gpui_platform::application()
        .with_assets(Assets)
        .with_quit_mode(QuitMode::LastWindowClosed)
        .run(move |cx| {
            gpui_component::init(cx);
            // §26's bindings, from `commands::keys`'s table. After
            // `gpui_component::init`, so the canvas's context wins the tie with
            // the component library's own.
            dodo_flow::init(cx);
            cx.activate(true);

            let options = WindowOptions {
                window_min_size: Some(size(px(720.), px(480.))),
                ..Default::default()
            };

            cx.open_window(options, |window, cx| {
                let view = cx.new(|cx| {
                    let flow = cx.new(|cx| {
                        let mut flow = FlowView::new(window, cx);
                        flow.set_document(demo_document(nodes));

                        // **The acceptance check, made visible on the first
                        // frame.** §44's controls belong to the selected
                        // element, so a launcher that selects nothing shows
                        // none of them — and the whole point of opening this
                        // window is to look at them.
                        flow.editor_mut().select_only(Some(NodeIndex::new(0)));

                        // §15 without a trackpad: `DODO_FLOW_ZOOM=0.4` opens in
                        // the compact rung and `0.1` in the overview one, which
                        // is how the ladder gets checked by eye rather than
                        // only by test.
                        if let Some(zoom) = env_zoom() {
                            flow.viewport_mut().zoom_around(Vec2::ZERO, zoom);
                        }
                        // **`DODO_FLOW_PICTURES=1` opens onto §10's row.** An
                        // unattended window presents one frame and stops, so
                        // the only way to see the picture path run — decode,
                        // prepaint, paint — in a report is to open the camera
                        // on it. It is a switch rather than the default because
                        // the first thing to look at is still the panel.
                        if std::env::var_os("DODO_FLOW_PICTURES").is_some() {
                            let pane = flow.viewport().size();
                            flow.viewport_mut().center_world_on_screen(
                                Vec2::new(280.0, 1_640.0),
                                Vec2::new(pane.x * 0.5, pane.y * 0.5),
                            );
                        }
                        if std::env::var_os("DODO_FLOW_SKETCH").is_some() {
                            flow.editor_mut().set_render_style(RenderStyle::Sketch);
                        }
                        if benchmarking {
                            // One line before the first frame, so a run that is
                            // only watched for a second still says what it
                            // built and what the spatial index made of it.
                            println!(
                                "scene: {} nodes, {} edges — index over {} node cells, \
                                 {} edge cells, {:.2} MB",
                                flow.world().nodes().len(),
                                flow.world().edges().len(),
                                flow.spatial().nodes().entry_count(),
                                flow.spatial().edges().entry_count(),
                                flow.spatial().memory_bytes() as f64 / 1e6,
                            );
                        }
                        flow
                    });
                    FlowWindow {
                        flow,
                        timer: benchmarking.then(FrameTimer::new),
                    }
                });
                cx.new(|cx| Root::new(view, window, cx))
            })
            .expect("failed to open window");
        });
}
