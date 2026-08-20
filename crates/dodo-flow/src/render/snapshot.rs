//! §24's render snapshot — **the boundary between the graph and the frame**,
//! and the one place the LOD ladder is spent.
//!
//! ```text
//! GraphWorld ─> SpatialIndex ─> VisibleSet ─> RenderSnapshot ─┬─> PaintPlan   (canvas)
//!  100,000 nodes    2.3 µs        ~36 nodes      LOD + registry └─> views       (elements)
//! ```
//!
//! # What §24 asks for, and what "never cloned metadata" costs to keep
//!
//! *"The snapshot should contain compact IDs/references rather than cloning all
//! node metadata."* Every field below is a runtime index, a screen rectangle or
//! a `Copy` descriptor. **There is no `String`, no `ElementKind`, no
//! `ElementStyle` and no `ElementId` anywhere in it** — `a_snapshot_holds_no_heap_metadata`
//! is the test, and `snapshot_extraction_allocates_nothing_on_a_steady_frame`
//! is the reason it matters: the buffers are reused, so a pan refills them
//! rather than reallocating (§40 rules 13 and 14).
//!
//! Colours are the interesting case. The snapshot carries **no colours at
//! all**: the painter resolves them per frame from the theme and the element's
//! own style, exactly as it did in Phase 4. A snapshot that carried resolved
//! colours would be a snapshot that goes stale when the theme changes, and
//! dodo applies a theme change live.
//!
//! # The split this module decides
//!
//! One visible node goes down exactly one of three routes, and which one is
//! §15's and §16's whole answer:
//!
//! | route | what draws it | when |
//! |---|---|---|
//! | [`RichNode`] | a real GPUI element in `views/` | full detail, rectangular body, big enough to read |
//! | [`CanvasNode`] | the painter, as a quad or a path | everything else |
//! | skipped | nothing | a kind with no representation yet |
//!
//! A rich node is **excluded from the canvas list**, because its element *is*
//! its body — that is what gives it hover, focus, a cursor and §47's standard
//! UI behaviours, and painting a quad under it would draw it twice.
//!
//! **A non-rectangular body never becomes an element.** A `div` cannot be a
//! diamond or an ellipse, so a decision node stays canvas-painted and gets a
//! canvas label instead. That is a real limitation of the hybrid approach and
//! it is stated rather than worked around; the alternative — an element with a
//! painted body behind it — draws every such node twice and doubles its cost
//! for chrome nobody asked for.
//!
//! # The count is bounded by the budget, not by the document *or* the screen
//!
//! §16's requirement is that 100,000 document nodes produce ~70 elements rather
//! than 100,000. The spatial query already delivers that. What this module adds
//! is the second bound: even a viewport that genuinely contains thousands of
//! nodes — Phase 4's dense scene shows 1,584 — produces at most
//! [`RenderBudgets::max_rich_elements`] of them, and the rest fall back to the
//! canvas. Phase 0 measured 1,600 rich elements holding 60 fps, so the ceiling
//! is where the platform actually is rather than where the requirements
//! guessed.
//!
//! `a_hundred_thousand_nodes_produce_tens_of_elements` is §16 as a test rather
//! than as an observation, which is this phase's exit criterion.
//!
//! # §44: controls exist for the active element, never for every element
//!
//! [`SnapshotOverlay`] is populated for the **one** node that is selected, and
//! [`RenderSnapshot::interactive_handles`] for the node that is selected or
//! hovered. Every other node's handles are painted dots. That is the difference
//! between a canvas whose control hierarchy is proportional to the selection
//! and one whose control hierarchy is proportional to the screen.
//!
//! **This file names no UI framework.**

use crate::{
    budgets::{DetailLevel, RenderBudgets},
    geometry::{Rect, Vec2, Viewport},
    models::{EdgeIndex, HandleIndex, NodeIndex},
    render::{
        cache::ScreenAnchor,
        lod::{HandleDetail, LodPlan, SceneLoad},
        registry::{NodeRendererRegistry, NodeVisual},
    },
    runtime::{GraphWorld, NodeShape},
    spatial::VisibleSet,
};

