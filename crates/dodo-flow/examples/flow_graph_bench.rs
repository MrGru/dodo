//! The graph engine's numbers, on §19's scene: **100,000 nodes and 500,000
//! edges**.
//!
//! ```sh
//! cargo run --release -p dodo-flow --example flow_graph_bench --locked
//! ```
//!
//! Release, always — a debug build measures the borrow checker's bookkeeping
//! rather than the data structure, and the two disagree by more than an order
//! of magnitude here.
//!
//! # What it answers
//!
//! 1. **§20's open question.** That section says the adjacency representation
//!    "should be benchmarked" and sketches `Vec<SmallVec<[EdgeIndex; 4]>>`.
//!    This builds the same graph twice — once through
//!    [`AdjacencyIndex`](dodo_flow::runtime::AdjacencyIndex), which is that
//!    shape without the dependency, and once through the obvious
//!    `Vec<Vec<u32>>` — and prints build time, lookup time and the allocation
//!    count for both. The answer goes into `runtime::adjacency`'s module doc,
//!    and it is a *measurement* there rather than an assertion because of this
//!    file.
//! 2. **§19's target, in wall-clock time.** Moving one node with four edges is
//!    supposed to cost one node, four route rebuilds and one spatial update.
//!    `runtime::world`'s property test asserts the *counts*; this prints the
//!    microseconds, and prints the same move on a graph two orders of magnitude
//!    smaller so the two can be compared. They should be the same number: that
//!    is the whole claim.
//!
//! No `criterion`. dodo has no bench harness and is deliberate about every
//! package in its graph (`deny.toml`, `THIRD-PARTY-NOTICES.md`), so this is an
//! example printing `Instant` timings — the shape the plan chose in Phase 0 and
//! the same one Phase 4's scene harness will take.

use std::time::{Duration, Instant};

use dodo_flow::{
    geometry::Vec2,
    models::{ElementKind, GraphNodeKind, NodeIndex},
    runtime::{AdjacencyIndex, ConnectionRules, EdgeEnd, GraphWorld},
};

/// §19's scene, exactly.
const NODES: usize = 100_000;
const EDGES: usize = 500_000;

/// A diagram-shaped graph: 100,000 nodes and 120,000 edges, average degree
/// 2.4.
///
/// §19's scene is a stress test, not a document. Real node graphs are sparse —
/// a node has an input, an output and occasionally a second output — and the
/// two densities answer §20's question differently, which is exactly the sort
/// of thing a benchmark is for and an assertion is not.
const SPARSE_EDGES: usize = 120_000;

/// The edge list, generated once and shared by every representation so they are
/// measured on identical input.
///
/// A ring plus a stride, which gives every node the same degree and keeps the
/// generator O(edges). The graph's *shape* does not matter to any of these
/// measurements; its size and its density do.
fn edge_list(edges: usize) -> Vec<(u32, u32)> {
    (0..edges)
        .map(|index| ((index % NODES) as u32, ((index * 7 + 1) % NODES) as u32))
        .collect()
}

fn millis(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1_000.0
}

/// The engine's own index: four inline slots per list, spilling to the heap
/// only for a node of degree five or more.
fn build_compact(edges: &[(u32, u32)]) -> (AdjacencyIndex, Duration) {
    let start = Instant::now();
    let mut index = AdjacencyIndex::new();
    index.reserve(NODES);
    for _ in 0..NODES {
        index.push_node();
    }
    for (edge, (source, target)) in edges.iter().enumerate() {
        index.connect(
            dodo_flow::models::EdgeIndex::new(edge as u32),
            NodeIndex::new(*source),
            NodeIndex::new(*target),
        );
    }
    (index, start.elapsed())
}

/// The obvious alternative, built here rather than shipped: one `Vec` per node
/// per direction.
fn build_vec_of_vecs(edges: &[(u32, u32)]) -> (Vec<Vec<u32>>, Vec<Vec<u32>>, Duration) {
    let start = Instant::now();
    let mut outgoing: Vec<Vec<u32>> = vec![Vec::new(); NODES];
    let mut incoming: Vec<Vec<u32>> = vec![Vec::new(); NODES];
    for (edge, (source, target)) in edges.iter().enumerate() {
        outgoing[*source as usize].push(edge as u32);
        incoming[*target as usize].push(edge as u32);
    }
    (outgoing, incoming, start.elapsed())
}

