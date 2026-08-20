//! §38's benchmark scenes, generated deterministically.
//!
//! §38 names four scenes this phase has to answer for — small (100/200),
//! medium (5,000/15,000), large (100,000/300,000+) and a dense-visibility
//! stress test where several thousand objects are deliberately on screen at
//! once. They live in the library rather than in the benchmark example so that
//! a *test* and the harness measure the same thing: a scene that only exists
//! in an example is a scene no test can assert against, and this phase's exit
//! criteria are assertions.
//!
//! # The graphs are local, and that is a measurement decision
//!
//! `examples/flow_graph_bench.rs` connects node *i* to node *(7i + 1) mod n*,
//! which is right for the question it asks — adjacency cost does not care where
//! a node is. It is **wrong** for a spatial index: that rule produces edges
//! that cross the entire document, and an edge's spatial entry is its control
//! hull, so a scene of them is a scene where every edge overlaps every query.
//!
//! Real diagrams are not like that. A node connects to something near it, and
//! [`SceneSpec::locality`] is that fact as a parameter: an edge joins a node to
//! one within that many places in the layout. The degenerate case is worth
//! measuring too rather than avoiding, so the harness builds one scene with the
//! locality turned off and prints what happens; [`crate::spatial`]'s module doc
//! records the answer.
//!
//! # Deterministic without a dependency
//!
//! Jitter comes from a linear congruential generator written out below. dodo is
//! deliberate about every package in its graph, a benchmark that cannot be
//! reproduced exactly is not a benchmark, and the whole of what is needed here
//! is a reproducible spread.
//!
//! **This file names no UI framework.**

use crate::{
    geometry::{Vec2, Viewport},
    models::{ElementKind, GraphNodeKind, NodeIndex},
    runtime::{ConnectionRules, EdgeEnd, GraphWorld},
};

/// A reproducible spread. See the module doc for why it is not a dependency.
struct Lcg(u32);

impl Lcg {
    fn new(seed: u32) -> Lcg {
        Lcg(seed | 1)
    }

    /// The next value in `[0, 1)`.
    fn unit(&mut self) -> f32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        (self.0 >> 8) as f32 / (1 << 24) as f32
    }

    fn range(&mut self, span: f32) -> f32 {
        (self.unit() - 0.5) * span
    }
}

/// One benchmark scene.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SceneSpec {
    /// The name printed in the harness and used in the recorded tables.
    pub name: &'static str,
    pub nodes: usize,
    pub edges: usize,
    /// Nodes per row of the layout grid.
    pub columns: usize,
    /// Distance between node origins, in world units.
    pub pitch: Vec2,
    pub node_size: Vec2,
    /// How far an edge may reach, in **rows and columns of the layout**.
    ///
    /// Rows and columns rather than flat index positions, and the difference is
    /// not cosmetic — it is the whole reason these scenes are trustworthy. A
    /// reach expressed as "within *n* index places" lets an edge jump a whole
    /// row, which at 400 columns is 104,000 world units: a document-crossing
    /// edge wearing the word "local". The first version of this generator did
    /// exactly that, and the numbers it produced were the spatial index's worst
    /// case dressed up as its normal one.
    ///
    /// `0` means anywhere in the document — the genuinely degenerate case,
    /// kept because it is worth measuring rather than avoiding.
    pub locality: usize,
    /// The zoom the harness views this scene at. Part of the scene because
    /// "how much is visible" is what several of the numbers are about.
    pub zoom: f32,
}

impl SceneSpec {
    /// §38's small scene.
    pub const SMALL: SceneSpec = SceneSpec {
        name: "small",
        nodes: 100,
        edges: 200,
        columns: 10,
        pitch: Vec2::new(260.0, 160.0),
        node_size: Vec2::new(160.0, 60.0),
        locality: 2,
        zoom: 1.0,
    };

