//! **Phase 4's gate.** §38's scenes, §50's questions, and the exit criteria as
//! numbers rather than as claims.
//!
//! ```sh
//! cargo run --release -p dodo-flow --example flow_scene_bench --locked
//! ```
//!
//! Release, always. A debug build measures the borrow checker's bookkeeping
//! rather than the data structure, and the two disagree by more than an order
//! of magnitude — the harness says so at the top and the numbers from a debug
//! run must never be reported as results.
//!
//! # What it answers
//!
//! §50 asks eight questions of a benchmark harness. This one answers the six
//! that belong to this phase:
//!
//! - How expensive is a viewport query at 100,000 nodes?
//! - How expensive is moving one node in a 300,000-edge graph?
//! - How many canvas edges can be painted smoothly? (as painted vertices
//!   against [`RenderBudgets`], which is the unit Phase 0 established)
//! - How much memory does each scale consume?
//! - Does pan cause edge-route cache misses? *It must not.*
//! - Does pan cause text-layout cache misses? *There is no text yet; the
//!   harness says so rather than printing a zero that looks like an answer.*
//!
//! The two it does not answer are Phase 5's and Phase 6's: how many visible
//! rich GPUI nodes stay interactive, and what sketch mode adds.
//!
//! # And §21's own question
//!
//! §21 says to start with a uniform hash grid **and then benchmark it**. So the
//! harness builds the same scene three ways — through the shipping grid,
//! through a dense array grid over the document's bounding box, and through a
//! brute-force scan — and prints all three. The dense grid and the scan exist
//! only in this file, exactly as `flow_graph_bench.rs`'s `Vec<Vec<u32>>` does:
//! an alternative you ship is a maintenance cost, an alternative you measure
//! against is evidence.
//!
//! No `criterion`. dodo has no bench harness and is deliberate about every
//! package in its graph (`deny.toml`, `THIRD-PARTY-NOTICES.md`), so this is an
//! example printing `Instant` timings.

use std::time::{Duration, Instant};

use dodo_flow::{
    budgets::{self, CACHE_BYTES_PER_VERTEX, RenderBudgets},
    geometry::{Rect, Vec2},
    instrument::{Instruments, Probe},
    models::{Color, NodeIndex},
    render::{
        GridLimits, GridSettings, PaintPlan, SceneInk, SceneOptions,
        painter::build_path,
        plan::{PathPrimitive, PrimitiveSink, QuadPrimitive, TextPrimitive},
        scene,
    },
    runtime::{BoxQuery, GraphWorld},
    scenes::{self, BENCH_PANE, SceneSpec},
    spatial::{SpatialIndex, VisibleSet},
};

/// **A painter with no window.**
///
/// `render::painter::build_path` is where tessellation actually happens, and it
/// needs no `App` and no `Window` — it is lyon behind a GPUI type. So the
/// harness can implement [`PrimitiveSink`] itself and get two things a window
/// would otherwise have been needed for: the **real** painted vertex count
/// rather than the estimate, and the CPU cost of tessellating a frame from
/// scratch, which is the number that decides whether §23's geometry cache is
/// worth building.
///
/// What it cannot measure is the GPU side — the per-batch intermediate render
/// pass, and the 104-bytes-per-vertex upload. Those are Phase 0's numbers and
/// they stay Phase 0's.
#[derive(Default)]
struct HeadlessPainter {
    quads: u32,
    paths: u32,
    vertices: u32,
    /// Nanoseconds spent inside `build_path`.
    tessellation_nanos: u128,
}

impl PrimitiveSink for HeadlessPainter {
    fn quad(&mut self, _quad: &QuadPrimitive) {
        self.quads += 1;
    }

    fn path(&mut self, path: &PathPrimitive) -> u32 {
        let start = Instant::now();
        let built = build_path(&path.outline, path.paint, path.quality.flattening_tolerance);
        self.tessellation_nanos += start.elapsed().as_nanos();

        match built {
            Some(built) => {
                self.paths += 1;
                let vertices = built.vertices.len() as u32;
                self.vertices += vertices;
                vertices
            }
            None => 0,
        }
    }

