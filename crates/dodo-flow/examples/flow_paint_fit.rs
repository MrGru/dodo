//! **The windowed spike Phase 1 asked for and Phases 1–4 could not run**: a
//! re-fit of [`RenderBudgets::nanos_per_path`] and
//! [`RenderBudgets::nanos_per_vertex`].
//!
//! ```sh
//! cargo run --release -p dodo-flow --example flow_paint_fit --locked
//! ```
//!
//! # Why this could not be a headless benchmark
//!
//! Those two coefficients describe `Window::paint_path` — the clone (it
//! consumes its argument), the `scale`, the `insert_primitive` clone and the
//! Metal renderer's per-batch expansion to 104 bytes a vertex. Four traversals
//! of the vertex array, every one of them behind a real `Window`.
//! `flow_scene_bench` is headless and can only measure *tessellation*, which is
//! a different cost. So the pair stayed an honest over-estimate, fitted over
//! the degraded region of Phase 0's sweep, and Phase 4's harness recorded that
//! it predicted ~13 ms for a frame measured at 5.0 ms — 2.6× high.
//!
//! Phase 5 is the first phase with a window, so this is that measurement.
//!
//! # What it does
//!
//! Builds a set of paths once, then paints `paths` of them per frame, cloning
//! each into `paint_path` exactly as [`crate::render::painter::WindowPainter`]
//! does — the clone is part of the cost and Phase 0 §3 correction 13 says there
//! is no borrow-based alternative without patching gpui. It times **only** that
//! loop, sweeps path count against vertices per path, and takes the **minimum**
//! frame time per configuration: the minimum is the one statistic that is not
//! contaminated by whatever else the laptop is doing.
//!
//! Then it least-squares fits `micros = a·paths + b·vertices` over the sweep
//! and prints the two constants to paste into [`crate::budgets`].
//!
//! # It has to be run by a person, and that is the finding
//!
//! **Phase 5 had a window and still could not take this measurement
//! unattended.** Two runs printed `config 1/10` and then sat at 0 % CPU until
//! they were killed: an unattended GPUI window on macOS presents its first
//! frame and stops, because `request_animation_frame` rides a display link the
//! window server does not drive for an app nobody is looking at. It is the same
//! wall Phase 0 hit posting synthetic input events.
//!
//! So this harness is committed **ready to run**, not run. Somebody sitting in
//! front of the machine gets the numbers in about ten seconds; nothing
//! automated will. `budgets`'s module doc records what was decided in the
//! meantime — the composite `predicted_paint_micros` was deleted rather than
//! left as an unvalidated model, and the two per-unit coefficients stayed
//! because they are separately load-bearing.
//!
//! # Reading the output honestly
//!
//! This measures CPU in the paint closure. It is **not** frame time: the GPU
//! work behind those paths is a separate cost with its own ceiling (the batch
//! rule), and the vertex ceiling that turns the window black is a third thing
//! again. What is being fitted is a cost model *for CPU*, and a model spent on
//! a black-window guard has to stay an upper bound — so the constants pasted
//! into `budgets` should keep a margin over the fit.

use std::time::Instant;

use dodo_flow::{
    budgets,
    geometry::Vec2,
    render::{painter::build_path, plan::PathPaint, shapes::Outline},
};
use gpui::{
    AppContext, Background, Context, IntoElement, ParentElement, Path, Pixels, QuitMode, Render,
    Styled, Window, WindowOptions, canvas, div, hsla, px, size,
};

/// The sweep. Chosen so the two coefficients are separable: the path count
/// varies at a fixed vertex count and the vertex count varies at a fixed path
/// count, which is what makes a two-variable fit mean anything.
const SWEEP: &[(usize, usize)] = &[
    (200, 6),
    (400, 6),
    (800, 6),
    (1_600, 6),
    (200, 50),
    (400, 50),
    (800, 50),
    (200, 200),
    (400, 200),
    (800, 200),
];

/// Frames per configuration. The first few are discarded — the first frame of a
/// configuration pays for cold vertex buffers, and the number wanted is the
/// steady state.
const FRAMES_PER_CONFIG: u32 = 40;
const WARMUP_FRAMES: u32 = 8;

/// One measured configuration.
#[derive(Debug, Clone, Copy)]
struct Sample {
    paths: usize,
    vertices_per_path: usize,
    /// The minimum observed microseconds for the paint loop.
    micros: f64,
}