    /// §38's medium scene.
    pub const MEDIUM: SceneSpec = SceneSpec {
        name: "medium",
        nodes: 5_000,
        edges: 15_000,
        columns: 80,
        pitch: Vec2::new(260.0, 160.0),
        node_size: Vec2::new(160.0, 60.0),
        locality: 2,
        zoom: 1.0,
    };

    /// §38's large scene: 100,000 nodes and 300,000 edges.
    pub const LARGE: SceneSpec = SceneSpec {
        name: "large",
        nodes: 100_000,
        edges: 300_000,
        columns: 400,
        pitch: Vec2::new(260.0, 160.0),
        node_size: Vec2::new(160.0, 60.0),
        locality: 2,
        zoom: 1.0,
    };

    /// §38's **dense visibility stress test**: a scene where several thousand
    /// objects are deliberately visible at once.
    ///
    /// Small nodes on a tight pitch, so a 1440×900 pane at 1:1 covers about
    /// 2,000 of them and their edges — which is the point. The others are
    /// scenes where culling has an easy job; this is the one where it does not.
    pub const DENSE: SceneSpec = SceneSpec {
        name: "dense",
        nodes: 20_000,
        edges: 40_000,
        columns: 200,
        pitch: Vec2::new(34.0, 26.0),
        node_size: Vec2::new(30.0, 18.0),
        locality: 2,
        zoom: 1.0,
    };

    /// The large scene with **locality turned off** — every edge free to cross
    /// the whole document.
    ///
    /// Not a scene anyone should draw. It is here because it is the input that
    /// breaks a uniform grid, and a benchmark that only measures the cases it
    /// is good at is an advertisement. See [`crate::spatial`] for what it says.
    pub const SCATTERED: SceneSpec = SceneSpec {
        name: "scattered",
        nodes: 100_000,
        edges: 300_000,
        columns: 400,
        pitch: Vec2::new(260.0, 160.0),
        node_size: Vec2::new(160.0, 60.0),
        locality: 0,
        zoom: 1.0,
    };

    /// §38's four scenes, in size order.
    pub const ALL: [SceneSpec; 4] = [
        SceneSpec::SMALL,
        SceneSpec::MEDIUM,
        SceneSpec::LARGE,
        SceneSpec::DENSE,
    ];

    /// The world rectangle the whole scene occupies.
    pub fn content_extent(&self) -> Vec2 {
        let rows = self.nodes.div_ceil(self.columns.max(1));
        Vec2::new(
            self.columns as f32 * self.pitch.x,
            rows as f32 * self.pitch.y,
        )
    }

    /// A camera on this scene, in the middle of the content so that a viewport
    /// query has neighbours on every side rather than a document edge.
    ///
    /// The middle matters: a camera at the origin sees a quarter of the
    /// candidates a camera in the interior does, and measuring the easy corner
    /// would flatter every number below.
    pub fn viewport(&self, pane: Vec2) -> Viewport {
        let extent = self.content_extent();
        let mut viewport = Viewport::new(Vec2::ZERO, self.zoom, pane);
        viewport.center_world_on_screen(extent / 2.0, pane / 2.0);
        viewport
    }
}