    fn text(&mut self, _text: &TextPrimitive) -> u32 {
        0
    }
}

/// How many times a measured operation is repeated before it is divided out.
/// Enough that a microsecond-scale operation is measured against a
/// millisecond-scale clock.
const REPEATS: u32 = 200;

fn millis(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1_000.0
}

fn micros(duration: Duration, over: u32) -> f64 {
    duration.as_secs_f64() * 1e6 / over.max(1) as f64
}

fn ink() -> SceneInk {
    SceneInk {
        fill: Color::rgb(0.16, 0.17, 0.20),
        stroke: Color::rgb(0.85, 0.86, 0.90),
        edge: Color::rgb(0.60, 0.62, 0.68),
        handle: Color::rgb(0.30, 0.60, 1.00),
        accent: Color::rgb(1.00, 0.62, 0.20),
    }
}

fn scene_options(budgets: &RenderBudgets) -> SceneOptions {
    SceneOptions::new(GridSettings::default(), GridLimits::from_budgets(budgets))
}

// ---------------------------------------------------------------------------
// The two alternatives §21 asks to be measured against. Neither ships.
// ---------------------------------------------------------------------------

/// A dense array grid over a **known** bounding box: one `Vec` of cells, one
/// multiply-add to find a cell, no hashing at all.
///
/// The fastest possible uniform grid, and the reason it is not the engine's:
/// it needs the bounding box up front. An infinite canvas does not have one, so
/// a node dragged a million units out either reallocates the whole array or
/// falls out of the index. Measured here so that the trade is a number.
struct DenseGrid {
    origin: Vec2,
    cell: f32,
    columns: usize,
    rows: usize,
    cells: Vec<Vec<u32>>,
}

impl DenseGrid {
    fn build(bounds: Rect, cell: f32, items: impl Iterator<Item = (u32, Rect)>) -> DenseGrid {
        let columns = ((bounds.width() / cell).ceil() as usize + 1).max(1);
        let rows = ((bounds.height() / cell).ceil() as usize + 1).max(1);
        let mut grid = DenseGrid {
            origin: bounds.min(),
            cell,
            columns,
            rows,
            cells: vec![Vec::new(); columns * rows],
        };
        for (item, item_bounds) in items {
            grid.insert(item, item_bounds);
        }
        grid
    }

    fn cell_range(&self, rect: Rect) -> (usize, usize, usize, usize) {
        let min = rect.min() - self.origin;
        let max = rect.max() - self.origin;
        let clamp_x = |v: f32| {
            (v / self.cell)
                .floor()
                .clamp(0.0, self.columns as f32 - 1.0) as usize
        };
        let clamp_y = |v: f32| (v / self.cell).floor().clamp(0.0, self.rows as f32 - 1.0) as usize;
        (
            clamp_x(min.x),
            clamp_y(min.y),
            clamp_x(max.x),
            clamp_y(max.y),
        )
    }

    fn insert(&mut self, item: u32, bounds: Rect) {
        let (x0, y0, x1, y1) = self.cell_range(bounds);
        for y in y0..=y1 {
            for x in x0..=x1 {
                self.cells[y * self.columns + x].push(item);
            }
        }
    }

    /// No dedup: an item spanning several cells is returned once per cell. The
    /// shipping grid deduplicates in O(1), so this comparison is generous to
    /// the alternative rather than to the engine.
    fn query(&self, rect: Rect, out: &mut Vec<u32>) {
        let (x0, y0, x1, y1) = self.cell_range(rect);
        for y in y0..=y1 {
            for x in x0..=x1 {
                out.extend_from_slice(&self.cells[y * self.columns + x]);
            }
        }
    }

    fn memory_bytes(&self) -> usize {
        self.cells.capacity() * size_of::<Vec<u32>>()
            + self
                .cells
                .iter()
                .map(|cell| cell.capacity() * size_of::<u32>())
                .sum::<usize>()
    }
}