/// A node that becomes a real GPUI element (§16, §43, §47).
///
/// Compact by construction: one index, one rectangle, one `Copy` descriptor and
/// three flags. The label text is **not** here — `views/` reads it from the
/// store through the index, which is what keeps a per-frame `String` clone out
/// of the one loop §40 rule 10 is about.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RichNode {
    pub node: NodeIndex,
    /// Pane-relative screen pixels.
    pub screen: Rect,
    pub visual: NodeVisual,
    /// The node's appearance version — §23's cache key, carried so a view can
    /// tell whether anything about this node actually changed.
    pub version: u32,
    pub selected: bool,
    pub hovered: bool,
    /// The quantised size its label is shaped at, or `None` when there is no
    /// label to shape.
    pub label_font_size: Option<f32>,
}

/// A node the painter draws.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CanvasNode {
    pub node: NodeIndex,
    pub screen: Rect,
    /// The body to paint — **from the registry when it overrode one**, which is
    /// how a registered kind gets a diamond without a new taxonomy variant.
    pub body: NodeShape,
    pub version: u32,
    pub selected: bool,
    /// Whether this node is large enough on screen to be worth more than a
    /// plain box: a border, a label, handles. §15's "merge/simplify visual
    /// details", decided once here rather than three times downstream.
    pub detailed: bool,
    /// The quantised label size, or `None`.
    pub label_font_size: Option<f32>,
    /// **The version this node's shaped line is keyed on** (§9) — its
    /// [`NodeStore::text_version`](crate::runtime::NodeStore::text_version),
    /// not the geometry one beside it, so a dragged node does not re-shape.
    pub text_version: u32,
}

/// An edge that survived the ladder's count cap.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlannedEdge {
    pub edge: EdgeIndex,
    /// The route's geometry version — §23's cache key.
    pub version: u32,
    pub selected: bool,
    /// **The quantised size this edge's label is shaped at, or `None`** (§9).
    ///
    /// `None` covers three different facts and the caller does not need to tell
    /// them apart: the edge has no label, the rung is `Overview`, or the label
    /// would render below the readable floor. All three mean *do not lay it
    /// out*, which is §15's first bullet costing nothing.
    pub label_font_size: Option<f32>,
}

/// One handle that gets a real element: hoverable, with a cursor (§44).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InteractiveHandle {
    pub node: NodeIndex,
    pub handle: HandleIndex,
    /// Pane-relative screen pixels, at the handle's centre.
    pub center: Vec2,
}

/// §44's selection overlay: the bounding box and the transform controls for the
/// **one** element the user is working on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SnapshotOverlay {
    pub node: NodeIndex,
    /// The selected node's screen rectangle.
    pub screen: Rect,
    /// Whether the node is large enough on screen for a toolbar to make sense.
    /// A toolbar over a six-pixel node is a toolbar over nothing.
    pub shows_toolbar: bool,
}

/// What one extraction decided, in counts. What a benchmark prints and what
/// §16's test asserts.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SnapshotCounts {
    pub rich_nodes: u32,
    pub canvas_nodes: u32,
    pub edges: u32,
    pub interactive_handles: u32,
    /// Visible nodes that would have been rich but hit
    /// [`RenderBudgets::max_rich_elements`]. **Non-zero is not a bug** — it is
    /// the ceiling doing its job — but it is worth seeing.
    pub demoted_rich: u32,
    /// Visible edges the ladder's count cap skipped. Non-zero only on a scene
    /// culling cannot bound; see [`crate::render::lod`].
    pub skipped_edges: u32,
    /// Visible nodes whose kind has no representation yet — text, images,
    /// frames. **Not** a culling number; a not-implemented one.
    pub unsupported_nodes: u32,
}

/// **§24's snapshot.** Built once per frame, drawn twice — by the painter and
/// by the element tree.
///
/// Held on the view and refilled rather than rebuilt, so a pan reuses every
/// buffer (§40 rule 14).
#[derive(Debug, Clone, Default)]
pub struct RenderSnapshot {
    lod: Option<LodPlan>,
    load: SceneLoad,
    anchor: Option<ScreenAnchor>,
    pane: Rect,
    rich: Vec<RichNode>,
    canvas: Vec<CanvasNode>,
    edges: Vec<PlannedEdge>,
    interactive_handles: Vec<InteractiveHandle>,
    overlay: Option<SnapshotOverlay>,
    counts: SnapshotCounts,
}

impl RenderSnapshot {
    pub fn new() -> RenderSnapshot {
        RenderSnapshot::default()
    }