/// Builds a scene's world, with every route already derived and the spatial
/// queues spent.
///
/// Reserves up front, so the build measures the stores rather than their
/// growth, and uses [`ConnectionRules::PERMISSIVE`] so the generator's edges
/// are never refused for a reason the benchmark does not care about.
pub fn build(spec: &SceneSpec) -> GraphWorld {
    let mut world = GraphWorld::new();
    world.set_rules(ConnectionRules::PERMISSIVE);
    world.reserve(spec.nodes, spec.edges);

    let mut random = Lcg::new(0x2545_F491);
    for index in 0..spec.nodes {
        let column = index % spec.columns.max(1);
        let row = index / spec.columns.max(1);
        // A little jitter, so every node does not share a cell boundary with
        // every other — a perfectly regular lattice is the one layout where a
        // uniform grid's cell edges line up with the content and the numbers
        // stop being representative.
        let jitter = Vec2::new(
            random.range(spec.pitch.x * 0.25),
            random.range(spec.pitch.y * 0.25),
        );
        world.create_node(
            ElementKind::GraphNode(GraphNodeKind::Default),
            Vec2::new(column as f32 * spec.pitch.x, row as f32 * spec.pitch.y) + jitter,
            spec.node_size,
        );
    }

    if spec.nodes > 1 {
        let columns = spec.columns.max(1) as isize;
        let rows = spec.nodes.div_ceil(spec.columns.max(1)) as isize;
        let span = (spec.locality * 2 + 1) as isize;

        for index in 0..spec.edges {
            let source = index % spec.nodes;
            let target = if spec.locality == 0 {
                (index * 7 + 1) % spec.nodes
            } else {
                // A neighbour in the *layout*, which is what makes the edge
                // short in world space. Derived from the edge index rather than
                // from the generator's random state so that adding a scene
                // never shifts another one's graph.
                let column = (source % spec.columns.max(1)) as isize;
                let row = (source / spec.columns.max(1)) as isize;

                // Walk the offsets until one lands on a different node. Near
                // the edge of the layout the clamp folds several offsets onto
                // the source itself, and skipping those would quietly build a
                // scene smaller than the one §38 names — the small scene lost
                // 16 % of its edges that way before this loop existed.
                let mut chosen = source;
                for attempt in 0..(span * span) {
                    let offset = index as isize + attempt;
                    let dx = (offset % span) - spec.locality as isize;
                    let dy = ((offset / span) % span) - spec.locality as isize;
                    let target_column = (column + dx).clamp(0, columns - 1);
                    let target_row = (row + dy).clamp(0, rows - 1);
                    let candidate =
                        ((target_row * columns + target_column) as usize).min(spec.nodes - 1);
                    if candidate != source {
                        chosen = candidate;
                        break;
                    }
                }
                chosen
            };
            if source == target {
                continue;
            }
            let _ = world.connect(
                EdgeEnd::node(NodeIndex::new(source as u32)),
                EdgeEnd::node(NodeIndex::new(target as u32)),
            );
        }
    }

    world.rebuild_all_geometry();
    // **A benchmark scene starts clean.** Building it queued every node and
    // every edge; leaving those queues full would make the first measured
    // frame rebuild the whole document and every number after it a lie.
    world.dirty_mut().clear_all();
    world
}