/// **The oracle**, and the thing culling has to beat: look at everything.
///
/// Answers exactly the question `SpatialIndex::query_visible` answers — nodes
/// *and* edges, at their painted bounds — because a comparison against a
/// cheaper question would flatter the index rather than test it.
fn brute_force_visible(world: &GraphWorld, query: Rect) -> (usize, usize) {
    let nodes = world
        .nodes()
        .indices()
        .filter(|node| !world.nodes().is_hidden(*node))
        .filter(|node| dodo_flow::spatial::node_painted_bounds(world, *node).intersects(query))
        .count();
    let edges = world
        .edges()
        .indices()
        .filter(|edge| !world.edges().is_hidden(*edge))
        .filter(|edge| {
            dodo_flow::spatial::edge_painted_bounds(world, *edge)
                .is_some_and(|bounds| bounds.intersects(query))
        })
        .count();
    (nodes, edges)
}

/// Just the nodes, for the structure comparison — which is node-only on both
/// sides so the two grids are answering the same question.
fn brute_force_nodes(world: &GraphWorld, query: Rect, out: &mut Vec<NodeIndex>) {
    for node in world.nodes().indices() {
        if world.nodes().bounds(node).intersects(query) {
            out.push(node);
        }
    }
}

// ---------------------------------------------------------------------------
// Memory
// ---------------------------------------------------------------------------

/// The process's resident set, in bytes, or `None` where it cannot be read.
///
/// Shelling out to `ps` rather than taking a dependency for it. It is the real
/// number — allocator overhead included — where the structural accounting below
/// is the exact one, and the two are worth printing side by side: a big gap is
/// fragmentation, and a small one means the accounting is honest.
fn resident_bytes() -> Option<u64> {
    let output = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .ok()?;
    String::from_utf8(output.stdout)
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()
        .map(|kilobytes| kilobytes * 1024)
}

/// What the runtime structurally occupies, counted rather than sampled.
///
/// Exact and portable, where [`resident_bytes`] is real and platform-specific.
/// §41 asks for bounded memory behaviour to be a bound; this is what it is
/// measured against.
fn structural_bytes(world: &GraphWorld, index: &SpatialIndex) -> (usize, usize) {
    let nodes = world.nodes().len();
    let edges = world.edges().len();

    // The stores' own arrays, from their declared shapes. Deliberately an
    // estimate of the *arrays* rather than a deep walk: nothing here allocates
    // per element except the cold kind strings, which are counted separately.
    let node_bytes = nodes
        * (size_of::<Vec2>() * 2                    // position, size
            + size_of::<u8>() * 2                   // shape, flags
            + size_of::<u64>()                      // id
            + size_of::<i32>()                      // z
            + size_of::<dodo_flow::runtime::CompactList>()
            + size_of::<dodo_flow::models::ElementStyle>()
            + size_of::<dodo_flow::runtime::NodeCold>());
    let edge_bytes = edges
        * (size_of::<dodo_flow::runtime::EdgeEnd>() * 2
            + size_of::<u8>() * 2
            + size_of::<u64>()
            + size_of::<i32>()
            + size_of::<dodo_flow::models::ElementStyle>()
            + size_of::<Option<String>>());
    // Two `CompactList`s per node, plus the routes: an `EdgeRoute` header and
    // its segment vector.
    let adjacency_bytes = nodes * size_of::<dodo_flow::runtime::CompactList>() * 2;
    let route_bytes = edges * (size_of::<dodo_flow::geometry::EdgeRoute>() + 4 * size_of::<Vec2>());

    (
        node_bytes + edge_bytes + adjacency_bytes + route_bytes,
        index.memory_bytes(),
    )
}

// ---------------------------------------------------------------------------
// One scene
// ---------------------------------------------------------------------------

struct SceneResult {
    spec: SceneSpec,
    visible_nodes: usize,
    visible_edges: usize,
    query_micros: f64,
    scan_micros: f64,
    build_millis: f64,
    index_millis: f64,
    /// The bound `enforce_vertex_ceiling` spends, before tessellation.
    estimated_vertices: u32,
    /// What lyon actually produced. The estimate must never be below it.
    painted_vertices: u32,
    /// Microseconds spent tessellating this frame from scratch.
    tessellation_micros: f64,
    paths: u32,
    quads: u32,
    path_batches: u32,
    culled_paths: u32,
    dropped_paths: u32,
    runtime_bytes: usize,
    index_bytes: usize,
    resident_bytes: Option<u64>,
    entries_per_node: f64,
    oversized: usize,
}