/// §20's comparison at one density.
fn compare_adjacency(label: &str, edge_count: usize) {
    let edges = edge_list(edge_count);
    let (compact, compact_build) = build_compact(&edges);
    let (outgoing, incoming, vec_build) = build_vec_of_vecs(&edges);

    // The same walk on both — every incident edge index, summed so the
    // optimiser cannot delete it. Comparing a real walk against `Vec::len` is
    // the mistake that makes an inline list look slow.
    let start = Instant::now();
    let mut compact_total = 0u64;
    for node in 0..NODES {
        for edge in compact.incident_edges(NodeIndex::new(node as u32)) {
            compact_total += edge.raw() as u64;
        }
    }
    let compact_lookup = start.elapsed();

    let start = Instant::now();
    let mut vec_total = 0u64;
    for node in 0..NODES {
        for edge in outgoing[node].iter().chain(incoming[node].iter()) {
            vec_total += *edge as u64;
        }
    }
    let vec_lookup = start.elapsed();

    assert_eq!(
        compact_total, vec_total,
        "the two representations must hold the same graph"
    );

    // How many lists actually reach the heap. The inline form allocates only
    // for a list longer than `INLINE_CAPACITY`; the `Vec` form allocates for
    // every non-empty list.
    let spilled = (0..NODES)
        .map(|node| {
            usize::from(compact.outgoing(NodeIndex::new(node as u32)).len() > 4)
                + usize::from(compact.incoming(NodeIndex::new(node as u32)).len() > 4)
        })
        .sum::<usize>();
    let vec_allocations = (0..NODES)
        .map(|node| {
            usize::from(!outgoing[node].is_empty()) + usize::from(!incoming[node].is_empty())
        })
        .sum::<usize>();

    println!("  {label}");
    println!(
        "    build            CompactList {:>7.2} ms    Vec<Vec<u32>> {:>7.2} ms",
        millis(compact_build),
        millis(vec_build)
    );
    println!(
        "    {NODES} lookups   CompactList {:>7.2} ms    Vec<Vec<u32>> {:>7.2} ms",
        millis(compact_lookup),
        millis(vec_lookup)
    );
    println!(
        "    heap lists       CompactList {:>7}       Vec<Vec<u32>> {:>7}",
        spilled, vec_allocations
    );
    println!(
        "    index headers    CompactList {:>7.1} MB    Vec<Vec<u32>> {:>7.1} MB",
        (2 * NODES * std::mem::size_of::<dodo_flow::runtime::CompactList>()) as f64 / 1e6,
        (2 * NODES * std::mem::size_of::<Vec<u32>>()) as f64 / 1e6
    );
}