impl Sample {
    fn total_vertices(&self) -> f64 {
        (self.paths * self.vertices_per_path) as f64
    }
}

/// Builds one path of roughly `vertices` vertices.
///
/// A stroked polyline, because that is what an edge is and because the vertex
/// count of a stroke is close to linear in its point count — which makes the
/// sweep's independent variable actually independent.
fn path_of(vertices: usize, seed: usize) -> Option<Path<Pixels>> {
    // A stroke emits roughly two triangles a segment, so six vertices; the
    // point count needed is the target over that.
    let points = (vertices / 6).max(2);
    let mut outline = Outline::with_capacity(points + 1);
    let x = 40.0 + (seed % 37) as f32 * 3.0;
    let y = 40.0 + (seed % 23) as f32 * 5.0;
    outline.move_to(Vec2::new(x, y));
    for step in 1..=points {
        let t = step as f32;
        outline.line_to(Vec2::new(x + t * 1.7, y + (t * 0.37).sin() * 24.0));
    }

    build_path(
        &outline,
        PathPaint::Stroke {
            color: dodo_flow::models::Color::WHITE,
            width: 1.5,
        },
        0.25,
    )
}

/// A path guaranteed to be six vertices: the smallest a stroke can be, so the
/// per-path term is measured with the per-vertex term as close to zero as it
/// gets.
fn tiny_path(seed: usize) -> Option<Path<Pixels>> {
    let x = 40.0 + (seed % 37) as f32 * 3.0;
    let y = 40.0 + (seed % 23) as f32 * 5.0;
    let mut outline = Outline::with_capacity(2);
    outline.move_to(Vec2::new(x, y));
    outline.line_to(Vec2::new(x + 8.0, y));
    build_path(
        &outline,
        PathPaint::Stroke {
            color: dodo_flow::models::Color::WHITE,
            width: 1.5,
        },
        0.25,
    )
}

struct FitWindow {
    /// One built path set per configuration, tessellated once before any
    /// measurement so the sweep measures painting rather than lyon.
    built: Vec<Vec<Path<Pixels>>>,
    config: usize,
    frame: u32,
    best_micros: f64,
    samples: Vec<Sample>,
    color: Background,
    done: bool,
}

impl FitWindow {
    fn new() -> FitWindow {
        let built = SWEEP
            .iter()
            .map(|&(paths, vertices)| {
                (0..paths)
                    .filter_map(|seed| {
                        if vertices <= 6 {
                            tiny_path(seed)
                        } else {
                            path_of(vertices, seed)
                        }
                    })
                    .collect::<Vec<_>>()
            })
            .collect();

        FitWindow {
            built,
            config: 0,
            frame: 0,
            best_micros: f64::MAX,
            samples: Vec::new(),
            color: Background::from(hsla(0.6, 0.5, 0.6, 0.9)),
            done: false,
        }
    }

    /// Paints one configuration and records the time.
    fn measure(&mut self, window: &mut Window) {
        let Some(paths) = self.built.get(self.config) else {
            return;
        };

        let start = Instant::now();
        for path in paths {
            // The clone is the measurement, not an artefact of it: `paint_path`
            // consumes its argument, so a cached path is cloned into it on
            // every frame it is painted. Phase 0 §3 correction 13.
            window.paint_path(path.clone(), self.color);
        }
        let micros = start.elapsed().as_secs_f64() * 1e6;

        self.frame += 1;
        if self.frame == 1 {
            eprintln!(
                "  config {}/{}: {} paths",
                self.config + 1,
                SWEEP.len(),
                paths.len()
            );
        }
        if self.frame > WARMUP_FRAMES {
            self.best_micros = self.best_micros.min(micros);
        }

        if self.frame >= FRAMES_PER_CONFIG {
            let (paths, vertices_per_path) = SWEEP[self.config];
            // The real vertex count, not the requested one — `build_path` decides.
            let actual: usize = self.built[self.config]
                .iter()
                .map(|path| path.vertices.len())
                .sum();
            self.samples.push(Sample {
                paths: self.built[self.config].len(),
                vertices_per_path: actual / self.built[self.config].len().max(1),
                micros: self.best_micros,
            });
            let _ = vertices_per_path;
            let _ = paths;

            self.config += 1;
            self.frame = 0;
            self.best_micros = f64::MAX;

            if self.config >= SWEEP.len() {
                self.done = true;
                report(&self.samples);
            }
        }
    }
}