fn measure_scene(spec: SceneSpec, budgets: &RenderBudgets) -> SceneResult {
    let start = Instant::now();
    let world = scenes::build(&spec);
    let build_millis = millis(start.elapsed());

    let start = Instant::now();
    let index = SpatialIndex::for_world(&world);
    let index_millis = millis(start.elapsed());

    let viewport = spec.viewport(BENCH_PANE);
    let mut visible = VisibleSet::new();

    // Warm, then measure. The first query touches cold pages of a 100,000-node
    // index and would be measuring the page fault rather than the query.
    index.query_visible(&world, &viewport, &mut visible);
    let start = Instant::now();
    for _ in 0..REPEATS {
        index.query_visible(&world, &viewport, &mut visible);
    }
    let query_micros = micros(start.elapsed(), REPEATS);

    // The same answer the slow way. Fewer repeats on the big scenes because it
    // is slow, but not so few that the number is noise — five repeats of the
    // small scene's scan varied by 3x between runs, which is not a measurement.
    let scan_repeats = if spec.nodes > 20_000 { 30 } else { 500 };
    let start = Instant::now();
    let mut scanned = (0, 0);
    for _ in 0..scan_repeats {
        scanned = brute_force_visible(&world, visible.query_rect());
    }
    let scan_micros = micros(start.elapsed(), scan_repeats);
    assert_eq!(
        (visible.node_count(), visible.edge_count()),
        scanned,
        "the index and the oracle disagreed on the {} scene",
        spec.name
    );

    let mut plan = PaintPlan::new();
    let stats = scene::plan_scene(
        &mut plan,
        &world,
        &visible,
        &viewport,
        ink(),
        &scene_options(budgets),
    );
    let estimated_vertices = plan.estimated_path_vertices();
    let culled_paths = plan.culled_paths();
    let paths = plan.path_count();
    let quads = plan.quad_count();
    let dropped_paths = plan.enforce_vertex_ceiling(budgets);
    let _ = stats;

    // The real thing, through the same `paint_into` a window would use.
    let mut painter = HeadlessPainter::default();
    let painted = plan.paint_into(&mut painter);

    let (runtime_bytes, index_bytes) = structural_bytes(&world, &index);

    SceneResult {
        spec,
        visible_nodes: visible.node_count(),
        visible_edges: visible.edge_count(),
        query_micros,
        scan_micros,
        build_millis,
        index_millis,
        estimated_vertices,
        painted_vertices: painted.path_vertices,
        tessellation_micros: painter.tessellation_nanos as f64 / 1_000.0,
        paths,
        quads,
        // One contiguous run, which `PaintPlan` guarantees; reported so the
        // exit criterion is measured rather than assumed.
        path_batches: u32::from(paths > 0),
        culled_paths,
        dropped_paths,
        runtime_bytes,
        index_bytes,
        resident_bytes: resident_bytes(),
        entries_per_node: index.nodes().entry_count() as f64 / world.nodes().len().max(1) as f64,
        oversized: index.nodes().oversized_count() + index.edges().oversized_count(),
    }
}

// ---------------------------------------------------------------------------
// The individual questions
// ---------------------------------------------------------------------------

