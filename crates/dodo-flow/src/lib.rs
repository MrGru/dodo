//! The Flow Canvas: an infinite canvas and node-graph engine — React-Flow-style
//! graphs, Excalidraw-style drawing and a hand-drawn render mode — built
//! directly on GPUI, with no WebView and no foreign UI framework.
//!
//! **This is the first slice of eight, and nothing here reaches the running app
//! yet** — by design. The canvas is built in phases, each one buildable,
//! testable and reviewable on its own, and the sidebar row is deliberately the
//! last of them so nobody meets a half-built tool. "What is here after slice
//! one" below says exactly where that leaves the crate. Until the row lands,
//! the canvas runs through its own launcher:
//!
//! ```sh
//! cargo run -p dodo-flow --example flow --locked
//! ```
//!
//! # Why a crate rather than a module
//!
//! The requirements document says to prefer a module unless there is a strong
//! architectural reason. **dodo's own convention is the reason**: eight app
//! features already live as feature crates, each with a standalone launcher,
//! and `docs/architecture/workspace-layout.md` governs the shape. This is the
//! first that was born a crate rather than lifted out of the binary, and it
//! qualifies on the test that doc states — the seam, not the size. Its outbound
//! edges are one kernel crate (`dodo-paths`, for `HostOs`), the UI framework
//! and serde; its inbound surface, when the sidebar row lands, is one `tools!`
//! row.
//!
//! The practical payoff is the second one that doc names: **a feature crate
//! earns a launcher**, and a canvas engine is unusually expensive to develop
//! through the whole app. `examples/flow.rs` opens the canvas alone in about a
//! second.
//!
//! # The boundary that matters: almost no UI framework outside `views/`
//!
//! ```text
//! views/                gpui, gpui-component     <- may name a UI framework
//! render/painter.rs     gpui  (the only painter) <- may name a UI framework
//! ────────────────────────────────────────────
//! render/plan.rs        the paint-order contract
//! render/shapes.rs      the shapes as outlines
//! render/edges.rs       routes and markers as primitives
//! render/grid.rs        the viewport-generated grid
//! interaction/state.rs  the interaction state machine
//! runtime/              the graph engine: stores,  no UI framework, no `App`,
//!                       adjacency, dirty, routes   no window, unit tested
//! models/               the document, serde
//! geometry/             world<->screen, routing
//! budgets.rs            the platform's ceilings
//! ```
//!
//! Everything on the lower side is plain Rust and plain `f32`. GPUI's
//! `Point<Pixels>` appears at the render boundary and nowhere else.
//!
//! Note that the line is drawn **per file, not per directory**. `render/` and
//! `interaction/` each hold one GPUI file and the rest pure, which is what lets
//! the paint-order contract, the grid's bounded output, the vertex estimate and
//! every interaction transition be asserted by ordinary unit tests with no
//! window anywhere.
//!
//! **This is worth more than it looks and it is very hard to recover once
//! lost.** A `Point<Pixels>` in a document struct means the document can only
//! be built inside an `App`; a `Bounds<Pixels>` in the culling predicate means
//! the culling test needs a window; an `Hsla` in a style means a style cannot
//! be asserted without a theme. Each is individually harmless and together they
//! are the difference between an engine whose transform maths, bounds maths,
//! serialization and budgets are covered by fast unit tests and one whose
//! correctness is only observable by looking at it. dodo already holds this
//! line elsewhere — 90 of `dodo-cleaner`'s 93 files name no UI framework, 27 of
//! `dodo-docker`'s 43 — and it holds harder here, because a canvas engine's
//! bugs are geometric and a geometric bug that can only be seen is a geometric
//! bug that ships.
//!
//! The line is enforced by what the modules import, and `models/` restates it
//! at each boundary: [`models::ids`] makes the runtime indices unserializable
//! so no cache can reach the document format, and
//! [`models::style`]'s colours are `Option<Color>` so no theme can be baked
//! into a file.
//!
//! # What is here after the first slice
//!
//! - [`models`] — [`ElementId`] and the compact `u32`
//!   runtime indices, the [`ElementKind`] taxonomy, the
//!   shared style structs, [`FlowDocument`], and the
//!   versioned format with its migration ladder in place from version 1.
//! - [`geometry`] — [`Vec2`], [`Rect`] and
//!   **[`Viewport`], the single owner of world↔screen**,
//!   cursor-anchored zoom included.
//! - [`budgets`] — the per-platform render ceilings and LOD thresholds, in one
//!   named place.
//! - [`views`] — [`FlowView`], an empty themed pane.
//!
//! # What the second slice added
//!
//! - [`render`] — **the two contracts Phase 0 made structural**:
//!   [`render::plan`] batches paint by primitive kind so interleaving is not
//!   expressible, and counts the vertices actually painted;
//!   [`render::shapes`] turns the four canvas shapes into outlines and decides
//!   which of them should be a quad instead; [`render::grid`] generates the
//!   background from the viewport with a bounded primitive count at any zoom;
//!   [`render::painter`] is the one file that paints.
//! - [`interaction`] — [`InteractionMachine`], §25's explicit state, with
//!   `Idle`, `Panning` and `BoxSelecting` and pure transitions.
//! - [`views`] — pan, zoom and box selection wired to real input, repainting on
//!   change and never on a clock.
//!
//! # What the third slice added: the graph engine
//!
//! - [`runtime`] — §17's SoA stores, §20's adjacency index, §19's dirty
//!   tracking, §4's handles and connection rules, §29's narrow-phase hit test,
//!   and [`runtime::GraphWorld`] holding them together.
//! - [`geometry::route`] — §8's five routings as derived world-space geometry;
//!   [`geometry::arrow`] — its endpoint decorations.
//! - [`render::edges`] — the one crossing from a world-space route to
//!   screen-space primitives, markers included.
//! - [`interaction`] — `DraggingNode` and `Connecting`, so one press means a
//!   pan, a box selection, a node drag or a connection depending on what it
//!   landed on.
//!
//! **The rule the whole engine exists for**, from §19: moving a node updates
//! that node, marks its render transform dirty, queues one spatial update,
//! finds its incident edges *through the adjacency index* and marks only those
//! edge geometries. `runtime::world`'s
//! `moving_one_node_in_a_huge_graph_rebuilds_only_its_own_edges` asserts it on
//! 100,000 nodes and 500,000 edges; measured on the M1, that move costs **0.17
//! µs against 0.18 µs in a graph a hundred times smaller**, which is the claim
//! stated as a number rather than as a diagram. `examples/flow_graph_bench.rs`
//! prints it, and answers §20's "benchmark the representation" while it is
//! there.
//!
//! Still to come, in roughly this order: `spatial/` (the uniform grid and its
//! viewport, box-select and hit-test queries), the tessellation cache,
//! `commands/` and `components/`, then the sidebar row and its translated
//! strings. Nothing is stubbed for them here; the seams are the module
//! boundaries.
//!
//! **Three absences are deliberate and load-bearing.** There is no culling —
//! §40 rule 1 forbids scanning every element to find the visible ones, and a
//! linear scan written here to fake it would be the thing nobody remembered to
//! delete once `spatial/` arrived. A committed box selection yields a world
//! rectangle and stops, for the same reason: resolving it into elements is the
//! spatial index's broad phase (§28). And **nothing is ever removed** from a
//! store — an index is a slot number, so removal is either a tombstone or a
//! swap-remove, and which one is right is decided by the undo history (§30)
//! that has to restore it, which is Phase 7's.
//!
//! The hit test is the one place this could have been fudged and was not:
//! [`runtime::GraphWorld::hit_test`] takes its candidate set **as an
//! argument**, so today's launcher passes every node and Phase 4 passes a
//! spatial query. There is nothing to delete, only an argument to change.
//!
//! # Where the budget numbers come from, and what they are not
//!
//! [`budgets`] is the one named place for every render ceiling and LOD
//! threshold. Its numbers were **measured against `gpui_macos`'s Metal renderer
//! on an Apple M1 laptop on 2026-08-16**, in a 1440×900 logical window at scale
//! factor 2, in dodo's shipping release profile. They are not portable and the
//! module does not pretend they are: `gpui_windows`, `gpui_linux` and
//! `gpui_wgpu` are separate implementations, two of dodo's four release targets
//! cannot be built on macOS at all, and those backends' rows are marked
//! [`Unmeasured`](budgets::Provenance::Unmeasured) and discounted so they fail
//! slow rather than fail black. Every constant there records the scene it came
//! from, so each can be re-measured on other hardware.
//!
//! One number reshapes the whole design and belongs in this doc rather than
//! only in that module: **macOS stops rendering entirely past ~2.58 M path
//! vertices in a frame, and the window goes solid black** — no panic, no
//! warning, because dodo installs no logger and the renderer's `log::error!`
//! goes nowhere. Culling is therefore a correctness requirement rather than an
//! optimisation, GPUI's content-mask clipping does not substitute for it, and
//! the renderer counts the vertices it is about to paint. That is why
//! [`budgets`] exists as a module instead of as a handful of constants next to
//! the code that spends them.
//!
//! Three more structural decisions come from the same measurements rather than
//! from taste: flattening tolerance is a first-class style field
//! ([`models::RenderQuality`]) because it is a 2× budget
//! multiplier and part of every geometry cache key; every axis-aligned
//! rectangle is a quad rather than a filled path; and paths are painted as one
//! contiguous run, never interleaved with quads or text, because each
//! contiguous run is a full-viewport render pass.