    /// **Extracts one frame** from the world, the visible set and the camera.
    ///
    /// Nothing here iterates the document: `visible` is the whole input, which
    /// is §40 rule 1 and what makes this function's cost proportional to the
    /// screen rather than to the file. The buffers are cleared and refilled, so
    /// a steady frame allocates nothing.
    #[allow(clippy::too_many_arguments)]
    pub fn extract(
        &mut self,
        world: &GraphWorld,
        visible: &VisibleSet,
        viewport: &Viewport,
        budgets: &RenderBudgets,
        registry: &NodeRendererRegistry,
        hovered: Option<NodeIndex>,
        pane: Rect,
    ) {
        self.load = SceneLoad::measure(world, visible, viewport);
        // §13's style is the document's, and the ladder decides whether this
        // frame can afford it — see `LodPlan::sketch`. Asking the settings here
        // rather than at each paint site is what makes "switching Clean↔Sketch
        // touches no element" true: one field is read one more time.
        let lod = LodPlan::choose(
            budgets,
            viewport.zoom(),
            self.load,
            world.settings().sketch_request(),
        );

        self.lod = Some(lod);
        self.anchor = Some(ScreenAnchor::of(viewport));
        self.pane = pane;
        self.rich.clear();
        self.canvas.clear();
        self.edges.clear();
        self.interactive_handles.clear();
        self.overlay = None;
        self.counts = SnapshotCounts::default();

        self.extract_edges(world, visible, viewport, budgets, &lod);
        self.extract_nodes(world, visible, viewport, budgets, registry, hovered, &lod);
        self.extract_controls(world, viewport, &lod, hovered, budgets);

        self.counts.rich_nodes = self.rich.len() as u32;
        self.counts.canvas_nodes = self.canvas.len() as u32;
        self.counts.edges = self.edges.len() as u32;
        self.counts.interactive_handles = self.interactive_handles.len() as u32;
    }

    /// Every visible edge that survives the ladder, in visible order.
    ///
    /// **The cap is applied by taking a prefix rather than by choosing**: the
    /// visible order is the grid's, which is stable frame to frame, so a scene
    /// that is over budget draws the *same* subset each frame rather than a
    /// different one — a flickering hairball is worse than a partial one.
    fn extract_edges(
        &mut self,
        world: &GraphWorld,
        visible: &VisibleSet,
        viewport: &Viewport,
        budgets: &RenderBudgets,
        lod: &LodPlan,
    ) {
        let thresholds = &budgets.lod;
        for &edge in visible.edges() {
            if self.edges.len() as u32 >= lod.max_edges {
                self.counts.skipped_edges += 1;
                continue;
            }

            let Some(route) = world.route(edge) else {
                // A stale route is skipped rather than drawn from: it would
                // show an edge hanging off a node that has already moved.
                continue;
            };

            // §15's cheapest rung, and the only one that costs literally
            // nothing: an edge a few pixels long is a smudge on a node's
            // border and still costs a whole path.
            let screen_length = viewport.world_to_screen_length(
                (route.end() - route.start())
                    .length()
                    .max(route.bounds().size.length() * 0.5),
            );
            if !lod.edge_is_worth_drawing(screen_length, thresholds) {
                self.counts.skipped_edges += 1;
                continue;
            }

            // **The label's size is decided here, from the edge's own style**,
            // for the same reason a node's is: the ladder answers per element,
            // and an edge labelled `XL` survives a zoom-out that an `S` one
            // does not.
            let label_font_size = world
                .edges()
                .label(edge)
                .is_some()
                .then(|| lod.font_size_for(thresholds, world.edges().style(edge).font.world_size()))
                .flatten();

            self.edges.push(PlannedEdge {
                edge,
                version: world.geometry().version(edge),
                selected: world.edges().is_selected(edge),
                label_font_size,
            });
        }
    }

