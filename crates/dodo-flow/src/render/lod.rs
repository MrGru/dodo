//! §15's level-of-detail ladder — **the only thing that bounds a hairball
//! graph**, and the reason it is a module rather than three `if`s.
//!
//! # Why this exists, in one measurement
//!
//! Phase 4 built the spatial index and proved it: a 100,000-node document
//! answers "what can the camera see?" in 2.3 µs, flat from 5,000 nodes to
//! 100,000, against 1,863 µs for the same question by scan. Then it built the
//! **scattered** scene — 100,000 nodes whose edges connect anything to
//! anything — and found the wall:
//!
//! | | large (local edges) | scattered (document-crossing edges) |
//! |---|---:|---:|
//! | visible edges | 126 | **61,104** |
//! | estimated vertices | 31,188 | **147,761,694** |
//! | paths dropped by the black-window guard | 0 | **60,061** |
//!
//! Nothing is wrong with the index. An edge whose control hull spans the
//! document genuinely *is* visible from anywhere in it, and **no spatial
//! structure can cull what really overlaps the viewport**. Culling answers
//! "which elements?"; only simplification answers "how much of each?". That is
//! this module.
//!
//! # The second wall, which Phase 4's table does not show
//!
//! Vertices are not the only ceiling, and on the hairball they are not even the
//! first one reached. [`RenderBudgets::nanos_per_path`] is ~1.5 µs of *fixed*
//! CPU per painted path — `Window::paint_path` consumes its argument, so a
//! cached path is cloned into it, and the vertex array is traversed four times
//! per frame. 61,104 edges is **92 ms of CPU before a single vertex is
//! counted**, and every one of those paths could be two vertices long and it
//! would still be 92 ms.
//!
//! So the ladder spends **two** budgets, not one:
//! [`RenderBudgets::target_paths_per_frame`] and
//! [`RenderBudgets::target_path_vertices_per_frame`]. A tier that halves the
//! vertices and leaves the path count alone has not helped, which is exactly
//! the mistake a vertices-only ladder makes.
//!
//! # The ladder, and what each rung refuses to do
//!
//! [`LodThresholds`] holds the zoom thresholds — they are configuration, in
//! [`crate::budgets`] alongside every other platform constant, because §15 says
//! they must be configurable and tuned by benchmarks later. [`LodPlan`] is one
//! frame's decision made from them.
//!
//! ```text
//! zoom >= 0.60   Full      rich GPUI elements, labels, handles, controls
//! 0.20 - 0.60    Compact   canvas quads + a quantised label, no controls
//! zoom <  0.20   Overview  boxes; no text layout, no handles, no decoration
//! ```
//!
//! Underneath the zoom ladder sits a **load** ladder that the zoom cannot see:
//! [`EdgeDetail`] is chosen from how much is actually on screen, so a hairball
//! at zoom 1.0 degrades its edges while a sparse diagram at the same zoom does
//! not. The two are independent on purpose — zoom is a legibility question and
//! load is a survival one, and conflating them means either a readable canvas
//! that dies on a dense document or a bounded one that throws away detail
//! nobody needed to lose.
//!
//! # The load decision is O(1), not O(visible)
//!
//! [`SceneLoad::measure`] **samples** at most [`LOAD_SAMPLE`] edges to estimate
//! the mean on-screen length. Scanning 61,104 edges to decide how to draw
//! 61,104 edges would be its own per-frame cost, on the frame that can least
//! afford one. The sample is deterministic — a fixed stride — so the tier does
//! not flicker between frames on an unchanged scene.
//!
//! **This file names no UI framework.**

use crate::{
    budgets::{DetailLevel, LodThresholds, RenderBudgets},
    geometry::{Rect, Vec2, Viewport},
    models::RenderQuality,
    render::{
        plan::{DashSpec, PathPaint},
        shapes::Outline,
    },
    runtime::GraphWorld,
    spatial::VisibleSet,
};

/// How many visible edges [`SceneLoad::measure`] looks at. See the module doc:
/// the decision has to be O(1) or it becomes part of the problem it solves.
pub const LOAD_SAMPLE: usize = 64;