fn main() {
    println!("dodo-flow graph engine — {NODES} nodes (requirements §19's scene)\n");
    if cfg!(debug_assertions) {
        println!("WARNING: debug build. Re-run with --release; these numbers mean nothing.\n");
    }

    println!("§20  adjacency representation — which one, and at which density");
    compare_adjacency("dense  (§19: 500,000 edges, average degree 10)", EDGES);
    compare_adjacency(
        "sparse (a real diagram: 120,000 edges, degree 2.4)",
        SPARSE_EDGES,
    );
    println!();

    // ---- §19: the propagation, in wall-clock time ------------------------

    let edges = edge_list(EDGES);

    let start = Instant::now();
    let mut world = GraphWorld::new();
    world.set_rules(ConnectionRules::PERMISSIVE);
    world.reserve(NODES, EDGES);
    for index in 0..NODES {
        world.create_node(
            ElementKind::GraphNode(GraphNodeKind::Default),
            Vec2::new((index % 400) as f32 * 200.0, (index / 400) as f32 * 120.0),
            Vec2::new(160.0, 60.0),
        );
    }
    for (source, target) in &edges {
        world
            .connect(
                EdgeEnd::node(NodeIndex::new(*source)),
                EdgeEnd::node(NodeIndex::new(*target)),
            )
            .expect("permissive rules accept every generated edge");
    }
    let world_build = start.elapsed();

    let start = Instant::now();
    let built = world.rebuild_all_geometry();
    let all_routes = start.elapsed();

    println!("§17  world construction");
    println!(
        "  build {NODES} nodes + {EDGES} edges   {:>8.2} ms",
        millis(world_build)
    );
    println!(
        "  route every edge once ({built})       {:>8.2} ms   ({:.2} µs/edge)\n",
        millis(all_routes),
        all_routes.as_secs_f64() * 1e6 / built.max(1) as f64
    );

    // The subject: one node, four edges, in the middle of the enormous graph.
    let subject = world.create_node(
        ElementKind::GraphNode(GraphNodeKind::Default),
        Vec2::new(-5_000.0, -5_000.0),
        Vec2::new(160.0, 60.0),
    );
    for neighbour in 0..4u32 {
        world
            .connect(
                EdgeEnd::node(subject),
                EdgeEnd::node(NodeIndex::new(neighbour * 1_000)),
            )
            .expect("valid");
    }
    world.rebuild_all_geometry();

    // The measurement: mark, then rebuild, a thousand times over — one drag's
    // worth of mouse moves, several times.
    let mut rebuilt = 0;
    let start = Instant::now();
    for step in 0..1_000 {
        world.move_node(
            subject,
            Vec2::new(if step % 2 == 0 { 1.0 } else { -1.0 }, 0.0),
        );
        rebuilt += world.rebuild_dirty_geometry();
    }
    let move_time = start.elapsed();

    println!("§19  move one node with four edges, in the {NODES}-node graph");
    println!(
        "  1,000 moves + rebuilds              {:>8.2} ms   ({:.2} µs per move)",
        millis(move_time),
        move_time.as_secs_f64() * 1e6 / 1_000.0
    );
    println!("  routes rebuilt                       {rebuilt}  (four per move, and no more)");

    // The same move in a graph two orders of magnitude smaller. The whole claim
    // is that these two numbers are the same.
    let mut small = GraphWorld::new();
    small.set_rules(ConnectionRules::PERMISSIVE);
    for index in 0..1_000 {
        small.create_node(
            ElementKind::GraphNode(GraphNodeKind::Default),
            Vec2::new(index as f32 * 200.0, 0.0),
            Vec2::new(160.0, 60.0),
        );
    }
    for index in 0..5_000u32 {
        small
            .connect(
                EdgeEnd::node(NodeIndex::new(index % 1_000)),
                EdgeEnd::node(NodeIndex::new((index * 7 + 1) % 1_000)),
            )
            .expect("valid");
    }
    let small_subject = small.create_node(
        ElementKind::GraphNode(GraphNodeKind::Default),
        Vec2::new(-5_000.0, -5_000.0),
        Vec2::new(160.0, 60.0),
    );
    for neighbour in 0..4u32 {
        small
            .connect(
                EdgeEnd::node(small_subject),
                EdgeEnd::node(NodeIndex::new(neighbour * 100)),
            )
            .expect("valid");
    }
    small.rebuild_all_geometry();

    let start = Instant::now();
    for step in 0..1_000 {
        small.move_node(
            small_subject,
            Vec2::new(if step % 2 == 0 { 1.0 } else { -1.0 }, 0.0),
        );
        small.rebuild_dirty_geometry();
    }
    let small_move = start.elapsed();

    println!(
        "  the same, in a 1,000-node graph     {:>8.2} ms   ({:.2} µs per move)",
        millis(small_move),
        small_move.as_secs_f64() * 1e6 / 1_000.0
    );
    println!(
        "\n  ratio {:.2}x for 100x the graph — the number the architecture exists to keep at 1.",
        move_time.as_secs_f64() / small_move.as_secs_f64()
    );
}