    /// Every visible node, down one of the three routes. See the module doc.
    #[allow(clippy::too_many_arguments)]
    fn extract_nodes(
        &mut self,
        world: &GraphWorld,
        visible: &VisibleSet,
        viewport: &Viewport,
        budgets: &RenderBudgets,
        registry: &NodeRendererRegistry,
        hovered: Option<NodeIndex>,
        lod: &LodPlan,
    ) {
        let nodes = world.nodes();
        let thresholds = budgets.lod;

        for &node in visible.nodes() {
            let shape = nodes.shape(node);
            let screen = viewport.world_rect_to_screen(nodes.bounds(node));
            let detailed = lod.node_deserves_detail(screen, &thresholds);
            let selected = nodes.is_selected(node);
            let version = nodes.version(node);

            // The registry is consulted **only** for a kind the hot array
            // cannot answer — see `render::registry`: a 100,000-node document
            // of ordinary graph nodes does no lookups at all.
            let visual = if shape == NodeShape::Other {
                registry.visual(crate::render::registry::NodeRef {
                    index: node,
                    kind: nodes.kind(node),
                    label: nodes.cold(node).label.as_deref(),
                    size: nodes.size(node),
                    handle_count: nodes.handle_count(node) as u32,
                    selected,
                })
            } else {
                NodeVisual {
                    body: shape,
                    ..registry.visual(crate::render::registry::NodeRef {
                        index: node,
                        kind: nodes.kind(node),
                        label: nodes.cold(node).label.as_deref(),
                        size: nodes.size(node),
                        handle_count: nodes.handle_count(node) as u32,
                        selected,
                    })
                }
            };

            if visual.body == NodeShape::Other {
                // Text, images, frames, freehand and an unregistered custom
                // kind. A fallback rectangle would silently draw them and hide
                // the fact that they are not implemented.
                self.counts.unsupported_nodes += 1;
                continue;
            }

            // **The element's own step, not the frame's default** (§9). Two
            // nodes at two authored sizes get two answers from the same rung,
            // and each disappears at the zoom where *it* stops being readable.
            //
            // **`detailed` is not asked of a text element**, and that is a
            // finding rather than an exception. `min_detailed_node_px` asks
            // whether a *body* is big enough to be worth a border and a line of
            // label **inside** it — 24 px, "two node bodies' worth of border
            // and a label's line height". A standalone text element has no
            // border and no body: it is exactly one line tall, 22 world units
            // by default, so it would never qualify and the whole kind would
            // have been invisible at 100 % zoom. The only legibility question
            // it has is whether its glyphs can be read, and `font_size_for` is
            // already the answer to that.
            let laid_out = visual.shows_label
                && nodes.cold(node).label.is_some()
                && (detailed || visual.body == NodeShape::Text);
            let label_font_size = laid_out
                .then(|| lod.font_size_for(&thresholds, nodes.style(node).font.world_size()))
                .flatten();

            let wants_element = lod.detail == DetailLevel::Full
                && detailed
                && is_rectangular(visual.body)
                && nodes.cold(node).parent.is_none();

            if wants_element {
                if self.rich.len() as u32 >= lod.max_rich_nodes.min(budgets.max_rich_elements) {
                    self.counts.demoted_rich += 1;
                } else {
                    self.rich.push(RichNode {
                        node,
                        screen,
                        visual,
                        version,
                        selected,
                        hovered: hovered == Some(node),
                        label_font_size,
                    });
                    continue;
                }
            }

            self.canvas.push(CanvasNode {
                node,
                screen,
                body: visual.body,
                version,
                selected,
                detailed,
                label_font_size,
                text_version: nodes.text_version(node),
            });
        }
    }

    /// §44's controls: the selection overlay, and interactive handles for the
    /// **one** node that is being worked on.
    fn extract_controls(
        &mut self,
        world: &GraphWorld,
        viewport: &Viewport,
        lod: &LodPlan,
        hovered: Option<NodeIndex>,
        budgets: &RenderBudgets,
    ) {
        if lod.handles != HandleDetail::Interactive {
            return;
        }

        // Selected first, hovered second: a user who has selected one node and
        // is passing the pointer over another is working on the selection.
        let active = world.selection().single_node().or(hovered);
        let Some(active) = active else {
            return;
        };
        if !world.nodes().contains(active) {
            return;
        }

        let screen = viewport.world_rect_to_screen(world.nodes().bounds(active));
        for handle in world.nodes().handles(active) {
            if world.handles().is_hidden(handle) {
                continue;
            }
            self.interactive_handles.push(InteractiveHandle {
                node: active,
                handle,
                center: viewport.world_to_screen(world.handle_position(handle)),
            });
        }

        if world.selection().single_node() == Some(active) {
            self.overlay = Some(SnapshotOverlay {
                node: active,
                screen,
                shows_toolbar: screen.size.x >= budgets.lod.min_toolbar_node_px
                    && screen.size.y >= budgets.lod.min_detailed_node_px,
            });
        }
    }