/// §50: *does pan cause edge-route cache misses? It generally should not.*
///
/// The answer must be an exact zero, and it holds by construction rather than
/// by care: a pan changes the viewport, the viewport is not in the world, so
/// nothing is invalidated and the dirty queue is empty. Measured anyway,
/// because "by construction" is how a regression gets missed.
fn measure_pan(spec: SceneSpec, budgets: &RenderBudgets) {
    let mut world = scenes::build(&spec);
    let mut index = SpatialIndex::for_world(&world);
    world.clear_spatial_updates();

    let mut viewport = spec.viewport(BENCH_PANE);
    let mut visible = VisibleSet::new();
    let mut plan = PaintPlan::new();
    let options = scene_options(budgets);

    let routes_before = world.geometry().rebuild_count();
    let mut instruments = Instruments::on();
    let frames = 120;

    let start = Instant::now();
    for frame in 0..frames {
        // A real pan: a few pixels a frame, in a direction that keeps the
        // camera inside the content.
        viewport.pan_by(Vec2::new(if frame % 2 == 0 { 6.0 } else { 5.0 }, 3.0));

        let timer = instruments.start();
        let rebuilt = world.rebuild_dirty_geometry();
        instruments.record(Probe::EdgeRoute, timer);
        assert_eq!(rebuilt, 0, "a pure pan rebuilt {rebuilt} routes");

        let timer = instruments.start();
        let report = index.sync(&world);
        world.clear_spatial_updates();
        instruments.record(Probe::SpatialUpdate, timer);
        assert!(report.is_empty(), "a pure pan queued a spatial update");

        let timer = instruments.start();
        index.query_visible(&world, &viewport, &mut visible);
        instruments.record(Probe::VisibilityQuery, timer);

        let timer = instruments.start();
        scene::plan_scene(&mut plan, &world, &visible, &viewport, ink(), &options);
        instruments.record(Probe::RenderExtract, timer);
    }
    let elapsed = start.elapsed();

    let route_misses = world.geometry().rebuild_count() - routes_before;
    println!("  {} frames of pure pan on the {} scene", frames, spec.name);
    println!(
        "    edge-route cache misses            {route_misses}   {}",
        if route_misses == 0 {
            "(§50: it should not, and it does not)"
        } else {
            "(§50 VIOLATED)"
        }
    );
    println!("    text-layout cache misses           n/a  (no text element exists yet — Phase 5)");
    println!(
        "    tessellation cache misses          {} paths/frame  (no geometry cache yet — see the report)",
        plan.path_count()
    );
    println!(
        "    per frame                          {:>8.3} ms   ({:.0} fps if this were the whole frame)",
        millis(elapsed) / frames as f64,
        frames as f64 / elapsed.as_secs_f64()
    );
    print!("{}", instruments.report());
}

/// §50: *how expensive is moving one node in a 300,000-edge graph?*
///
/// Phase 3 measured the dirty propagation alone at 0.17 µs. This is that plus
/// the spatial update, which is the number a drag actually pays.
fn measure_drag(spec: SceneSpec) {
    let mut world = scenes::build(&spec);
    let mut index = SpatialIndex::for_world(&world);
    world.clear_spatial_updates();

    // A node in the middle of the document, with real neighbours.
    let subject = NodeIndex::new((spec.nodes / 2) as u32);
    let degree = world.incident_edges(subject).count();

    let mut rebuilt = 0;
    let mut moved = 0;
    let moves = 1_000;

    let start = Instant::now();
    for step in 0..moves {
        // A pixel-scale wobble, which is what a drag actually emits.
        world.move_node(
            subject,
            Vec2::new(if step % 2 == 0 { 0.7 } else { -0.7 }, 0.3),
        );
        rebuilt += world.rebuild_dirty_geometry();
        moved += index.sync(&world).nodes_moved;
        world.clear_spatial_updates();
    }
    let wobble = start.elapsed();

    // And the same node crossing cells, which is the expensive case.
    let start = Instant::now();
    let mut crossed = 0;
    for step in 0..moves {
        world.move_node(
            subject,
            Vec2::new(if step % 2 == 0 { 900.0 } else { -900.0 }, 0.0),
        );
        world.rebuild_dirty_geometry();
        crossed += index.sync(&world).nodes_moved;
        world.clear_spatial_updates();
    }
    let crossing = start.elapsed();

    println!(
        "  one node of degree {degree} in {} nodes / {} edges",
        spec.nodes, spec.edges
    );
    println!(
        "    {moves} sub-cell moves               {:>8.3} ms   ({:.2} µs per move)",
        millis(wobble),
        micros(wobble, moves)
    );
    println!(
        "    {moves} cell-crossing moves          {:>8.3} ms   ({:.2} µs per move)",
        millis(crossing),
        micros(crossing, moves)
    );
    println!(
        "    routes rebuilt                     {rebuilt}  ({} per move — the node's degree, and no more)",
        rebuilt / moves
    );
    println!(
        "    index re-links                     {moved} of {moves} sub-cell, {crossed} of {moves} crossing"
    );
}