/// How an edge is drawn this frame. **The rungs are ordered cheapest last**, so
/// `>` means "more detailed", and `next_coarser` walks down.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum EdgeDetail {
    /// The route as the document describes it: curves tessellated at the
    /// document's own [`RenderQuality`], dashes honoured, markers drawn.
    Full,
    /// The same geometry at a looser flattening tolerance, and **no dashes** —
    /// a dash costs ~63× the vertices of the same line solid (Phase 0 §1.2),
    /// and a dash pattern is unreadable long before its cost stops mattering.
    Coarse,
    /// Corner points only: every cubic collapses to a line to its endpoint, so
    /// the edge keeps its shape and loses its curvature. Markers are dropped —
    /// an arrow head is a second path per edge, and the path count is a budget
    /// of its own.
    Polyline,
    /// Start to end, one straight segment, minimum stroke width. The cheapest
    /// thing that is still an edge.
    Hairline,
}

impl EdgeDetail {
    /// Every rung, most detailed first.
    pub const LADDER: [EdgeDetail; 4] = [
        EdgeDetail::Full,
        EdgeDetail::Coarse,
        EdgeDetail::Polyline,
        EdgeDetail::Hairline,
    ];

    /// Whether the route's curves survive at this rung.
    pub fn keeps_curves(self) -> bool {
        matches!(self, EdgeDetail::Full | EdgeDetail::Coarse)
    }

    /// Whether endpoint markers are drawn. They are a **second path per edge**,
    /// so this is a path-count decision rather than a cosmetic one.
    pub fn keeps_markers(self) -> bool {
        matches!(self, EdgeDetail::Full | EdgeDetail::Coarse)
    }

    /// Whether a dashed edge is still dashed. See [`EdgeDetail::Coarse`].
    pub fn keeps_dashes(self) -> bool {
        matches!(self, EdgeDetail::Full)
    }

    /// The flattening tolerance this rung tessellates at, given the document's.
    ///
    /// Multiplying rather than replacing: a document that asked for
    /// [`RenderQuality::DRAFT`] must not be silently upgraded by a coarse rung,
    /// and one that asked for `PRECISE` must still degrade. The tolerance is
    /// part of every geometry cache key (Phase 0 §3 correction 5), so this is
    /// also what makes a tier change a cache miss rather than a silent
    /// mismatch.
    pub fn quality(self, document: RenderQuality) -> RenderQuality {
        let factor = match self {
            EdgeDetail::Full => 1.0,
            EdgeDetail::Coarse => 4.0,
            // Neither of these tessellates a curve at all, so the tolerance is
            // carried only so the cache key still separates the rungs.
            EdgeDetail::Polyline | EdgeDetail::Hairline => 8.0,
        };
        RenderQuality::new(document.flattening_tolerance * factor)
    }

    /// The next rung down, or `None` at the bottom.
    pub fn next_coarser(self) -> Option<EdgeDetail> {
        match self {
            EdgeDetail::Full => Some(EdgeDetail::Coarse),
            EdgeDetail::Coarse => Some(EdgeDetail::Polyline),
            EdgeDetail::Polyline => Some(EdgeDetail::Hairline),
            EdgeDetail::Hairline => None,
        }
    }

    /// **The vertex estimate for one edge of this on-screen length at this
    /// rung**, built from a synthetic route rather than from a second formula.
    ///
    /// A second formula is how an estimator and a painter drift apart: the
    /// guard would be spending numbers the tessellator never produces. This
    /// builds the shape each rung actually plans — one cubic, one polyline of
    /// [`POLYLINE_SEGMENTS`], one straight line — and asks
    /// [`Outline::estimated_vertices`], which is the same call
    /// [`crate::render::plan`] makes on the real thing and which
    /// `render::painter`'s calibration test keeps honest.
    pub fn estimated_vertices(self, screen_length: f32, quality: RenderQuality) -> u32 {
        let length = screen_length.max(1.0);
        let quality = self.quality(quality);
        let mut outline = Outline::with_capacity(POLYLINE_SEGMENTS + 2);
        outline.move_to(Vec2::ZERO);

        match self {
            EdgeDetail::Full | EdgeDetail::Coarse => {
                // A route's cubic bulges away from the chord; a control hull
                // spanning the length is the shape the router actually emits.
                outline.cubic_to(
                    Vec2::new(length * 0.5, -length * 0.25),
                    Vec2::new(length * 0.5, length * 0.25),
                    Vec2::new(length, 0.0),
                );
            }
            EdgeDetail::Polyline => {
                for step in 1..=POLYLINE_SEGMENTS {
                    let t = step as f32 / POLYLINE_SEGMENTS as f32;
                    outline.line_to(Vec2::new(length * t, 0.0));
                }
            }
            EdgeDetail::Hairline => {
                outline.line_to(Vec2::new(length, 0.0));
            }
        }

        let stroke = PathPaint::Stroke {
            color: crate::models::Color::WHITE,
            width: 1.0,
        };
        let line = outline.estimated_vertices(stroke, quality);

        // A marker is a second path and its own vertices; at the two rungs that
        // keep them, both ends may carry one.
        if self.keeps_markers() {
            line + MARKER_VERTICES * 2
        } else {
            line
        }
    }

