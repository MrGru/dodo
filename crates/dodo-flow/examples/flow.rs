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
//! | Gesture | Effect |
//! |---|---|
//! | drag with the middle button, or hold space and drag | pan |
//! | two-finger trackpad swipe | pan |
//! | trackpad pinch | zoom, anchored at the pointer |
//! | Cmd or Ctrl + scroll wheel | zoom, anchored at the pointer |
//! | drag a node's body with the left button | move it — and **only its own edges reroute** |
//! | drag out of a handle dot | a connection preview follows the pointer |
//! | drop it on a handle or a node body | connect, if §4's rules allow it |
//! | drop it on empty canvas | cancel |
//! | drag on empty space with the left button | rubber band — **it selects on release** |
//! | shift + drag on empty space | add the band's contents to the selection |
//! | `Esc` | abandon the drag — a moved node goes back exactly where it was |
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
        FlowDocument, GraphNodeKind, Handle, HandleDirection, HandlePlacement, ShapeKind,
        StrokeStyle,
    },
};
use gpui::{
    AppContext, AssetSource, Context, Entity, IntoElement, ParentElement, QuitMode, Render,
    SharedString, Styled, Window, WindowOptions, div, px, size,
};
use gpui_component::{ActiveTheme, Root};

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

        div()
            .size_full()
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .child(self.flow.clone())
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