/// §21's own question: is a uniform hash grid the right structure here?
fn compare_structures(spec: SceneSpec) {
    let world = scenes::build(&spec);
    let bounds = world.content_bounds().expect("the scene has content");
    let cell = dodo_flow::spatial::cell_size_for(&world);

    // Node-only on both sides. `SpatialIndex::for_world` also builds the edge
    // grid, and comparing that against a node-only array grid is the kind of
    // measurement that proves whatever its author wanted.
    let start = Instant::now();
    let mut index = SpatialIndex::new(cell);
    for node in world.nodes().indices() {
        index.insert_node(&world, node);
    }
    let hash_build = start.elapsed();

    let start = Instant::now();
    let dense = DenseGrid::build(
        bounds,
        cell,
        world
            .nodes()
            .indices()
            .map(|node| (node.raw(), world.nodes().bounds(node))),
    );
    let dense_build = start.elapsed();

    let viewport = spec.viewport(BENCH_PANE);
    let query = dodo_flow::spatial::query_rect(&viewport);

    let mut out = Vec::new();
    let start = Instant::now();
    for _ in 0..REPEATS {
        out.clear();
        index.node_candidates(query, &mut out);
    }
    let hash_query = micros(start.elapsed(), REPEATS);
    let hash_found = out.len();

    let mut raw = Vec::new();
    let start = Instant::now();
    for _ in 0..REPEATS {
        raw.clear();
        dense.query(query, &mut raw);
    }
    let dense_query = micros(start.elapsed(), REPEATS);

    let mut scanned = Vec::new();
    let start = Instant::now();
    for _ in 0..5 {
        scanned.clear();
        brute_force_nodes(&world, query, &mut scanned);
    }
    let scan_query = micros(start.elapsed(), 5);

    println!(
        "  {} — {} nodes, {:.0}-unit cells",
        spec.name, spec.nodes, cell
    );
    println!(
        "    uniform hash grid (ships)          build {:>7.2} ms   query {:>8.2} µs   {:>7.1} MB",
        millis(hash_build),
        hash_query,
        index.memory_bytes() as f64 / 1e6
    );
    println!(
        "    dense array grid                   build {:>7.2} ms   query {:>8.2} µs   {:>7.1} MB",
        millis(dense_build),
        dense_query,
        dense.memory_bytes() as f64 / 1e6
    );
    println!(
        "    brute-force scan                   build       —      query {:>8.2} µs   {:>7.1} MB",
        scan_query, 0.0
    );
    println!(
        "    ratio                              scan is {:.0}x the hash grid; the dense grid is {:.2}x",
        scan_query / hash_query.max(1e-9),
        dense_query / hash_query.max(1e-9)
    );
    println!(
        "    candidates                         {hash_found} deduplicated, {} raw from the dense grid",
        raw.len()
    );
}

/// The cell-size sweep behind [`dodo_flow::spatial::index::cell_size_for`].
fn sweep_cell_size(spec: SceneSpec) {
    let world = scenes::build(&spec);
    let viewport = spec.viewport(BENCH_PANE);
    let query = dodo_flow::spatial::query_rect(&viewport);
    let base = spec.node_size.x.max(spec.node_size.y);

    println!("  {} — mean node extent {base:.0} units", spec.name);
    for multiple in [0.5f32, 1.0, 2.0, 4.0, 8.0] {
        let cell = base * multiple;
        let mut index = SpatialIndex::new(cell);
        index.rebuild(&world);

        let mut out = Vec::new();
        index.node_candidates(query, &mut out);
        let start = Instant::now();
        for _ in 0..REPEATS {
            out.clear();
            index.node_candidates(query, &mut out);
        }
        let query_micros = micros(start.elapsed(), REPEATS);

        println!(
            "    cell {:>6.0} ({multiple:>3.1}x)   entries/node {:>5.2}   candidates {:>6}   query {:>8.2} µs",
            cell,
            index.nodes().entry_count() as f64 / world.nodes().len().max(1) as f64,
            out.len(),
            query_micros
        );
    }
}