    /// How many **paths** one edge costs at this rung. The other budget — see
    /// the module doc; on the hairball it is the one that is reached first.
    pub fn paths_per_edge(self) -> u32 {
        if self.keeps_markers() { 3 } else { 1 }
    }
}

/// The segments a [`EdgeDetail::Polyline`] edge keeps. Enough that an
/// orthogonal route's corners survive — the shape of the route is what a
/// polyline is preserving — and few enough that the estimate stays near a
/// straight line's.
pub const POLYLINE_SEGMENTS: usize = 4;

/// A filled arrow head's vertices, from `geometry::arrow`'s polygons: a closed
/// triangle is three points, and [`Outline::estimated_vertices`] inflates it by
/// its safety margin. Rounded up rather than computed, because it is a constant
/// the estimate only needs a bound on.
pub const MARKER_VERTICES: u32 = 16;

/// How many handles a node is assumed to carry, when the budget is being
/// planned before the handles are walked. Four is §4's default placement — one
/// per side — and the number the scene generator produces.
pub const HANDLES_PER_NODE: u32 = 4;

/// **What is actually on screen this frame**, in the three numbers the ladder
/// spends. Measured in O(1) — see the module doc.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct SceneLoad {
    pub visible_nodes: u32,
    pub visible_edges: u32,
    /// The mean on-screen length of the sampled edges, in screen pixels. Zero
    /// when there are no edges.
    pub mean_edge_screen_length: f32,
}

impl SceneLoad {
    /// Samples the visible set. **Reads at most [`LOAD_SAMPLE`] edges.**
    ///
    /// The stride is derived from the edge count rather than randomised, so two
    /// frames of an unchanged scene sample the same edges and choose the same
    /// rung. A flickering LOD tier is worse than a wrong one.
    pub fn measure(world: &GraphWorld, visible: &VisibleSet, viewport: &Viewport) -> SceneLoad {
        let edges = visible.edges();
        let mut load = SceneLoad {
            visible_nodes: visible.node_count() as u32,
            visible_edges: edges.len() as u32,
            mean_edge_screen_length: 0.0,
        };

        if edges.is_empty() {
            return load;
        }

        let stride = edges.len().div_ceil(LOAD_SAMPLE).max(1);
        let mut total = 0.0;
        let mut sampled = 0u32;

        for &edge in edges.iter().step_by(stride) {
            let Some(route) = world.route(edge) else {
                continue;
            };
            // The chord, not the arc: a bound is not wanted here — this feeds a
            // mean, and a control-hull length would overstate every curve.
            total += (route.end() - route.start()).length();
            sampled += 1;
        }

        if sampled > 0 {
            load.mean_edge_screen_length = viewport.world_to_screen_length(total / sampled as f32);
        }
        load
    }
}

/// How a node's interactive parts are drawn this frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum HandleDetail {
    /// Real elements, with hover state and a cursor — for the few nodes that
    /// have earned them (§44: selected, hovered or editing, never every node).
    Interactive,
    /// Painted dots on the canvas. Cheap, aimable, not hoverable.
    Painted,
    /// Not drawn at all. §15: *do not create handles unless needed for
    /// interaction* — so a connection in progress overrides this.
    Hidden,
}

/// **One frame's level-of-detail decision.**
///
/// A value rather than a set of methods on the renderer, so a test can assert
/// what a given zoom and a given load decide without planning a frame, and so a
/// benchmark can print it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LodPlan {
    pub detail: DetailLevel,
    pub zoom: f32,
    /// The quantised font size node labels are shaped at, or `None` when a
    /// label would be too small to read. **`None` is the common case when
    /// zoomed out**, and it is what §15's "do not lay out rich text that cannot
    /// be read" costs: nothing.
    pub label_font_size: Option<f32>,
    /// Whether a curved shape is painted as its bounding quad (§15, and Phase 0
    /// §3 correction 7: an ellipse is 337 vertices against a rectangle's 24).
    pub degrade_curves: bool,
    pub edges: EdgeDetail,
    /// The most edges that will be planned. Beyond it they are skipped and
    /// counted — see [`crate::render::scene::SceneStats::skipped_edges`].
    pub max_edges: u32,
    /// The most nodes that get a rich GPUI element (§16, §43). Bounded by
    /// [`RenderBudgets::max_rich_elements`] and zero below full detail.
    pub max_rich_nodes: u32,
    pub handles: HandleDetail,
}