    /// The ladder this frame was extracted under, or `None` before the first
    /// extraction.
    pub fn lod(&self) -> Option<LodPlan> {
        self.lod
    }

    /// What was on screen when the ladder was chosen.
    pub fn load(&self) -> SceneLoad {
        self.load
    }

    /// The camera this frame's screen coordinates are in — the geometry cache's
    /// anchor.
    pub fn anchor(&self) -> Option<ScreenAnchor> {
        self.anchor
    }

    pub fn pane(&self) -> Rect {
        self.pane
    }

    /// **The nodes that become GPUI elements** (§16). Tens, not thousands.
    pub fn rich(&self) -> &[RichNode] {
        &self.rich
    }

    pub fn canvas(&self) -> &[CanvasNode] {
        &self.canvas
    }

    pub fn edges(&self) -> &[PlannedEdge] {
        &self.edges
    }

    /// §44's handles, for the selected or hovered node only.
    pub fn interactive_handles(&self) -> &[InteractiveHandle] {
        &self.interactive_handles
    }

    pub fn overlay(&self) -> Option<SnapshotOverlay> {
        self.overlay
    }

    pub fn counts(&self) -> SnapshotCounts {
        self.counts
    }

    /// **Every GPUI element this frame will create.** §16's number, in one
    /// place, so a test can assert it without building an element tree.
    ///
    /// Each rich node is one element, each interactive handle is one, and the
    /// overlay is one. `views/` nests a few children inside each, which is a
    /// constant factor and is what
    /// [`RenderBudgets::max_rich_elements`] was measured against — Phase 0's
    /// 1,600 were `div`s with a background, a border, a label and a hover
    /// style.
    pub fn element_count(&self) -> u32 {
        self.counts.rich_nodes + self.counts.interactive_handles + u32::from(self.overlay.is_some())
    }

    /// Clears everything, keeping the buffers. For a document swap, where every
    /// index means something else.
    pub fn reset(&mut self) {
        self.rich.clear();
        self.canvas.clear();
        self.edges.clear();
        self.interactive_handles.clear();
        self.overlay = None;
        self.counts = SnapshotCounts::default();
        self.lod = None;
        self.anchor = None;
    }
}

/// Whether a body can be a `div`. See the module doc: a diamond cannot.
fn is_rectangular(body: NodeShape) -> bool {
    matches!(
        body,
        NodeShape::GraphNode | NodeShape::Rectangle | NodeShape::RoundedRectangle
    )
}