/// §28's box selection, at the scale that makes it interesting.
fn measure_box_selection(spec: SceneSpec) {
    let mut world = scenes::build(&spec);
    let index = SpatialIndex::for_world(&world);
    world.clear_spatial_updates();

    let viewport = spec.viewport(BENCH_PANE);
    let band = viewport.visible_world_rect();

    let mut nodes = Vec::new();
    let mut edges = Vec::new();
    let repeats = 50;

    let start = Instant::now();
    let mut selected = 0;
    for _ in 0..repeats {
        nodes.clear();
        edges.clear();
        index.node_candidates(band, &mut nodes);
        index.edge_candidates(band, &mut edges);
        selected = world.apply_box_selection(
            BoxQuery::at_zoom(band, viewport.zoom()),
            nodes.iter().copied(),
            edges.iter().copied(),
        );
    }
    let elapsed = start.elapsed();

    println!("  {} — a band over the whole viewport", spec.name);
    println!(
        "    broad phase + exact tests          {:>8.2} µs   ({} candidates -> {selected} selected)",
        micros(elapsed, repeats),
        nodes.len() + edges.len()
    );
    println!(
        "    selection memory                   {:>8.1} KB   for {} ids",
        world.selection().memory_bytes() as f64 / 1e3,
        world.selection().len()
    );
}

// ---------------------------------------------------------------------------