impl LodPlan {
    /// **Chooses the whole frame's detail** from the zoom and the load.
    ///
    /// The order matters: the zoom rung is decided first because it is a
    /// legibility judgement and must not be overridden by load, then the edge
    /// rung is walked down from whatever the zoom rung allows until both
    /// budgets fit. A frame that cannot fit even [`EdgeDetail::Hairline`] caps
    /// the edge count instead, which is the last resort and is reported rather
    /// than hidden.
    pub fn choose(budgets: &RenderBudgets, zoom: f32, load: SceneLoad) -> LodPlan {
        let lod = &budgets.lod;
        let detail = lod.detail(zoom);

        let mut plan = LodPlan {
            detail,
            zoom,
            label_font_size: label_font_size(lod, detail, zoom),
            degrade_curves: lod.degrade_curves_to_quads(zoom),
            edges: EdgeDetail::Full,
            max_edges: load.visible_edges,
            max_rich_nodes: match detail {
                DetailLevel::Full => budgets.max_rich_elements,
                // §15, stated as a number: below full detail nothing gets an
                // element, so nothing has to be culled back later.
                DetailLevel::Compact | DetailLevel::Overview => 0,
            },
            handles: match detail {
                DetailLevel::Full => HandleDetail::Interactive,
                DetailLevel::Compact => HandleDetail::Painted,
                DetailLevel::Overview => HandleDetail::Hidden,
            },
        };

        // Below the zoom at which a curve is worth tessellating, an edge's
        // curvature is invisible too — so the ladder starts lower rather than
        // walking down to the same place every frame.
        if plan.degrade_curves {
            plan.edges = EdgeDetail::Polyline;
        }

        plan.fit_edges(budgets, load);
        plan
    }

    /// Walks the edge rung down until both budgets fit, then caps the count.
    ///
    /// The node layer's share is charged first and is not negotiable — the
    /// nodes are what the user is looking at, and a frame that drew every edge
    /// and no nodes would be a worse answer than one that did the reverse.
    fn fit_edges(&mut self, budgets: &RenderBudgets, load: SceneLoad) {
        let (node_paths, node_vertices) = node_layer_cost(budgets, load, self);
        let path_budget = budgets.target_paths_per_frame.saturating_sub(node_paths);
        let vertex_budget = budgets
            .target_path_vertices_per_frame
            .saturating_sub(node_vertices);

        loop {
            let fits = self.edge_layer_fits(load, path_budget, vertex_budget);
            if fits {
                self.max_edges = load.visible_edges;
                return;
            }
            match self.edges.next_coarser() {
                Some(coarser) => self.edges = coarser,
                None => break,
            }
        }

        // The bottom rung still does not fit: cap the count. Whichever budget
        // binds first decides, so a scene bounded by the per-path tax and one
        // bounded by vertices both come out drawable.
        let per_edge_paths = self.edges.paths_per_edge().max(1);
        let per_edge_vertices = self
            .edges
            .estimated_vertices(load.mean_edge_screen_length, RenderQuality::BALANCED)
            .max(1);

        self.max_edges = (path_budget / per_edge_paths).min(vertex_budget / per_edge_vertices);
    }

    fn edge_layer_fits(&self, load: SceneLoad, path_budget: u32, vertex_budget: u32) -> bool {
        let paths = load
            .visible_edges
            .saturating_mul(self.edges.paths_per_edge());
        if paths > path_budget {
            return false;
        }

        let per_edge = self
            .edges
            .estimated_vertices(load.mean_edge_screen_length, RenderQuality::BALANCED);
        load.visible_edges.saturating_mul(per_edge) <= vertex_budget
    }

    /// Whether a node this size on screen is worth more than a plain box.
    ///
    /// Independent of the zoom rung, because a small node at high zoom and a
    /// large node at low zoom are the same legibility problem. §15's "merge and
    /// simplify visual details" as a predicate.
    pub fn node_deserves_detail(&self, screen: Rect, thresholds: &LodThresholds) -> bool {
        screen.size.x >= thresholds.min_detailed_node_px
            && screen.size.y >= thresholds.min_detailed_node_px
    }