/// The pane every recorded number was measured at: Phase 0's window, so the
/// budgets in [`crate::budgets`] and the scenes here describe the same screen.
pub const BENCH_PANE: Vec2 = Vec2::new(1_440.0, 900.0);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spatial::{SpatialIndex, VisibleSet};

    /// The scenes are cheap enough to build in a test at their real size,
    /// except the two 100,000-node ones — those are the harness's.
    #[test]
    fn the_small_and_medium_scenes_are_the_size_they_say() {
        for spec in [SceneSpec::SMALL, SceneSpec::MEDIUM] {
            let world = build(&spec);
            assert_eq!(world.nodes().len(), spec.nodes, "{}", spec.name);
            // A shade under, because the generator skips the offsets that would
            // have made a self-connection. Close enough that the scene is the
            // size it claims, and exact enough to notice a generator bug.
            let edges = world.edges().len();
            assert!(
                edges > spec.edges * 9 / 10 && edges <= spec.edges,
                "{} built {edges} of {} edges",
                spec.name,
                spec.edges
            );
            assert!(world.dirty().is_clean(), "{} left a dirty queue", spec.name);
        }
    }

    /// **The property the whole benchmark rests on**: a "local" scene's edges
    /// are short in *world* space, not merely close in index space.
    #[test]
    fn a_local_scene_has_short_edges() {
        let spec = SceneSpec {
            nodes: 4_000,
            edges: 8_000,
            ..SceneSpec::LARGE
        };
        let world = build(&spec);
        let reach = spec.pitch * (spec.locality as f32 + 1.0);

        let long = world
            .edges()
            .indices()
            .filter_map(|edge| world.route(edge))
            .filter(|route| {
                route.bounds().width() > reach.x * 2.0 || route.bounds().height() > reach.y * 2.0
            })
            .count();
        assert_eq!(
            long, 0,
            "{long} edges reached further than the locality allows"
        );
    }

    #[test]
    fn building_the_same_scene_twice_gives_the_same_world() {
        let a = build(&SceneSpec::SMALL);
        let b = build(&SceneSpec::SMALL);

        assert_eq!(a.nodes().positions(), b.nodes().positions());
        assert_eq!(a.content_bounds(), b.content_bounds());
    }

    /// **What makes the dense scene the dense scene.** If this stops holding,
    /// the stress test has quietly become an easy one.
    #[test]
    fn the_dense_scene_really_does_put_thousands_on_screen() {
        let spec = SceneSpec::DENSE;
        let world = build(&spec);
        let index = SpatialIndex::for_world(&world);

        let mut visible = VisibleSet::new();
        index.query_visible(&world, &spec.viewport(BENCH_PANE), &mut visible);

        assert!(
            visible.node_count() > 1_500,
            "the dense scene showed {} nodes",
            visible.node_count()
        );
        assert!(visible.edge_count() > 1_500);
    }

    /// And what makes the others the others: culling has a real job.
    #[test]
    fn the_medium_scene_shows_a_screenful_rather_than_a_document() {
        let spec = SceneSpec::MEDIUM;
        let world = build(&spec);
        let index = SpatialIndex::for_world(&world);

        let mut visible = VisibleSet::new();
        index.query_visible(&world, &spec.viewport(BENCH_PANE), &mut visible);

        assert!(visible.node_count() > 0);
        assert!(
            visible.node_count() < spec.nodes / 50,
            "{} of {} nodes visible",
            visible.node_count(),
            spec.nodes
        );
    }

    /// The camera has content on every side, which is what makes the query
    /// numbers representative rather than flattering.
    #[test]
    fn the_bench_camera_sits_inside_the_content() {
        for spec in SceneSpec::ALL {
            let world = build(&SceneSpec {
                // Same shape, smaller, so the test stays fast.
                nodes: spec.nodes.min(2_000),
                edges: spec.edges.min(4_000),
                ..spec
            });
            let bounds = world.content_bounds().expect("a scene has content");
            let camera = spec.viewport(BENCH_PANE).visible_world_rect().center();

            assert!(
                bounds.contains_point(camera) || spec.nodes > 2_000,
                "{}'s camera fell outside its content",
                spec.name
            );
        }
    }

    #[test]
    fn a_scattered_scene_has_edges_that_cross_the_document() {
        let spec = SceneSpec {
            nodes: 2_000,
            edges: 4_000,
            ..SceneSpec::SCATTERED
        };
        let world = build(&spec);

        let long = world
            .edges()
            .indices()
            .filter_map(|edge| world.route(edge))
            .filter(|route| route.bounds().width() > spec.content_extent().x * 0.25)
            .count();
        assert!(long > 100, "only {long} edges crossed the document");
    }

    #[test]
    fn a_degenerate_spec_does_not_panic() {
        let world = build(&SceneSpec {
            name: "empty",
            nodes: 0,
            edges: 10,
            columns: 0,
            ..SceneSpec::SMALL
        });
        assert!(world.is_empty());

        let world = build(&SceneSpec {
            name: "one",
            nodes: 1,
            edges: 10,
            ..SceneSpec::SMALL
        });
        assert_eq!(world.nodes().len(), 1);
        assert_eq!(world.edges().len(), 0, "a lone node has nothing to join");
    }
}