/// The thresholds a plan was chosen against.
///
/// Re-read from the defaults rather than carried on the plan: the ladder's
/// thresholds are configuration in [`crate::budgets`], and duplicating them
/// onto every plan would be a second copy that could disagree with the first.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        budgets::{RenderBackend, for_backend},
        geometry::Vec2,
        models::{ElementKind, GraphNodeKind},
        models::{HandleDirection, HandlePlacement},
        render::registry::GenericKind,
        runtime::{ConnectionRules, EdgeEnd, HandleSpec},
        scenes::{self, SceneSpec},
        spatial::SpatialIndex,
    };

    fn budgets() -> RenderBudgets {
        for_backend(RenderBackend::Metal)
    }

    struct Harness {
        world: GraphWorld,
        spatial: SpatialIndex,
        visible: VisibleSet,
        viewport: Viewport,
        snapshot: RenderSnapshot,
        registry: NodeRendererRegistry,
    }

    impl Harness {
        fn from_world(mut world: GraphWorld, pane: Vec2) -> Harness {
            world.rebuild_all_geometry();
            world.clear_spatial_updates();
            let spatial = SpatialIndex::for_world(&world);
            let mut viewport = Viewport::default();
            viewport.set_size(pane);

            Harness {
                world,
                spatial,
                visible: VisibleSet::new(),
                viewport,
                snapshot: RenderSnapshot::new(),
                registry: NodeRendererRegistry::with_generic_kinds(),
            }
        }

        fn frame(&mut self, hovered: Option<NodeIndex>) -> &RenderSnapshot {
            self.spatial
                .query_visible(&self.world, &self.viewport, &mut self.visible);
            self.snapshot.extract(
                &self.world,
                &self.visible,
                &self.viewport,
                &budgets(),
                &self.registry,
                hovered,
                Rect::new(Vec2::ZERO, self.viewport.size()),
            );
            &self.snapshot
        }
    }

    /// A locality-preserving grid of connected nodes, each with the two handles
    /// §4's default placement gives a graph node — `create_node` adds none, so
    /// a test that wants handles has to say so.
    fn grid_world(columns: u32, rows: u32) -> GraphWorld {
        let mut world = GraphWorld::new();
        world.set_rules(ConnectionRules::PERMISSIVE);
        for row in 0..rows {
            for column in 0..columns {
                let node = world.create_node(
                    ElementKind::GraphNode(GraphNodeKind::Default),
                    Vec2::new(column as f32 * 240.0, row as f32 * 140.0),
                    Vec2::new(160.0, 60.0),
                );
                world.add_handle(
                    node,
                    HandleSpec::new("in", HandlePlacement::Left, HandleDirection::Target),
                );
                world.add_handle(
                    node,
                    HandleSpec::new("out", HandlePlacement::Right, HandleDirection::Source),
                );
            }
        }
        for row in 0..rows {
            for column in 0..columns.saturating_sub(1) {
                let index = row * columns + column;
                world
                    .connect(
                        EdgeEnd::node(NodeIndex::new(index)),
                        EdgeEnd::node(NodeIndex::new(index + 1)),
                    )
                    .expect("permissive rules accept it");
            }
        }
        world
    }

    /// **§16, as a test rather than as an observation** — this phase's exit
    /// criterion, in the units the requirements state it in.
    #[test]
    fn a_hundred_thousand_nodes_produce_tens_of_elements() {
        let spec = SceneSpec::LARGE;
        let world = scenes::build(&spec);
        assert!(world.nodes().len() >= 100_000, "the scene must be large");

        let mut harness = Harness::from_world(world, scenes::BENCH_PANE);
        harness.viewport = spec.viewport(scenes::BENCH_PANE);
        let snapshot = harness.frame(None);

        let elements = snapshot.element_count();
        assert!(
            elements < 100,
            "{elements} GPUI elements from a 100,000-node document; \
             §16 asks for tens"
        );
        assert!(
            snapshot.counts().rich_nodes > 0,
            "and it must actually be drawing something"
        );
    }

    /// The other half of §16: the element count follows the **screen**, not the
    /// document. A document a thousand times larger at the same camera makes
    /// the same number of elements.
    #[test]
    fn the_element_count_is_set_by_the_screen_and_not_by_the_document() {
        // Both documents are larger than the pane, so the camera — not the
        // document — is what decides. A document that fits on screen would
        // prove nothing here.
        let mut small = Harness::from_world(grid_world(20, 20), Vec2::new(1_440.0, 900.0));
        let mut large = Harness::from_world(grid_world(200, 200), Vec2::new(1_440.0, 900.0));

        let small_count = small.frame(None).element_count();
        let large_count = large.frame(None).element_count();

        assert!(small_count > 0, "the camera must be seeing something");
        assert_eq!(
            small_count, large_count,
            "a 100× larger document at the same camera produced a different \
             element count ({small_count} against {large_count})"
        );
    }

    /// Phase 4's dense scene genuinely puts 1,584 nodes on screen. Culling
    /// cannot help there — they really are visible — so the **ceiling** is what
    /// bounds the element tree, and the rest fall back to the canvas rather
    /// than disappearing.
    #[test]
    fn a_genuinely_dense_viewport_is_bounded_by_the_ceiling_not_by_culling() {
        let budgets = budgets();
        let spec = SceneSpec::DENSE;
        let world = scenes::build(&spec);
        let mut harness = Harness::from_world(world, scenes::BENCH_PANE);
        harness.viewport = spec.viewport(scenes::BENCH_PANE);

        let snapshot = harness.frame(None);
        let counts = snapshot.counts();

        assert!(
            counts.rich_nodes <= budgets.max_rich_elements,
            "{} rich nodes against a ceiling of {}",
            counts.rich_nodes,
            budgets.max_rich_elements
        );
        assert_eq!(
            counts.rich_nodes + counts.canvas_nodes + counts.unsupported_nodes,
            harness.visible.node_count() as u32,
            "every visible node must go down exactly one route"
        );
    }

    /// §15's first rung, end to end: below full zoom nothing is an element at
    /// all, so nothing has to be culled back.
    #[test]
    fn zooming_out_takes_every_element_away() {
        let mut harness = Harness::from_world(grid_world(6, 6), Vec2::new(1_440.0, 900.0));
        assert!(harness.frame(None).element_count() > 0, "full detail");

        harness.viewport.zoom_around(Vec2::ZERO, 0.4);
        let compact = harness.frame(None);
        assert_eq!(compact.counts().rich_nodes, 0, "compact draws no elements");
        assert!(
            compact.counts().canvas_nodes > 0,
            "but the nodes are still drawn"
        );

        harness.viewport.zoom_around(Vec2::ZERO, 0.1);
        let overview = harness.frame(None);
        assert_eq!(overview.element_count(), 0);
        assert_eq!(
            overview.lod().unwrap().label_font_size,
            None,
            "§15: do not lay out rich text that cannot be read"
        );
    }

    /// §44: controls belong to the active element, not to every element.
    #[test]
    fn only_the_selected_or_hovered_node_gets_interactive_handles() {
        let mut harness = Harness::from_world(grid_world(6, 6), Vec2::new(1_440.0, 900.0));

        assert_eq!(
            harness.frame(None).interactive_handles().len(),
            0,
            "an idle canvas has no handle elements at all"
        );

        harness.world.select_only(Some(NodeIndex::new(0)));
        let selected = harness.frame(None);
        assert!(!selected.interactive_handles().is_empty());
        assert!(
            selected
                .interactive_handles()
                .iter()
                .all(|handle| handle.node == NodeIndex::new(0)),
            "handles leaked to an unselected node"
        );
        assert_eq!(
            selected.overlay().map(|overlay| overlay.node),
            Some(NodeIndex::new(0))
        );
    }

    #[test]
    fn hovering_a_node_gives_it_handles_without_selecting_it() {
        let mut harness = Harness::from_world(grid_world(6, 6), Vec2::new(1_440.0, 900.0));
        let hovered = NodeIndex::new(3);

        let snapshot = harness.frame(Some(hovered));
        assert!(!snapshot.interactive_handles().is_empty());
        assert_eq!(
            snapshot.overlay(),
            None,
            "a hover is not a selection and gets no toolbar"
        );
        assert!(
            snapshot
                .rich()
                .iter()
                .any(|rich| rich.node == hovered && rich.hovered),
            "the hovered node must know it is hovered"
        );
    }

    /// The registry's shape override reaching the frame: a decision node is a
    /// diamond, which no `ElementKind` variant says, and a diamond is not an
    /// element.
    #[test]
    fn a_registered_diamond_kind_is_canvas_drawn_rather_than_an_element() {
        let mut world = GraphWorld::new();
        world.create_node(
            GenericKind::Decision.element_kind(),
            Vec2::new(100.0, 100.0),
            Vec2::new(160.0, 100.0),
        );
        let mut harness = Harness::from_world(world, Vec2::new(1_440.0, 900.0));

        let snapshot = harness.frame(None);
        assert_eq!(snapshot.counts().rich_nodes, 0, "a div cannot be a diamond");
        assert_eq!(snapshot.canvas().len(), 1);
        assert_eq!(snapshot.canvas()[0].body, NodeShape::Diamond);
        assert_eq!(snapshot.counts().unsupported_nodes, 0);
    }

    /// An unregistered custom kind is honestly unsupported rather than quietly
    /// drawn as a rectangle.
    #[test]
    fn an_unregistered_kind_is_counted_rather_than_drawn_as_something_else() {
        let mut world = GraphWorld::new();
        world.create_node(
            ElementKind::Custom(crate::models::CustomKind::new("dodo.mermaid.actor")),
            Vec2::ZERO,
            Vec2::new(160.0, 60.0),
        );
        let mut harness = Harness::from_world(world, Vec2::new(1_440.0, 900.0));

        // The fallback visual is a graph node, so the registry *does* give it a
        // body — the point is that it goes through the registry rather than
        // being special-cased, and that a kind with no body at all is counted.
        let snapshot = harness.frame(None);
        assert_eq!(
            snapshot.counts().rich_nodes + snapshot.counts().unsupported_nodes,
            1
        );
    }

    /// §24's rule 10, checked structurally: the snapshot carries indices and
    /// numbers, and nothing that owns heap memory per element.
    #[test]
    fn a_snapshot_holds_no_heap_metadata() {
        // If any of these grew a `String`, an `ElementKind` or an
        // `ElementStyle`, it would stop being `Copy` and this would not
        // compile. The size assertions are the second half: a `Copy` struct can
        // still be made needlessly fat.
        fn assert_copy<T: Copy>() {}
        assert_copy::<RichNode>();
        assert_copy::<CanvasNode>();
        assert_copy::<PlannedEdge>();
        assert_copy::<InteractiveHandle>();
        assert_copy::<SnapshotOverlay>();

        assert!(size_of::<RichNode>() <= 48, "{}", size_of::<RichNode>());
        assert!(size_of::<CanvasNode>() <= 48, "{}", size_of::<CanvasNode>());
        // 24 rather than 16 since §9's edge labels: a `PlannedEdge` carries
        // the quantised size its label is shaped at, which is an `Option<f32>`
        // and pads the struct to three words. Still `Copy`, still no heap, and
        // still one row per *visible* edge — which is the property the bound is
        // guarding.
        assert!(
            size_of::<PlannedEdge>() <= 24,
            "{}",
            size_of::<PlannedEdge>()
        );
    }

    /// §40 rules 13 and 14: a pan refills the buffers rather than reallocating
    /// them, which is the difference between a smooth drag and a stuttering
    /// one.
    #[test]
    fn a_steady_pan_reuses_the_snapshots_buffers() {
        let mut harness = Harness::from_world(grid_world(40, 40), Vec2::new(1_440.0, 900.0));
        harness.frame(None);

        let capacity = (
            harness.snapshot.rich.capacity(),
            harness.snapshot.canvas.capacity(),
            harness.snapshot.edges.capacity(),
        );

        for _ in 0..60 {
            harness.viewport.pan_by(Vec2::new(1.0, 0.0));
            harness.frame(None);
        }

        assert_eq!(
            (
                harness.snapshot.rich.capacity(),
                harness.snapshot.canvas.capacity(),
                harness.snapshot.edges.capacity(),
            ),
            capacity,
            "a pan grew a snapshot buffer"
        );
    }

    /// The cap is a prefix of a stable order, so an over-budget scene draws the
    /// same subset every frame. A flickering hairball is worse than a partial
    /// one.
    #[test]
    fn an_over_budget_scene_skips_the_same_edges_every_frame() {
        let spec = SceneSpec::SCATTERED;
        let world = scenes::build(&spec);
        let mut harness = Harness::from_world(world, scenes::BENCH_PANE);
        harness.viewport = spec.viewport(scenes::BENCH_PANE);

        let first: Vec<EdgeIndex> = harness.frame(None).edges().iter().map(|e| e.edge).collect();
        let second: Vec<EdgeIndex> = harness.frame(None).edges().iter().map(|e| e.edge).collect();

        assert_eq!(first, second, "the drawn subset moved between frames");
        assert!(
            harness.snapshot.counts().skipped_edges > 0,
            "the scattered scene must be over budget — that is the point of it"
        );
    }

    /// **The hairball, with the ladder engaged.** Phase 4 measured 61,104
    /// visible edges, 147 M estimated vertices and 60,061 paths lost to the
    /// black-window guard. This is what is left of it.
    #[test]
    fn the_hairball_scene_comes_out_inside_both_budgets() {
        let budgets = budgets();
        let spec = SceneSpec::SCATTERED;
        let world = scenes::build(&spec);
        let mut harness = Harness::from_world(world, scenes::BENCH_PANE);
        harness.viewport = spec.viewport(scenes::BENCH_PANE);

        let snapshot = harness.frame(None);
        let lod = snapshot.lod().expect("a frame was extracted");
        let counts = snapshot.counts();

        let paths = counts.edges * lod.edges.paths_per_edge();
        assert!(
            paths <= budgets.target_paths_per_frame,
            "{paths} edge paths against a budget of {}",
            budgets.target_paths_per_frame
        );
        assert!(
            counts.rich_nodes <= budgets.max_rich_elements,
            "{} rich nodes",
            counts.rich_nodes
        );
        assert!(lod.is_degraded(), "the hairball must have degraded");
    }
}