    /// Whether an edge this long on screen is worth drawing at all. Below a few
    /// pixels it is a smudge on a node's border and costs a whole path.
    pub fn edge_is_worth_drawing(&self, screen_length: f32, thresholds: &LodThresholds) -> bool {
        screen_length >= thresholds.min_edge_screen_px
    }

    /// Whether this frame degraded anything — the honest signal for a benchmark
    /// and for the launcher's overlay.
    pub fn is_degraded(&self) -> bool {
        self.edges != EdgeDetail::Full
    }
}

/// What the node layer is expected to cost, charged before the edges get a
/// budget.
///
/// Deliberately generous on paths: a node that is not a quad plans a fill and a
/// stroke, and its handles are quads. Quads are not charged against the path
/// budget at all — a quad is a fixed-size instance with no vertex buffer and no
/// render pass (Phase 0 §1.7), which is the whole reason node bodies are quads.
fn node_layer_cost(budgets: &RenderBudgets, load: SceneLoad, plan: &LodPlan) -> (u32, u32) {
    // Two paths per non-quad node, and the pessimistic assumption that every
    // visible node is one. Under `degrade_curves` they are all quads instead.
    let paths_per_node = if plan.degrade_curves { 0 } else { 2 };
    let paths = load.visible_nodes.saturating_mul(paths_per_node);

    // A node body is small and its outline is a handful of points; the ellipse
    // is the expensive one at 337 vertices, and that is the one this charges
    // for, so the estimate errs the way a guard has to.
    let vertices = paths.saturating_mul(ELLIPSE_VERTEX_ESTIMATE);

    (
        paths.min(budgets.target_paths_per_frame),
        vertices.min(budgets.target_path_vertices_per_frame),
    )
}

/// The measured vertex count of an `arc_to` ellipse (Phase 0 §1.2), used as the
/// pessimistic per-node-path charge.
const ELLIPSE_VERTEX_ESTIMATE: u32 = 337;

/// The label size for a rung, or `None` when it would be unreadable.
///
/// The world-space nominal size is scaled by the zoom and then **quantised onto
/// [`LodThresholds::font_size_ladder`]**, which is not an optimisation but the
/// only thing that makes text survive a zoom gesture: `font_size` is part of
/// GPUI's shaped-line cache key, so an unquantised size re-shapes every visible
/// label on every frame of a pinch (Phase 0 §1.9).
fn label_font_size(lod: &LodThresholds, detail: DetailLevel, zoom: f32) -> Option<f32> {
    if detail == DetailLevel::Overview {
        // §15: do not lay out rich text that cannot be read. Not "lay it out
        // and skip painting it" — the shaping is the cost.
        return None;
    }

    let rendered = lod.nominal_label_size * zoom;
    if rendered < lod.min_readable_font_px {
        return None;
    }
    Some(lod.quantize_font_size(rendered))
}