// Only the files under `views/` may name a UI framework. See the crate doc for
// what that boundary is worth; `Cargo.toml` says why each dependency is here.
pub mod budgets;
pub mod geometry;
pub mod interaction;
pub mod models;
pub mod render;
pub mod runtime;
pub mod views;

pub use budgets::RenderBudgets;
pub use geometry::{Rect, Vec2, Viewport};
pub use interaction::{InteractionEffect, InteractionEvent, InteractionMachine, InteractionState};
pub use models::{ElementId, ElementKind, FlowDocument};
pub use render::{GridSettings, GridStyle, PaintPlan, PaintStats};
pub use runtime::{ConnectionRules, EdgeEnd, GraphWorld, NodeSpec, PointerTarget};
pub use views::FlowView;

#[cfg(test)]
mod tests {
    /// Every file below `views/`, with its source. Explicit rather than walked,
    /// for the same reason `src/i18n_lint.rs` lists dodo's view files by hand:
    /// `include_str!` is compile-time, so a file that is added and forgotten
    /// here is a visible omission in a diff, where a directory walk would
    /// quietly cover — or quietly stop covering — whatever happened to be on
    /// disk when the test ran.
    const PURE_FILES: &[(&str, &str)] = &[
        ("budgets.rs", include_str!("budgets.rs")),
        ("geometry/mod.rs", include_str!("geometry/mod.rs")),
        ("geometry/arrow.rs", include_str!("geometry/arrow.rs")),
        ("geometry/bounds.rs", include_str!("geometry/bounds.rs")),
        ("geometry/route.rs", include_str!("geometry/route.rs")),
        (
            "geometry/transform.rs",
            include_str!("geometry/transform.rs"),
        ),
        ("geometry/vec.rs", include_str!("geometry/vec.rs")),
        ("models/mod.rs", include_str!("models/mod.rs")),
        ("models/document.rs", include_str!("models/document.rs")),
        ("models/ids.rs", include_str!("models/ids.rs")),
        ("models/kind.rs", include_str!("models/kind.rs")),
        (
            "models/serialization.rs",
            include_str!("models/serialization.rs"),
        ),
        ("models/style.rs", include_str!("models/style.rs")),
        ("render/edges.rs", include_str!("render/edges.rs")),
        ("render/grid.rs", include_str!("render/grid.rs")),
        ("render/mod.rs", include_str!("render/mod.rs")),
        ("render/plan.rs", include_str!("render/plan.rs")),
        ("render/shapes.rs", include_str!("render/shapes.rs")),
        ("interaction/mod.rs", include_str!("interaction/mod.rs")),
        ("interaction/state.rs", include_str!("interaction/state.rs")),
        ("runtime/mod.rs", include_str!("runtime/mod.rs")),
        ("runtime/adjacency.rs", include_str!("runtime/adjacency.rs")),
        ("runtime/compact.rs", include_str!("runtime/compact.rs")),
        (
            "runtime/connection.rs",
            include_str!("runtime/connection.rs"),
        ),
        ("runtime/dirty.rs", include_str!("runtime/dirty.rs")),
        ("runtime/edges.rs", include_str!("runtime/edges.rs")),
        ("runtime/handles.rs", include_str!("runtime/handles.rs")),
        ("runtime/hit.rs", include_str!("runtime/hit.rs")),
        ("runtime/nodes.rs", include_str!("runtime/nodes.rs")),
        ("runtime/routes.rs", include_str!("runtime/routes.rs")),
        ("runtime/world.rs", include_str!("runtime/world.rs")),
    ];

    /// **The crate's central invariant, enforced rather than remembered.**
    ///
    /// The document, geometry and budget layers name no UI framework, so they
    /// are unit tested with no `App` and no window. The crate doc explains what
    /// that is worth and why it is very hard to recover once lost; this is the
    /// tripwire that notices the first `use gpui::…` to cross the line, while
    /// removing it is still a one-line edit.
    ///
    /// Prose may name gpui freely — several of these files explain *why* a type
    /// is not gpui's — so comment lines are skipped and only code is scanned.
    #[test]
    fn the_pure_layers_name_no_ui_framework() {
        for (path, source) in PURE_FILES {
            for (number, line) in source.lines().enumerate() {
                let code = line.trim_start();
                if code.starts_with("//") {
                    continue;
                }

                assert!(
                    !code.contains("gpui"),
                    "{path}:{} names a UI framework in code, which the crate doc \
                     forbids below `views/`:\n  {code}",
                    number + 1
                );
            }
        }
    }
}
