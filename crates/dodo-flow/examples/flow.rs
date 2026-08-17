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
//! | drag on empty space with the left button | selection rectangle |
//! | `Esc` | abandon the drag |
//!
//! # Environment switches
//!
//! | Variable | Effect |
//! |---|---|
//! | `DODO_FLOW_SHAPES=n` | how many shapes the demo scene holds (default 1,000) |
//! | `DODO_FLOW_BENCH=1` | drive continuous frames and print frame timings |
//! | `DODO_FLOW_TRACE_INPUT=1` | print every mouse, scroll and pinch event received |
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
    models::{Color, ElementKind, FlowDocument, ShapeKind, StrokeStyle},
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

/// The demo document: the four shapes at readable size, then a field of them.
///
/// The field exists to make the performance claim checkable rather than
/// asserted — "pan and zoom stay smooth with ~1,000 shapes visible" is the
/// phase's exit condition, and this is the scene it is measured on. It is laid
/// out on a grid so that at the default zoom roughly all of it is on screen at
/// once, which is the case that costs the most: there is no culling yet, so an
/// off-screen shape would be just as expensive and the number would mean less.
fn demo_document(shapes: usize) -> FlowDocument {
    let mut document = FlowDocument::new();

    // The four shapes this phase draws, large enough to see the tessellation.
    let showcase = [
        (ShapeKind::Rectangle, 0.0, Color::rgb(0.29, 0.56, 0.85)),
        (
            ShapeKind::RoundedRectangle,
            18.0,
            Color::rgb(0.35, 0.72, 0.55),
        ),
        (ShapeKind::Ellipse, 0.0, Color::rgb(0.90, 0.60, 0.26)),
        (ShapeKind::Diamond, 0.0, Color::rgb(0.78, 0.40, 0.68)),
    ];

    for (index, (kind, radius, fill)) in showcase.into_iter().enumerate() {
        let id = document.add_node(
            ElementKind::Shape(kind),
            Vec2::new(60.0 + index as f32 * 220.0, 60.0),
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

    // The field. Rotating through the kinds keeps the mix honest — a scene of
    // nothing but rectangles would be quads only and would say nothing about
    // the path budget, and one of nothing but ellipses would be the worst case
    // rather than a realistic one.
    let kinds = [
        ShapeKind::Rectangle,
        ShapeKind::RoundedRectangle,
        ShapeKind::Ellipse,
        ShapeKind::Diamond,
    ];
    let columns = (shapes as f32).sqrt().ceil().max(1.0) as usize;

    for index in 0..shapes {
        let (column, row) = (index % columns, index / columns);
        let kind = kinds[index % kinds.len()].clone();
        let id = document.add_node(
            ElementKind::Shape(kind),
            Vec2::new(40.0 + column as f32 * 90.0, 260.0 + row as f32 * 70.0),
            Vec2::new(64.0, 44.0),
        );
        if let Some(node) = document.node_mut(id) {
            node.style.corner_radius = 8.0;
        }
    }

    document
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

        println!(
            "frame {:>6} | median {:>6.2} ms ({:>5.1} fps) | p95 {:>6.2} ms | worst {:>6.2} ms \
             | quads {:>6} paths {:>5} vertices {:>8} | grid level {} spacing {:.0}px | dropped {}",
            self.frames,
            median.as_secs_f64() * 1000.0,
            1.0 / median.as_secs_f64(),
            p95.as_secs_f64() * 1000.0,
            worst.as_secs_f64() * 1000.0,
            stats.quads,
            stats.paths,
            stats.path_vertices,
            grid.level,
            grid.screen_spacing,
            view.dropped_paths(),
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
    let shapes = env_count("DODO_FLOW_SHAPES", 1_000);
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
                        flow.set_document(demo_document(shapes));
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