/// The dash spec an edge is drawn with at this rung, given the document's.
pub fn dash_for(detail: EdgeDetail, document: Option<DashSpec>) -> Option<DashSpec> {
    if detail.keeps_dashes() {
        document
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::budgets::{RenderBackend, for_backend};

    fn budgets() -> RenderBudgets {
        for_backend(RenderBackend::Metal)
    }

    /// Phase 4's scattered scene, as the ladder sees it: the numbers from
    /// `spatial`'s doc table, fed in directly.
    fn hairball() -> SceneLoad {
        SceneLoad {
            visible_nodes: 36,
            visible_edges: 61_104,
            // The scattered scene's edges span the document; at the bench
            // camera that is most of the pane's diagonal.
            mean_edge_screen_length: 1_200.0,
        }
    }

    /// The large scene: the same document, local edges.
    fn healthy() -> SceneLoad {
        SceneLoad {
            visible_nodes: 36,
            visible_edges: 126,
            mean_edge_screen_length: 180.0,
        }
    }

    #[test]
    fn a_healthy_frame_is_not_degraded_at_all() {
        let plan = LodPlan::choose(&budgets(), 1.0, healthy());

        assert_eq!(plan.edges, EdgeDetail::Full);
        assert_eq!(plan.max_edges, 126, "nothing is skipped");
        assert!(!plan.is_degraded());
        assert_eq!(plan.detail, DetailLevel::Full);
    }

    /// **The property this module exists for.** The scene that put 147 M
    /// estimated vertices and 60,061 dropped paths in front of Phase 4 must
    /// come out inside both budgets.
    #[test]
    fn the_hairball_scene_is_bounded_by_the_ladder() {
        let budgets = budgets();
        let load = hairball();
        let plan = LodPlan::choose(&budgets, 1.0, load);

        assert!(plan.is_degraded(), "61,104 visible edges must degrade");

        let paths = plan.max_edges * plan.edges.paths_per_edge();
        let vertices = plan.max_edges
            * plan
                .edges
                .estimated_vertices(load.mean_edge_screen_length, RenderQuality::BALANCED);

        assert!(
            paths <= budgets.target_paths_per_frame,
            "{paths} paths against a budget of {}",
            budgets.target_paths_per_frame
        );
        assert!(
            vertices <= budgets.target_path_vertices_per_frame,
            "{vertices} vertices against a budget of {}",
            budgets.target_path_vertices_per_frame
        );
        assert!(
            vertices < budgets.safe_path_vertex_ceiling,
            "and nowhere near the black-window guard"
        );
    }

    /// The hairball is bounded by the **path** tax before it is bounded by
    /// vertices — the finding Phase 4's vertices-only table does not show, and
    /// the reason a vertices-only ladder would have looked like it worked.
    #[test]
    fn the_path_count_is_what_binds_first_on_a_hairball() {
        let budgets = budgets();
        let load = hairball();
        let plan = LodPlan::choose(&budgets, 1.0, load);

        assert!(
            plan.max_edges < load.visible_edges,
            "the count itself has to be capped: even one path per edge is \
             {} paths against a budget of {}",
            load.visible_edges,
            budgets.target_paths_per_frame
        );
        assert_eq!(
            plan.edges,
            EdgeDetail::Hairline,
            "the ladder must have walked all the way down first"
        );
    }

    #[test]
    fn the_ladder_walks_down_one_rung_at_a_time_rather_than_jumping() {
        let budgets = budgets();
        // Sized so `Full` misses and `Coarse` fits: enough edges that curve
        // tessellation is too expensive, few enough that the count is fine.
        let mut edges = 1;
        let load = loop {
            let load = SceneLoad {
                visible_nodes: 20,
                visible_edges: edges,
                mean_edge_screen_length: 400.0,
            };
            if LodPlan::choose(&budgets, 1.0, load).edges == EdgeDetail::Coarse {
                break load;
            }
            edges += 1;
            assert!(edges < 20_000, "no load produced a Coarse frame");
        };

        let plan = LodPlan::choose(&budgets, 1.0, load);
        assert_eq!(plan.edges, EdgeDetail::Coarse);
        assert_eq!(plan.max_edges, load.visible_edges, "nothing was skipped");
    }

    #[test]
    fn each_rung_really_is_cheaper_than_the_one_above_it() {
        let quality = RenderQuality::BALANCED;
        let mut previous = u32::MAX;

        for rung in EdgeDetail::LADDER {
            let vertices = rung.estimated_vertices(600.0, quality);
            assert!(
                vertices < previous,
                "{rung:?} costs {vertices} against {previous} for the rung above"
            );
            previous = vertices;
        }
    }

    #[test]
    fn markers_and_dashes_are_dropped_before_the_line_is() {
        assert!(EdgeDetail::Full.keeps_dashes());
        assert!(
            !EdgeDetail::Coarse.keeps_dashes(),
            "a dash is 63x a solid line"
        );
        assert!(EdgeDetail::Coarse.keeps_markers());
        assert!(!EdgeDetail::Polyline.keeps_markers());
        assert_eq!(EdgeDetail::Polyline.paths_per_edge(), 1);
        assert_eq!(EdgeDetail::Full.paths_per_edge(), 3);
    }

    #[test]
    fn a_coarser_rung_never_upgrades_a_documents_own_quality() {
        let draft = RenderQuality::DRAFT;

        for rung in EdgeDetail::LADDER {
            assert!(
                rung.quality(draft).flattening_tolerance >= draft.flattening_tolerance,
                "{rung:?} tessellated finer than the document asked for"
            );
        }
    }

    #[test]
    fn the_zoom_ladder_is_the_one_the_requirements_describe() {
        let budgets = budgets();
        let load = healthy();

        assert_eq!(
            LodPlan::choose(&budgets, 1.0, load).detail,
            DetailLevel::Full
        );
        assert_eq!(
            LodPlan::choose(&budgets, 0.4, load).detail,
            DetailLevel::Compact
        );
        assert_eq!(
            LodPlan::choose(&budgets, 0.1, load).detail,
            DetailLevel::Overview
        );
    }

    /// §16, as a number: only the full rung creates elements, and even there
    /// the count is a ceiling rather than "however many are visible".
    #[test]
    fn rich_elements_exist_only_at_full_detail() {
        let budgets = budgets();
        let load = healthy();

        assert_eq!(
            LodPlan::choose(&budgets, 1.0, load).max_rich_nodes,
            budgets.max_rich_elements
        );
        assert_eq!(LodPlan::choose(&budgets, 0.4, load).max_rich_nodes, 0);
        assert_eq!(LodPlan::choose(&budgets, 0.1, load).max_rich_nodes, 0);
    }

    #[test]
    fn handles_degrade_from_elements_to_dots_to_nothing() {
        let budgets = budgets();
        let load = healthy();

        assert_eq!(
            LodPlan::choose(&budgets, 1.0, load).handles,
            HandleDetail::Interactive
        );
        assert_eq!(
            LodPlan::choose(&budgets, 0.4, load).handles,
            HandleDetail::Painted
        );
        assert_eq!(
            LodPlan::choose(&budgets, 0.1, load).handles,
            HandleDetail::Hidden
        );
    }

    /// §15's first bullet, as a test: below readable zoom there is no font size
    /// at all, so nothing downstream can shape a line.
    #[test]
    fn an_unreadable_label_has_no_size_rather_than_a_tiny_one() {
        let budgets = budgets();
        let load = healthy();

        assert!(
            LodPlan::choose(&budgets, 1.0, load)
                .label_font_size
                .is_some()
        );
        assert_eq!(
            LodPlan::choose(&budgets, 0.1, load).label_font_size,
            None,
            "overview draws boxes"
        );
        assert_eq!(
            LodPlan::choose(&budgets, 0.25, load).label_font_size,
            None,
            "13 world units at 0.25 zoom is 3.25 px, under the readable floor"
        );
    }

    /// The whole reason the ladder quantises: a continuous zoom must not
    /// produce a new shaped line per frame.
    #[test]
    fn sweeping_the_zoom_produces_only_a_handful_of_label_sizes() {
        let budgets = budgets();
        let load = healthy();
        let mut sizes: Vec<f32> = (1..=400)
            .filter_map(|step| LodPlan::choose(&budgets, step as f32 / 100.0, load).label_font_size)
            .collect();
        sizes.sort_by(f32::total_cmp);
        sizes.dedup();

        assert!(
            sizes.len() <= budgets.lod.font_size_ladder.len(),
            "400 zoom steps produced {} distinct label sizes",
            sizes.len()
        );
    }

    /// The sample is what keeps the decision O(1); this is the property that
    /// says it is also stable.
    #[test]
    fn the_load_sample_is_deterministic_and_bounded() {
        let stride = 61_104usize.div_ceil(LOAD_SAMPLE);
        let sampled = (0..61_104usize).step_by(stride).count();

        assert!(
            sampled <= LOAD_SAMPLE,
            "{sampled} edges sampled, over the {LOAD_SAMPLE} cap"
        );
    }

    #[test]
    fn an_empty_scene_decides_without_dividing_by_zero() {
        let plan = LodPlan::choose(&budgets(), 1.0, SceneLoad::default());

        assert_eq!(plan.edges, EdgeDetail::Full);
        assert_eq!(plan.max_edges, 0);
    }

    /// Zoom drives legibility and load drives survival, and they must not be
    /// the same lever: a sparse diagram at any zoom keeps its full edges.
    #[test]
    fn a_sparse_scene_keeps_full_edges_at_every_zoom() {
        let budgets = budgets();
        let sparse = SceneLoad {
            visible_nodes: 8,
            visible_edges: 12,
            mean_edge_screen_length: 200.0,
        };

        for step in 4..=200 {
            let zoom = step as f32 / 100.0;
            let plan = LodPlan::choose(&budgets, zoom, sparse);
            let expected = if plan.degrade_curves {
                EdgeDetail::Polyline
            } else {
                EdgeDetail::Full
            };
            assert_eq!(plan.edges, expected, "at zoom {zoom}");
        }
    }
}