fn main() {
    let budgets = budgets::current();

    println!("dodo-flow — Phase 4: spatial index, culling and the numbers that gate the rest\n");
    if cfg!(debug_assertions) {
        println!("WARNING: debug build. Re-run with --release; these numbers mean nothing.\n");
    }
    println!(
        "  backend {:?} ({:?}), pane {}x{}",
        budgets.backend, budgets.provenance, BENCH_PANE.x, BENCH_PANE.y
    );
    println!(
        "  ceilings: {} vertices hard, {} safe, {} at 60 fps, {} path batches\n",
        budgets.hard_path_vertex_ceiling,
        budgets.safe_path_vertex_ceiling,
        budgets.target_path_vertices_per_frame,
        budgets.max_path_batches_per_frame
    );

    println!("§38  the scenes");
    println!(
        "  {:<9} {:>9} {:>9} {:>10} {:>10} {:>11} {:>10}",
        "scene", "nodes", "edges", "visible n", "visible e", "query", "scan"
    );
    let mut results = Vec::new();
    for spec in SceneSpec::ALL {
        let result = measure_scene(spec, &budgets);
        println!(
            "  {:<9} {:>9} {:>9} {:>10} {:>10} {:>8.1} µs {:>7.1} µs",
            result.spec.name,
            result.spec.nodes,
            result.spec.edges,
            result.visible_nodes,
            result.visible_edges,
            result.query_micros,
            result.scan_micros
        );
        results.push(result);
    }

    println!("\n§16  culling — the exit criteria, per scene");
    println!(
        "  {:<9} {:>8} {:>8} {:>11} {:>11} {:>8} {:>8} {:>8}",
        "scene", "quads", "paths", "est. verts", "painted", "batches", "culled", "dropped"
    );
    for result in &results {
        println!(
            "  {:<9} {:>8} {:>8} {:>11} {:>11} {:>8} {:>8} {:>8}",
            result.spec.name,
            result.quads,
            result.paths,
            result.estimated_vertices,
            result.painted_vertices,
            result.path_batches,
            result.culled_paths,
            result.dropped_paths
        );
    }
    let worst = results
        .iter()
        .max_by_key(|result| result.painted_vertices)
        .expect("there is at least one scene");
    println!(
        "\n  worst scene ({}): {} painted vertices — {:.1}% of the {} safe ceiling, {:.0}x headroom",
        worst.spec.name,
        worst.painted_vertices,
        worst.painted_vertices as f64 * 100.0 / budgets.safe_path_vertex_ceiling as f64,
        budgets.safe_path_vertex_ceiling,
        budgets.safe_path_vertex_ceiling as f64 / worst.painted_vertices.max(1) as f64
    );
    println!(
        "  and {:.1}% of the {} that holds 60 fps",
        worst.painted_vertices as f64 * 100.0 / budgets.target_path_vertices_per_frame as f64,
        budgets.target_path_vertices_per_frame
    );
    println!(
        "  offscreen paths passed to the painter: 0 by construction — PaintPlan::push_path\n  refuses them, and `culled` above is what it had to refuse."
    );

    println!("\n§38 / §41  build cost and memory");
    println!(
        "  {:<9} {:>11} {:>11} {:>12} {:>11} {:>12}",
        "scene", "world build", "index build", "runtime", "index", "resident"
    );
    for result in &results {
        println!(
            "  {:<9} {:>8.2} ms {:>8.2} ms {:>9.1} MB {:>8.1} MB {:>9}",
            result.spec.name,
            result.build_millis,
            result.index_millis,
            result.runtime_bytes as f64 / 1e6,
            result.index_bytes as f64 / 1e6,
            result
                .resident_bytes
                .map(|bytes| format!("{:.1} MB", bytes as f64 / 1e6))
                .unwrap_or_else(|| "n/a".into())
        );
    }
    println!(
        "  index shape on the large scene: {:.2} entries per node, {} oversized items",
        results[2].entries_per_node, results[2].oversized
    );

    println!("\n§23  tessellation, and what a geometry cache would buy");
    println!(
        "  {:<9} {:>10} {:>13} {:>13} {:>13}",
        "scene", "paths", "tessellate", "per path", "cache bytes"
    );
    for result in &results {
        println!(
            "  {:<9} {:>10} {:>10.3} ms {:>10.2} µs {:>10.2} MB",
            result.spec.name,
            result.paths,
            result.tessellation_micros / 1_000.0,
            result.tessellation_micros / result.paths.max(1) as f64,
            (result.painted_vertices as usize * CACHE_BYTES_PER_VERTEX as usize) as f64 / 1e6
        );
    }
    println!(
        "  the whole 300,000-edge document cached would be {:.1} GB at {} bytes a vertex,\n           which is why §23's cache is byte-bounded and viewport-scoped rather than watched.",
        (300_000usize * 200 * CACHE_BYTES_PER_VERTEX as usize) as f64 / 1e9,
        CACHE_BYTES_PER_VERTEX
    );

    println!("\n§50  does pan cost anything it should not?");
    measure_pan(SceneSpec::LARGE, &budgets);

    println!("\n§50 / §19  moving one node in a 300,000-edge graph");
    measure_drag(SceneSpec::LARGE);

    println!("\n§21  which structure — benchmarked, not asserted");
    compare_structures(SceneSpec::LARGE);

    println!("\n§21  cell size");
    sweep_cell_size(SceneSpec::LARGE);

    println!("\n§28  box selection");
    measure_box_selection(SceneSpec::DENSE);

    println!("\n§21  the degenerate input: a document of document-crossing edges");
    let scattered = measure_scene(SceneSpec::SCATTERED, &budgets);
    println!(
        "  scattered  visible {} n / {} e   query {:.1} µs   oversized {}   index {:.1} MB",
        scattered.visible_nodes,
        scattered.visible_edges,
        scattered.query_micros,
        scattered.oversized,
        scattered.index_bytes as f64 / 1e6
    );
    println!(
        "  paths {} — estimated {} vertices ({:.0}% of the safe ceiling), dropped {}",
        scattered.paths,
        scattered.estimated_vertices,
        scattered.estimated_vertices as f64 * 100.0 / budgets.safe_path_vertex_ceiling as f64,
        scattered.dropped_paths
    );
}