impl Render for FitWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.done {
            cx.quit();
        } else {
            // The idle loop §35 forbids, deliberately: a frame time is only
            // measurable if frames are being produced. This is the measurement
            // harness, not the behaviour.
            window.request_animation_frame();
        }

        let view = cx.entity();
        div().size_full().bg(hsla(0.0, 0.0, 0.08, 1.0)).child(
            canvas(
                |_bounds, _window, _cx| (),
                move |_bounds, _prepaint, window, cx| {
                    view.update(cx, |this, _cx| this.measure(window));
                },
            )
            .size_full(),
        )
    }
}

/// Least-squares fit of `micros = a·paths + b·vertices`, with no intercept —
/// painting nothing costs nothing, and an intercept would let the fit hide a
/// constant that does not exist.
fn fit(samples: &[Sample]) -> (f64, f64) {
    let (mut spp, mut spv, mut svv, mut spt, mut svt) = (0.0, 0.0, 0.0, 0.0, 0.0);
    for sample in samples {
        let (p, v, t) = (sample.paths as f64, sample.total_vertices(), sample.micros);
        spp += p * p;
        spv += p * v;
        svv += v * v;
        spt += p * t;
        svt += v * t;
    }

    let determinant = spp * svv - spv * spv;
    if determinant.abs() < f64::EPSILON {
        return (0.0, 0.0);
    }
    (
        (svv * spt - spv * svt) / determinant,
        (spp * svt - spv * spt) / determinant,
    )
}

fn report(samples: &[Sample]) {
    let budgets = budgets::for_backend(budgets::RenderBackend::Metal);

    println!("\ndodo-flow — Phase 5: re-fitting the paint cost model");
    println!("\n  measured, minimum over {FRAMES_PER_CONFIG} frames per configuration:");
    println!(
        "  {:>8} {:>10} {:>12} {:>12} {:>12}",
        "paths", "verts/path", "vertices", "measured µs", "current µs"
    );
    for sample in samples {
        // What the constants currently in `budgets` say this frame costs.
        let current = (sample.paths as f64 * budgets.nanos_per_path as f64
            + sample.total_vertices() * budgets.nanos_per_vertex as f64)
            / 1_000.0;
        println!(
            "  {:>8} {:>10} {:>12} {:>12.1} {:>12.1}",
            sample.paths,
            sample.vertices_per_path,
            sample.total_vertices() as u64,
            sample.micros,
            current
        );
    }

    let (nanos_per_path, nanos_per_vertex) = fit(samples);
    println!("\n  fitted:");
    println!(
        "    nanos_per_path    {:>8.0}   (currently {})",
        nanos_per_path * 1_000.0,
        budgets.nanos_per_path
    );
    println!(
        "    nanos_per_vertex  {:>8.0}   (currently {})",
        nanos_per_vertex * 1_000.0,
        budgets.nanos_per_vertex
    );

    // The check that matters: a cost model whose failure mode is a black window
    // has to be an upper bound. Report the worst under-prediction, if any.
    let mut worst_ratio = 0.0f64;
    for sample in samples {
        let predicted =
            nanos_per_path * sample.paths as f64 + nanos_per_vertex * sample.total_vertices();
        worst_ratio = worst_ratio.max(sample.micros / predicted.max(f64::EPSILON));
    }
    println!(
        "\n  the fit under-predicts by at most {:.2}x — a model spent on a black-window",
        worst_ratio
    );
    println!("  guard must be a ceiling, so the constants pasted in should carry a margin.");
}

fn main() {
    gpui_platform::application()
        .with_quit_mode(QuitMode::LastWindowClosed)
        .run(move |cx| {
            cx.activate(true);
            let options = WindowOptions {
                window_bounds: Some(gpui::WindowBounds::Windowed(gpui::Bounds {
                    origin: gpui::point(px(80.0), px(80.0)),
                    size: size(px(1440.0), px(900.0)),
                })),
                ..Default::default()
            };

            cx.open_window(options, |_window, cx| cx.new(|_cx| FitWindow::new()))
                .expect("failed to open window");
        });
}
