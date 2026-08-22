//! The Flow Canvas: an infinite canvas and node-graph engine — React-Flow-style
//! graphs, Excalidraw-style drawing and a hand-drawn render mode — built
//! directly on GPUI, with no WebView and no foreign UI framework.
//!
//! The canvas was built in phases, each one buildable, testable and reviewable
//! on its own, and the sidebar row (Phase 8) was deliberately held until last
//! so nobody met a half-built tool. The captain reviewed the running canvas
//! after Phase 7.5 and specified a second scope, so Phases 9 to 12 — editing,
//! text, the property panel and images — landed before Phase 8 wired the
//! finished Diagram tool into dodo. The per-slice sections below are the record,
//! in order. The standalone launcher remains available for focused work:
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
//! views/flow.rs         gpui, gpui-component     <- may name a UI framework
//! views/nodes.rs        gpui  (the rich elements)<- may name a UI framework
//! views/keymap.rs       gpui  (the bindings)      <- may name a UI framework
//! render/painter.rs     gpui  (the only painter) <- may name a UI framework
//! ────────────────────────────────────────────
//! commands/             §30's deltas, the applier, the
//!                       history, the gesture mapping
//! render/plan.rs        paint order, and the clip
//! render/lod.rs         §15's ladder: what to simplify
//! render/snapshot.rs    §24's extraction: compact indices only
//! render/registry.rs    §43's node renderer registry
//! render/cache.rs       §23's caches, generic over what they hold
//! render/scene.rs       the snapshot -> primitives
//! render/shapes.rs      the shapes as outlines
//! render/sketch.rs      §13's hand, as perturbed outlines
//! render/edges.rs       routes and markers as primitives
//! render/grid.rs        the viewport-generated grid
//! interaction/state.rs  the interaction state machine
//! spatial/              the uniform grid and its    no UI framework, no `App`,
//!                       viewport / hit / box query  no window, unit tested
//! runtime/              the graph engine: stores,
//!                       adjacency, dirty, routes,
//!                       selection
//! models/               the document, serde
//! geometry/             world<->screen, routing, curves
//! scenes.rs             the benchmark scenes
//! instrument.rs         the performance probes
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
//! *(The `render/` list above is Phase 2's; `render::cache`, `render::lod`,
//! `render::registry`, `render::scene` and `render::snapshot` joined it later,
//! and `views::nodes` with them. All of them except the last name no UI
//! framework.)*
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
//! # What the fourth slice added: the gate
//!
//! Phase 4 is the phase that proves the three before it, and it was allowed to
//! return bad news. It did not have to.
//!
//! - [`spatial`] — §21's uniform hash grid, [`SpatialIndex`] over it, and the
//!   [`VisibleSet`] every frame is planned from. **Read its module doc for the
//!   numbers**; they are the ones that gate every later phase.
//! - [`render::scene`] — the node, handle and edge planning loops, moved out of
//!   `views::flow` so that "no offscreen path reaches the painter" is an
//!   ordinary unit test rather than something you check by looking.
//! - [`render::plan`] — the clip became part of the plan: `clear` takes the
//!   pane and `push_path` refuses anything outside it, so a painter cannot
//!   express the mistake that Phase 0 measured at 6.3 ms a frame.
//! - [`runtime::selection`] — §28's set: a bitset for "is this selected?" and
//!   a list for "what is selected?", holding compact ids and never elements.
//! - [`instrument`] — §39's ten named probes, off unless
//!   `DODO_FLOW_INSTRUMENT` is set.
//! - [`scenes`] — §38's four scenes, in the library rather than in the example
//!   so a test and the harness measure the same thing.
//!
//! **The two seams Phase 3 left open are closed, each by changing one thing.**
//! [`runtime::GraphWorld::hit_test`] took its candidate set as an argument and
//! now gets a grid query instead of `nodes().indices()`; a committed box
//! selection yielded a world rectangle and now flows through the broad phase
//! into [`runtime::GraphWorld::apply_box_selection`]. Neither had a placeholder
//! to delete.
//!
//! ## The numbers, in one place
//!
//! Apple M1, release, 1440×900, 2026-08-19, from
//! `cargo run --release -p dodo-flow --example flow_scene_bench --locked`:
//!
//! | | |
//! |---|---|
//! | viewport query at 100,000 nodes / 300,000 edges | **2.3 µs**, against 1,863 µs for the same question by scan |
//! | the same query at 5,000 nodes | 2.3 µs — **it does not grow with the document** |
//! | tessellating the dense frame from scratch | 3.08 ms, 19 % of a 16.7 ms frame — the case for §23's cache |
//! | one node moved in a 300,000-edge graph | **0.51 µs** (Phase 3's 0.17 µs propagation, plus the index) |
//! | painted vertices, worst scene | **132,888** — 5.5 % of the 2.4 M ceiling, 38 % of the 60 fps budget |
//! | path batches per frame | **1**, against a budget of 64 |
//! | offscreen paths reaching the painter | **0**, by construction |
//! | paths dropped by the black-window guard | **0** on all four scenes |
//! | pure pan: edge-route cache misses | **0**; spatial updates **0** |
//! | memory at 100,000 nodes / 300,000 edges | 128.8 MB runtime + 23.9 MB index, 205 MB resident |
//!
//! ## And the three pieces of bad news, because they matter more
//!
//! 1. **Culling does not bound a hairball graph.** Every number above assumes
//!    edges join nodes that are near each other, which real diagrams do. A
//!    100,000-node document whose edges connect anything to anything puts
//!    61,104 edges in the viewport, estimates 147 M vertices, and loses 60,061
//!    paths to the black-window guard. Nothing is wrong with the index — an
//!    edge that spans the document genuinely *is* visible — and the failure is
//!    graceful rather than black. But it means **LOD (§15) is required, not
//!    optional**, and [`spatial`]'s doc carries the table.
//! 2. **There is no geometry cache yet, and the dense scene wants one.**
//!    Tessellating that frame from scratch costs **3.12 ms**, 19 % of a 16.7 ms
//!    frame, for geometry that did not change. Its whole visible set would be
//!    4.25 MB against [`budgets`]'s 64 MiB bound, so §23's cache both fits and
//!    pays. It is deferred, not forgotten.
//! 3. **Phase 1's request to re-fit the paint cost model could not be met.**
//!    `predicted_paint_micros` described `Window::paint_path`, which needs a
//!    real window; the harness is headless. What it *could* measure —
//!    tessellation — is a different cost and is recorded separately. The re-fit
//!    needs a windowed spike, and Phase 5 is the first phase that has a window
//!    to hand. *(Phase 5's answer is in [`budgets`]'s module doc: a window was
//!    not enough, the helper is deleted, and the harness is committed for a
//!    human to run.)*
//!
//! # What the fifth slice added: the hybrid renderer
//!
//! Phase 4 proved the architecture and handed forward three things. Phase 5 is
//! where the canvas stops being only pixels.
//!
//! - [`render::lod`] — §15's ladder, and **the only thing that bounds a
//!   hairball**. It spends *two* budgets: the vertex one, and
//!   [`budgets::RenderBudgets::target_paths_per_frame`], which is new and which
//!   the hairball reaches first.
//! - [`render::snapshot`] — §24's extraction. Compact indices and screen
//!   rectangles, never cloned metadata and never colours; the boundary a later
//!   phase can compute in the background.
//! - [`render::registry`] — §43's node renderer registry, plus six generic
//!   kinds, three of which register through the same public path a third party
//!   would use.
//! - [`render::cache`] — §23's geometry and shaped-line caches, byte-bounded,
//!   viewport-scoped, generic over what they hold so every property is asserted
//!   with no window.
//! - [`views::nodes`] — the rich half: node elements, interactive handles for
//!   the selected-or-hovered node and a selection ring. The contextual property
//!   panel added in Phase 11 now supplies §44's detailed selection controls,
//!   rather than duplicating them over the node.
//!
//! ## The numbers, beside Phase 4's
//!
//! Same machine, same command, 2026-08-19:
//!
//! | | Phase 4 | Phase 5 |
//! |---|---:|---:|
//! | **scattered**: edges drawn of 61,104 visible | 61,104 | **5,000** |
//! | **scattered**: estimated vertices | 147,761,694 | **99,960** |
//! | **scattered**: paths dropped by the black-window guard | 60,061 | **0** |
//! | **dense**: painted vertices | 132,888 | **17,916** |
//! | pure pan: geometry cache hit rate | *(no cache)* | **99.2 %**, all exact translations |
//! | pure pan: tessellations over 60 frames | 179,160 | **4,286** |
//! | geometry cache held | — | **0.60 MB** of a 64 MiB bound |
//! | GPUI elements, 100,000-node document | — | **36** |
//!
//! **The black-window guard stopped firing**, which is the result that matters:
//! `enforce_vertex_ceiling` drops geometry blindly from the end of a plan, and
//! a frame that reaches it has already lost. The ladder simplifies while it
//! still knows what the elements are.
//!
//! ## And what Phase 5 got wrong on the way
//!
//! Two mistakes worth the next person's attention, both caught by numbers
//! rather than by review:
//!
//! 1. **The first cost model charged every visible node as an ellipse**, so
//!    Phase 4's dense scene — 1,584 nodes, every one of them a quad — came out
//!    with a vertex budget of zero and drew *none* of its 3,182 edges. A
//!    conservative estimate is not automatically a safe one: being wrong in the
//!    common case starves the layer that was supposed to be protected.
//!    [`render::lod::SceneLoad::path_bodied_fraction`] samples what the nodes
//!    actually cost.
//! 2. **Both caches evicted to exactly their bound**, so every insert past it
//!    re-sorted the whole entry set. A test offering the shaped-line cache four
//!    times its capacity took 22 seconds and took the crate's whole suite with
//!    it — but it is a per-frame pathology on any document with more labels
//!    than the cache holds, not a slow test.
//!    [`render::cache::EVICT_TO_FRACTION`] is the fix and records why.
//!
//! ## The measurement Phase 5 still could not take
//!
//! Phase 4 said the paint-cost re-fit needed a window and that Phase 5 would
//! have one. It did, and that was not enough: an unattended GPUI window on
//! macOS presents its first frame and then stops. `predicted_paint_micros` is
//! **deleted** rather than left as an unvalidated model, its two coefficients
//! stay because they are separately load-bearing, and
//! `examples/flow_paint_fit.rs` is committed ready for a human to run.
//! [`budgets`]'s module doc has the whole decision.
//!
//! # What the sixth slice added: the hand
//!
//! §13's sketch renderer, and the word that matters is **renderer**: switching
//! between [`models::RenderStyle::Clean`] and `Sketch` writes one field on
//! [`models::DocumentSettings`] and asks for a repaint. No element is created,
//! moved or rewritten, no version moves, no route is rebuilt and no spatial
//! update is raised — `runtime::world`'s
//! `switching_render_style_touches_no_element` asserts every one of those, and
//! it is what makes the toggle instant on a 100,000-node document.
//!
//! **What it costs to *draw* and what it costs to *change* are different
//! questions, and only the first one is answered above.** The style is saved
//! with the document and is the author's choice rather than the viewer's, so
//! changing it is an edit: it travels [`commands::EditCommand::SetRenderStyle`]
//! — the one variant that names no element — and lands on the undo stack like
//! any other change. The palette's hand-drawn toggle
//! ([`views::palette`]'s `sketch_button`) is where a user reaches it; the
//! property panel is not, because that panel is a view of the selection and
//! this is a property of the document.
//!
//! - [`render::sketch`] — the generator: a deterministic function from a
//!   canonical [`render::Outline`] to a wobbly one, seeded per element from its
//!   [`ElementId`] and the document's own seed. splitmix64, no clock, no state,
//!   no `rand` — so §49's property (*same element + same seed + same geometry ⇒
//!   the same geometry*) is the signature rather than a soak test, and §40 rule
//!   5's "never fresh random values on repaint" cannot be violated by accident.
//! - [`models::SketchStyle`] — roughness, bowing, stroke count, seed and
//!   jitter, serialized with the document because a diagram drawn by hand is
//!   drawn by hand every time it is opened.
//! - [`render::cache`] — the sketch style is **part of the geometry cache key**,
//!   beside the version and the flattening tolerance, and each stroke pass is
//!   its own [`render::cache::GeometryPart`]. So neither mode can serve the
//!   other's geometry, and switching back finds the old tessellations still
//!   warm.
//! - [`render::lod`] — [`render::lod::LodPlan::sketch`], the hand *this frame*
//!   can afford, degraded to clean by two rules: below `curve_to_quad_zoom`
//!   (a 2 px wobble is not visible, and a sketched outline is nothing but
//!   curves) and above what the node layer's share of the frame can hold.
//!
//! ## The numbers, beside Phase 4's and Phase 5's
//!
//! Same machine, same command, 2026-08-19. **Real tessellations**, not
//! estimates:
//!
//! | | clean | sketch |
//! |---|---:|---:|
//! | rectangle body, painted vertices | **0** — it is a quad | **132** as two stroked paths |
//! | ellipse body, painted vertices | 264 | 312 |
//! | Bézier edge, 200 px | 144 | 168 |
//! | **large** scene: paths / painted vertices | 126 / 19,242 | **324 / 32,790** |
//! | **dense** scene (1,584 bodies) | 2,986 / 17,916 | *identical — the ladder drew it clean* |
//! | pure pan, 60 frames: geometry cache hit rate | 99.2 % | **99.1 %** |
//! | pure pan: tessellations over 60 frames | 195 | **484**, against 19,440 with no cache |
//! | visible bodies a hand fits in one frame | — | **331** |
//! | zoom at which the hand is dropped | — | **0.35** |
//!
//! **The axis-aligned row is the whole finding.** A clean rectangle in this
//! engine is a quad — zero path vertices, no batch — and a sketched one cannot
//! be, so sketch mode moves every rectangular node body from the cheapest
//! primitive the engine has onto two of its most expensive.
//! [`render::sketch`]'s doc has the full table and the two mitigations that
//! were measured rather than assumed.
//!
//! ## And the correction Phase 6 sends back
//!
//! **The vertex estimate is 4.5× the painted reality for hand-drawn geometry,
//! against 1.6× for clean.** A perturbed straight side is a cubic that is
//! nearly straight, and
//! [`geometry::curve::cubic_segments`](crate::geometry::curve::cubic_segments)
//! sizes a curve by its control hull — a length, where flattening cost is a
//! deviation. So the ladder drops the hand at 331 visible bodies where the
//! painted cost would fit about 1,400, and scenes between those numbers are
//! drawn clean for nothing. It costs the dense scene nothing *today*, because
//! the path budget binds there first (3,168 paths against a share of 3,000).
//! That function's doc carries the diagnosis, the numbers and the shape of the
//! fix; it is a re-fit of a Phase 4 formula every recorded estimate in the
//! crate is stated against, which is a phase's work rather than a side effect.
//!
//! # What the seventh slice added: one mutation path
//!
//! §30's commands and undo. The feature is undo; **the deliverable is the
//! shape**, because an edit that changes the world behind the history's back
//! does not fail then — it fails three undos later, with no stack trace and no
//! reproduction.
//!
//! - [`commands::edit`] — the delta vocabulary, and it is *both directions*:
//!   every inverse of an edit is itself an edit, so [`mod@commands::apply`] returns
//!   the command that undoes the one it just made. One enum, one applier, and
//!   no second path that can drift from the first. `Rotate`, `Group` and
//!   `Ungroup` are **left out** rather than stubbed — the engine has no angle
//!   and no hierarchy resolver, and a variant nobody can apply is worse than an
//!   absent one.
//! - [`commands::editor`] — [`FlowEditor`], the world and the history welded
//!   together. **It owns its [`GraphWorld`] privately and never lends `&mut` to
//!   it**, which is the whole enforcement: a caller cannot write the bypass
//!   because there is no reference to write it through. [`views::FlowView`]
//!   holds one of these instead of a world, and `world_mut` is gone.
//! - [`commands::history`] — the stacks, bounded, holding deltas and nothing
//!   else. Two mechanisms, deliberately not one: *merging* folds consecutive
//!   moves of the same nodes so a drag does not grow the stack sixty times a
//!   second, and *gesture grouping* pops a whole gesture as one step so that
//!   edits which cannot merge still undo together.
//! - [`commands::gesture`] — §25's interaction effects as commands, moved out
//!   of `views/flow.rs` so that "a whole drag is one undo step" is a unit test
//!   driving the real state machine rather than something you check by
//!   dragging.
//! - [`commands::keys`] — §26's bindings as a total function of a `HostOs`, so
//!   every platform's answer is asserted from any machine; `views::keymap` is
//!   the four lines that need `gpui`.
//!
//! **The absence Phase 3 left open is closed, and undo is what closed it.**
//! Removal is a **tombstone** — [`runtime::NodeFlags::REMOVED`] — because an
//! undo entry *is* a held index: a swap-remove moves another node into the
//! freed slot and silently repoints every entry already on the stack, and it
//! cannot restore an element at its own index at all. Compaction is a document
//! round-trip and nothing else. [`runtime::NodeStore`]'s module doc has the
//! argument.
//!
//! ## What undo restores, and why almost none of it is written down
//!
//! Undoing a node move restores the node, its spatial-index entry, its incident
//! edges' routes, the dirty flags and the geometry cache — and **not one line
//! in `commands/` deals with any of those**. An undo is an ordinary edit
//! applied through the ordinary mutators, so §19's propagation runs for it
//! exactly as it ran for the edit. `commands`'s own tests assert that at the
//! frame level: after an undo, the visible set, the painted bounds, the routes,
//! the index occupancy **and every primitive the painter would be handed** —
//! collected through [`render::PaintPlan::paint_into`], in paint order — equal
//! the frame before the edit was made.
//!
//! **One thing is deliberately not equal, and it is worth knowing about**: the
//! §23 geometry cache version. A version only ever goes *up*, and the write
//! that undoes an edit is still a write, so after an undo the geometry is
//! identical and its cache key is new. That is the safe direction — a spurious
//! miss costs one tessellation, where a version returning to a value it had
//! held before would serve geometry from a state the element has left. The
//! property test compares the versions separately and asserts exactly that.
//!
//! ## The two corrections Phase 7 sends back
//!
//! 1. **A coalesced translation is not invertible in `f32`.** Sixty two-unit
//!    steps and one hundred-and-twenty-unit step back miss by about two
//!    ten-millionths: sixty additions round sixty times, the subtraction rounds
//!    once. Invisible on screen, and *not* invisible to the frame-equivalence
//!    property above, because the node's painted bounds and therefore its
//!    spatial cell can differ. So [`commands::EditCommand::MoveNodes`] keeps
//!    its delta going forward and its inverse is `SetNodePositions`, absolute —
//!    and the merge rule for the two halves is different on purpose, one
//!    summing and one keeping the earliest. `commands::edit` pins the
//!    arithmetic in a test so nobody simplifies it back.
//! 2. **Every key binding scoped to the canvas was dead, and had been since
//!    Phase 2.** GPUI dispatches a key event down the *focus* path and falls
//!    back to the dispatch tree's **root** when nothing is focused, so a canvas
//!    that never takes focus has its key context, its handlers and its actions
//!    outside the path entirely — `Esc` and the space-to-pan key included.
//!    Nothing reports it. [`views::FlowView`] now focuses on mount and refocuses
//!    on every press. **dodo's own `gpui-component-recipes` skill already says
//!    this**, and says to focus in the constructor; the canvas did not, for five
//!    phases. The transferable part is the failure mode rather than the fact — a
//!    dead binding produces no error, no warning and no wrong behaviour, only an
//!    absence — and it is the second thing the plan's "live input is
//!    source-verified, never observed" risk has cost. The keystrokes themselves
//!    still need a human at the keyboard to confirm.
//!
//! # What the seventh-and-a-half slice added: something to draw with
//!
//! §45's tool system, and the half number is the point. It was folded into
//! "the interaction state machine" when the plan was written, landed nowhere in
//! Phase 2, and was not named in §53's milestone list either. Seven phases
//! later the canvas could pan, zoom, drag, select, connect, simplify, sketch
//! and undo, and **a user still could not create a single element**, because
//! nothing let them say "now I am drawing a rectangle". It was found by opening
//! the window.
//!
//! That is the transferable part, and it is Phase 7's dead-key-binding lesson
//! in a second costume: **nothing failed.** Every test passed, every number
//! held, no warning was printed. A capability was simply absent, and no test in
//! this crate asks what a person can accomplish — they ask whether what exists
//! is correct. The two are not the same question and only one of them was being
//! asked.
//!
//! - [`interaction::tool`] — [`CanvasTool`], and the pure geometry a creation
//!   gesture resolves to: the click-versus-drag threshold in **screen** pixels
//!   (so it means the same at every zoom), the shift constraint, and the
//!   default size a click places. §45's rule — *tool activation drives
//!   interaction state and must not alter the document model* — is a property
//!   of where the type lives rather than a convention to remember.
//! - [`interaction::state`] — §25's `CreatingShape`, in the file's own style:
//!   the whole gesture in one variant, transitions pure, no booleans. Picking a
//!   tool is an [`InteractionEvent::SelectTool`](interaction::InteractionEvent::SelectTool)
//!   through the same total transition function as a mouse press, so there is
//!   no second place the active tool is written and none for it to drift from.
//! - [`commands::gesture`] — creation as `AddNodes`, through Phase 7's one
//!   applier. **The element reaches the document exactly once, on the release**,
//!   which is what makes a created shape undo and redo with no line in
//!   `commands/` knowing a palette exists — and what makes an abandoned
//!   creation cost nothing, because there was never a draft to reverse.
//! - [`commands::keys`] — the eight tool letters, joined to the undo/redo rows
//!   in one table, so [`views::keymap`] is still four lines and one keystroke
//!   cannot be bound to two things without a test saying so.
//! - [`views::palette`] — the strip, and it ships with the canvas rather than
//!   with the launcher, so Phase 8's sidebar row gets it for nothing.
//!
//! ## The palette needs no assets and no strings, and that is why it is drawn
//!
//! Each button paints its own tool's shape through
//! [`render::shapes::outline_for_node`] and
//! [`render::painter::build_path`] — the same two functions that draw the
//! element the button creates. `gpui-component`'s icon set has no square,
//! circle, diamond, pointer or hand, so an icon palette meant eight new SVGs, an
//! `AppIcon` variant each and a new crate dependency; a labelled one meant
//! English literals in a crate whose translations are Phase 8's. Drawing the
//! glyphs costs neither, and it buys a property an icon set cannot have: **a
//! button cannot drift from what it makes**, because there is one outline
//! builder and both call it.
//!
//! ## The correction Phase 7.5 sends back
//!
//! **Two of the tools the brief named could be created and not drawn.**
//! `NodeShape::of` mapped [`ElementKind::Linear`](models::ElementKind::Linear)
//! to `NodeShape::Other`, which [`render::snapshot`] counts as
//! `unsupported_nodes` and skips — so a Line or Arrow button would have added a
//! real, selectable, undoable element that never appeared on screen. That is
//! precisely the failure the brief's own rule about `Text`, `Frame` and `Image`
//! is written to avoid, arriving through a kind that *looked* supported because
//! its `ElementKind` variant had been there since Phase 1.
//!
//! An `ElementKind` existing is not the same as the engine being able to draw
//! it, and only [`runtime::NodeShape::of`] knows the difference.
//! `interaction::tool`'s `every_tool_creates_something_the_renderer_can_draw`
//! is that question asked as a test, so the next tool cannot be added without
//! answering it.
//!
//! Closing it was [`runtime::NodeShape::Line`] and `Arrow` plus
//! [`render::shapes::is_open`] — an open outline is stroked, never filled, and
//! **never degraded to its bounding quad**, since the box a diagonal spans is
//! mostly the part of the canvas it is not covering. Three separate default
//! behaviours in the paint loop were each wrong for it, which is why the
//! question has a named function rather than three inline conditions.
//!
//! ## And two limitations a user meets, recorded where they are caused
//!
//! 1. **An arrow always points from its box's top-left to its bottom-right.** A
//!    node stores an origin and a size, never a pair of endpoints, so a linear
//!    element's direction is its diagonal and dragging one out leftwards still
//!    produces an arrow pointing right. The preview is built from the same
//!    rectangle by the same builder, so what is committed is what was shown —
//!    it is consistent rather than wrong. A genuinely free arrow needs §7's
//!    point list, which is a change to the document model.
//!    [`render::shapes::line`] and [`interaction::tool::creation_rect`] carry it.
//! 2. **A linear element's hit test is its whole bounding box.** The narrow
//!    phase is rectangle containment, which is exact for every closed body and
//!    generous for a diagonal — the empty corners of a long arrow are where a
//!    user reaches for whatever is behind it. [`runtime::hit`]'s module doc has
//!    the diagnosis and the one arm that fixes it.
//!
//! Still to come: the sidebar row. *(Its translated strings were expected to
//! come with it — the palette's labels, tooltips and key hints among them.
//! Phase 9 brought them forward instead, and the section below says why.)*
//!
//! # What the ninth slice added: editing, and the first strings
//!
//! The captain reviewed the running canvas and specified a second scope: make
//! editing behave like Excalidraw. Phase 9 is the first of four and carries the
//! foundations the other three stand on, so the deliverable is again a
//! **shape** — one state model and one command model for selection, the active
//! tool, keyboard actions and document mutations — rather than four
//! independently reasonable pieces of UI.
//!
//! - [`FlowEditor::delete_selection`](commands::FlowEditor::delete_selection) —
//!   `Delete`, `Backspace` and the tool palette's Delete action are the same method, which
//!   reads §28's selection and hands it to §30's `SetPresence`. So a removed
//!   node takes its incident edges with it (the applier records the cascade
//!   rather than the request), the whole removal is one undo press, and **the
//!   method does not change when Phase 10 adds text or Phase 12 adds images**:
//!   a selection is a set of indices and does not know what kind an element is.
//! - [`interaction::InteractionMachine`] — the tool lock lives beside the
//!   active tool, because the one place either is read is the transition that
//!   commits a creation. Draw, finish and land back on Select is the default;
//!   with the lock on the tool stays. Both are the *machine's*, so a palette
//!   cannot disagree with what the next press will do.
//! - [`commands::keys`] — `Delete`, `Backspace` and `q`, joined to the undo,
//!   redo and tool rows in the same table, so every platform's answer is still
//!   asserted from any machine and no keystroke can be bound twice without a
//!   test saying so.
//! - [`views::palette`] — the two actions past a divider, and **tooltips at
//!   last**. The keystroke beside each label is not a string: it is rendered
//!   from the binding table through `Tooltip::action`, so a rebind moves the
//!   hint with it.
//! - `dodo_i18n::flow` — the canvas's own catalogue, English and Vietnamese.
//!
//! ## Why the catalogue arrived four phases early
//!
//! Phase 8 is deliberately last, so writing the palette's labels, the Delete
//! action, the lock and Phase 11's whole property panel as English placeholders
//! would mean **touching every string twice** and four phases in which a bare
//! literal could pass unnoticed — the exact defect
//! `i18n_lint::view_code_draws_no_untranslated_literals` exists to catch, in
//! the window before that guard was watching this crate. It is watching now:
//! `views/flow.rs`, `views/palette.rs` and `views/nodes.rs` joined its
//! `SOURCES`, which is the change that makes the decision stick rather than
//! merely start well.
//!
//! ## Two things that were already true, and one that was not
//!
//! The brief asked for tool activation to enter drawing mode immediately, and
//! it already did — one `SelectTool` and the next press is a creation, because
//! Phase 7.5 put the tool in the transition function rather than in a mode
//! flag. `a_tool_draws_on_the_first_press_after_it_is_picked_up` is that
//! turned into a property so it cannot quietly stop being true. The same goes
//! for the tombstone: removal has been undoable since Phase 7, and this phase
//! only had to reach it.
//!
//! What was **not** already true is smaller and worth the next person's
//! attention: `set_presence` cascades a node's edges, and nothing above it did.
//! A view that had assembled its own removal — "delete the selected nodes" —
//! would have produced a document whose edges point at a node that is not
//! there, which `from_document` drops as corrupt on the next load. That is one
//! more argument for the one-door design and it is why the delete lives on the
//! editor rather than in the handler that has the selection to hand.
//!
//! # What the tenth slice added: text, in the three places it lives
//!
//! §9. The feature is text; **the deliverable is again a vocabulary**, because
//! Phase 11 wires a property panel to whatever is here and a panel is very hard
//! to build against a model that has to change under it. The captain supplied
//! reference screenshots and they fix the words: four discrete sizes, three
//! families, three alignments, and text that carries a stroke colour, an
//! opacity and a place in the layer order.
//!
//! - [`models::FontSize`] — **four steps, not a number**, and the discreteness
//!   is load-bearing rather than a simplification. Phase 5 found `font_size` is
//!   part of GPUI's own shaped-line cache key, so a continuously-sized label is
//!   re-shaped on every frame of a zoom. A continuous field would have had to be
//!   quantised *somewhere*; making the document discrete means there is no
//!   second answer to "what size is this text?". Its world sizes are rungs of
//!   [`budgets::LodThresholds::font_size_ladder`], so at 100 % zoom the
//!   quantiser is the identity — a contract between two modules with nothing
//!   else joining them, and one test holding it.
//! - [`models::FontFamily`] and [`models::TextAlign`] beside it. Alignment is
//!   arithmetic ([`models::TextAlign::offset`]), so it is asserted with no
//!   window; the painter applies it from the run's *shaped* width, because
//!   nothing earlier knows one.
//! - [`render::lod::LodPlan::font_size_for`] — the ladder answers **per
//!   element**, so `S` text stops being laid out at a zoom `XL` text survives.
//! - [`runtime::NodeShape::Text`] — a body that is nothing but its glyphs, with
//!   **no outline at all** rather than an empty one, and its own arm in
//!   `plan_nodes`. The fall-through was not harmless: below full detail every
//!   body is drawn as a quad, so a text element would have grown a solid box
//!   the moment you zoomed out.
//! - Edge labels, at the route's **arc-length** midpoint, derived every frame.
//!   That is what makes requirement 5 fall out rather than need machinery:
//!   §19's propagation rebuilds a route when an endpoint moves, so the label
//!   has already moved and nothing in `render/` knows a node was dragged.
//! - [`runtime::PointerTarget::Edge`] and an edge narrow phase, ranked below
//!   bodies and handles. Added for exactly one gesture — labelling an edge
//!   needs to know *which* — and `runtime::hit`'s doc had predicted the shape
//!   of it four phases earlier.
//! - [`interaction::TextTarget`], [`commands::FlowEditor::commit_text`] and
//!   `text_of` — the one door, and the reason existing text is **editable**
//!   rather than merely replaceable: an editor is seeded from `text_of`, and a
//!   blank one silently replaces a sentence the second time anybody opens it.
//! - Document format **version 2**, and the migration ladder's first real rung.
//!   A version-1 `font.size` is a float where version 2 wants one of four
//!   names, and that is the migration case `#[serde(default)]` cannot cover —
//!   the field is present with the wrong shape, so one stale font would have
//!   refused the whole file. Nine phases of empty machinery cost three lines to
//!   use.
//!
//! ## The Text tool is the one that does not create on release
//!
//! Every other tool commits its element on the mouse-up. [`CanvasTool::Text`](interaction::CanvasTool::Text)
//! opens a caret over the rectangle it drew, and the element is added when — and
//! only when — non-empty text is committed. That is not a special case bolted
//! on: **an empty text element is invisible**, because a text element is its
//! glyphs. Creating one on release would leave a selectable, undoable,
//! unpaintable thing on the canvas every time somebody pressed `Esc`, which is
//! precisely the failure Phase 7.5 caught for Line and Arrow arriving from the
//! other direction. The same rule runs backwards: emptying an existing text
//! element removes it, and one undo brings both the element and its words back.
//!
//! ## The two things the tests found, which review would not have
//!
//! 1. **A moved node was re-shaping its label sixty times a second.** The
//!    shaped-line key used [`runtime::NodeStore::version`], which bumps on
//!    *every* write including a position — so dragging a labelled node paid
//!    7–11 µs of shaping per frame for glyphs that had not changed a character.
//!    [`runtime::NodeStore::text_version`] is the fix: a second, narrower
//!    version bumped by the label, the style and a resize (which changes the
//!    width the run wraps into) and by nothing else. Four bytes a node, 400 KB
//!    at a hundred thousand. It was found by writing "a move must not change
//!    the key" as an assertion and watching it fail.
//! 2. **A one-line text element was never "detailed" enough to be drawn at
//!    all.** [`budgets::LodThresholds::min_detailed_node_px`] is 24 px and asks
//!    whether a *body* has room for a border and a line of label **inside** it.
//!    A standalone text element has neither and is exactly one line tall — 22
//!    world units by default — so it failed that gate at 100 % zoom and the
//!    whole kind was invisible. The gate does not apply to it: its only
//!    legibility question is whether its glyphs read, and the ladder already
//!    answers that. A threshold written for one kind silently excluded a kind
//!    that did not exist when it was written, which is worth remembering before
//!    Phase 12 adds images.
//!
//! A third, smaller: [`commands::FlowEditor::text_of`] read *presence* rather
//! than liveness, so a tombstoned element still handed back its words and a
//! caret could have opened on something already deleted.
//!
//! ## §40 rule 7, extended to the third cache
//!
//! Phase 4 asserted that a pure pan rebuilds no routes and Phase 5 that it
//! re-tessellates almost nothing. [`render::cache::TextKey`] carries the owner,
//! the element's text version and the quantised size, and **deliberately not
//! the position** — so `a_pure_pan_shapes_every_label_once_and_never_again`
//! drives sixty panned frames through a real cache and counts three misses,
//! one per label, ever.
//!
//! ## And the limitations a user meets, recorded where they are caused
//!
//! 1. ~~**An edge cannot be selected by clicking it.**~~ *Closed by Phase 10.5.*
//! 2. ~~**Text is a single line.**~~ *Closed by Phase 10.5, and the reason
//!    given for the first of its three changes was wrong — see below.*
//! 3. **Choosing Hand-drawn may change nothing on screen.** dodo ships no
//!    hand-drawn face — a bundled font is a licence, a build step and half a
//!    megabyte in every release artefact — so `views::flow` probes the text
//!    system for the platform's candidates and falls back to the theme's UI
//!    font. [`models::FontFamily::preferred_faces`] carries the list and the
//!    argument. It is probed rather than merely named because GPUI's
//!    `resolve_font` silently substitutes a fallback for a name it cannot find:
//!    a family that is only named would draw in something arbitrary with
//!    nothing to say so.
//!
//! ## What was verified by running, and what was not
//!
//! Everything above is asserted by `cargo test` with no window: the model, the
//! ladder, the plan a frame hands its painter, the interaction transitions, the
//! commands and the round trip. **The keystrokes and the caret are not.** An
//! unattended GPUI window on macOS presents its first frame and then stops, so
//! double-clicking a node, typing into it, re-opening it and watching a label
//! ride a dragged edge are a human's to check — `examples/flow.rs` opens onto a
//! text row put there for exactly that pass, and its doc lists the four things
//! to look at. This is the third phase to hand that forward, and it is the same
//! risk the plan named as "live input is source-verified, never observed".
//!
//! # What the tenth-and-a-half slice added: the two limitations, closed
//!
//! Both were recorded honestly at the line that caused them, and both turned
//! out to be one arm plus its supporting cast rather than a phase's work. The
//! half number says so.
//!
//! ## An edge is clickable
//!
//! [`interaction::InteractionEffect::SelectEdge`],
//! raised from `Idle` and **leaving the machine in `Idle`**. An edge has no
//! drag gesture, so the moves after such a press mean nothing and `Idle`
//! already answers every event with `None`; a state entered only to ignore
//! things is a state to keep correct for no behaviour.
//!
//! Phase 10 argued the gesture should wait for whatever makes an edge
//! *draggable*, so the two would be designed together. **That was wrong, and
//! the shape of the mistake is worth more than the fix.** Selecting is a press
//! that ends where it started; dragging is what the *following* moves mean.
//! They share a target and nothing else, and treating "these two features touch
//! the same type" as "these two features are one design" cost a user the only
//! way to delete an edge for a whole phase. [`runtime::hit`]'s doc carries it.
//!
//! Two things fell out of doing it:
//!
//! - **Shift did not extend the selection for nodes either.** The brief asked
//!   for edges to behave "the way it does for nodes"; `BeginNodeDrag` called
//!   `select_only` unconditionally, so shift-clicking a second node replaced
//!   the selection with it and the multi-select a band produced could not be
//!   built up one press at a time. Both carry `additive` now.
//! - **[`runtime::HitTolerance::at_zoom`]** — §29's screen-pixel tolerance was
//!   two lines inside `views::flow`, which put the only statement of "a thin
//!   edge stays clickable at any zoom" in the one file that needs a `Window` to
//!   build. It is a pure function now, asserted at seven cameras.
//!
//! The trade is stated where it is paid: canvas within six screen pixels of a
//! route is canvas a rubber band can no longer be started in.
//!
//! ## Text wraps
//!
//! [`render::painter`]'s three named changes were the right three — `shape_text`
//! for `shape_line`, a wrapped line in the cache, and a line-height model — and
//! **the reason given for the first was not**:
//!
//! > `shape_line`'s fourth argument is not a wrap width. It is `force_width`,
//! > the per-glyph advance a terminal grid uses.
//!
//! So Phase 10 was not truncating long labels, as its own note said; it was
//! quietly scattering the glyphs of any label longer than half its box onto a
//! lattice of `max_width` steps. A parameter whose name you have not read is a
//! parameter you are guessing at, and the guess was plausible enough to be
//! written down as a fact and reviewed as one.
//!
//! - [`render::plan::TextPrimitive::vertical_offset`] and `line_height` are the
//!   model, and they are **pure** — the painter learns how many lines a
//!   paragraph took and lifts the block by half of every line past the first,
//!   so a one-line label lands on exactly the pixel Phase 10 put it on and a
//!   block of any height stays centred on the same point.
//! - **[`render::cache::TextKey`] gains the wrap width.** It could be omitted
//!   only while nothing wrapped; once a line break is part of the laid-out
//!   result, a cache that ignores the width is a paragraph laid out for a box
//!   the element has left. It is quantised down onto an eight-pixel grid for
//!   exactly the reason the font size is quantised — a node's wrap width is its
//!   *screen* width, so an exact one would re-wrap every visible paragraph on
//!   every frame of a zoom, reintroducing through a new field the cost §23's
//!   cache exists to remove.
//! - The wrap rule is **recorded per carrier** rather than left implicit: node
//!   text to its host's inner width, a standalone text element to its whole box
//!   (it has no border to keep clear of), an edge label to a constant screen
//!   width (an edge has no rectangle at all, and being a screen constant it
//!   never re-wraps on a zoom).
//! - `views::nodes` wraps too. A rich node truncated its label with an ellipsis
//!   while the canvas path wrapped, so one sentence read two ways either side
//!   of a zoom rung — the same element seen through two rungs must not disagree
//!   about what a label does.
//!
//! **The limitation this leaves**, at [`render::painter`]: a paragraph can
//! outgrow its box downwards. `shape_text`'s `line_clamp` would hide that by
//! deleting words, and overflowing text a user can fix by dragging a corner
//! beats words that are silently gone. Growing the container is an edit on
//! every keystroke and belongs with whatever gives an element an auto-height.
//!
//! ## What Phase 10.5 verified by running
//!
//! `cargo test` covers every claim above that does not need a window, including
//! the press that selects an edge and the delete and undo after it, driven
//! through the real machine and the real applier. The launcher was **run**, and
//! its first-frame report shows the wrapped-text path shaping real labels
//! through `shape_text` at two zoom levels. What it cannot show is still the
//! same list: an unattended window presents one frame and stops, so *clicking*
//! an edge, seeing its ring, pressing `Delete`, and typing a sentence long
//! enough to wrap are a human's to check.
//!
//! # What the eleventh slice added: the surface everything else was for
//!
//! §32's property panel. Ten phases built a model nobody could reach; this is
//! the one that makes it editable, and it takes **four forms** — a node panel,
//! an edge panel, a text panel and an image panel. **The differences between
//! the four are the specification**, not a detail of it: it is not one panel
//! with rows greyed out, and a node gets Background, Fill and a corner style
//! exactly where an edge gets Arrow type and Arrowheads. [`properties`]'s
//! module doc holds the table.
//!
//! - [`properties`] — the vocabulary, and **that table as data**. Which
//!   sections each selection kind gets, what each control's steps mean in the
//!   model, and both directions of every one of them. It names no UI framework,
//!   so the table is checked by a test that states it a second time and
//!   independently, rather than by opening a window.
//! - [`views::properties`] — the drawing. Two of its rows are drawn **by the
//!   generator that performs them**: the Sloppiness samples are a straight line
//!   handed to [`render::sketch::perturb`] at each step's own roughness, and the
//!   Fill samples are the real [`hatch`](mod@render::hatch) over a small square. Neither
//!   button can drift from what it selects, and neither needs an asset — which
//!   is `views::palette`'s trick, paying better.
//! - [`mod@render::hatch`] — §32's hachure and cross-hatch, as a scanline clip.
//!   **One path of many subpaths**, so a hatched fill is one tessellation, one
//!   cache entry and no extra batch; bounded at 64 lines per direction, so a
//!   shape zoomed to fill the display coarsens instead of costing the frame.
//! - [`commands::layers`] — the four Layers buttons' arithmetic, as a pure
//!   function over two [`DepthSpan`](commands::DepthSpan)s, with the four
//!   decisions it encodes written down.
//! - [`models::FillStyle`], [`models::Sloppiness`] and a `link` on both element
//!   types, all serialized, all undoable. Every default is what documents
//!   already looked like, so the format grew with no migration.
//! - [`commands::FlowEditor`] — `restyle_selection`, `reorder_selection`,
//!   `duplicate_selection`, `set_selection_link` and `link_at`. The panel hands
//!   a closure over an [`ElementStyle`](models::ElementStyle) to one method,
//!   which is why a fifteen-row panel needs no fifteen methods.
//!
//! ## Z-order against the batching contract, which is the interesting part
//!
//! [`render::plan`] emits **every quad, then every path, then every text, one
//! contiguous run each**, and that is a correctness contract rather than a
//! preference: each contiguous run is a full-viewport render pass, 192 of them
//! halve the frame rate, and the whole engine is built so interleaving is not
//! expressible. Z-order is a **per-element** ordering. The two had to be made to
//! compose rather than one of them bent.
//!
//! The answer is three parts and one limitation:
//!
//! 1. **Depth is exact within a run**, and it is achieved by ordering the
//!    *planning walk* rather than the paint. Edges and nodes are merged into one
//!    depth-ordered pass, so each bucket receives a depth-sorted run — the
//!    buckets already keep insertion order, so nothing about the contract
//!    changes.
//! 2. **A quad-bodied body that a depth needs above a path is promoted into the
//!    path run.** `render::scene`'s `promotes_to_path` states the three
//!    conditions and why each is load-bearing.
//! 3. **A node that must sit below a canvas-drawn body leaves the element
//!    layer**, because the rich half of §16's hybrid renderer paints above the
//!    canvas whatever the depths say. `render::snapshot`'s `place_in_depth_order`
//!    takes the largest safe suffix in one pass, which is the minimal set.
//!
//! **A document nobody has reordered pays one `bool` read per frame**, and
//! `an_unlayered_document_plans_exactly_what_it_always_did` asserts that as an
//! equality on what the painter is handed. [`GraphWorld::is_layered`](runtime::GraphWorld::is_layered)
//! is a counter kept exact by the two writers that can change a depth, so
//! undoing the only reorder puts the frame back on its fast path rather than
//! leaving it slow for the session.
//!
//! The measured cost is in `render::scene`'s
//! `a_layered_frame_costs_paths_in_proportion_to_the_detailed_bodies_on_screen`:
//! **zero on every benchmark scene**, and 108 paths / 3,804 estimated
//! vertices for a screenful of 54 detailed bodies — against a budget of 3,000
//! paths and a 2.4 M vertex ceiling.
//!
//! ## The coalescing rule that had to be new
//!
//! A slider drag is one undo step, and [`commands::history`]'s existing
//! mechanisms could not express it. `merge` folds **both** halves of a history
//! entry with one function, and a dragged control wants the *latest* forward
//! value and the *earliest* before-value — opposite folds of one variant, which
//! `MoveNodes`/`SetNodePositions` escape only by being two variants. A style
//! change's inverse genuinely *is* a style change, so that trick was not
//! available.
//!
//! [`EditCommand::supersedes`](commands::EditCommand::supersedes) is the third
//! mechanism: inside one gesture, an absolute per-element write **replaces** the
//! forward command and leaves the inverse alone. Sixty ticks are one entry as
//! well as one step, which matters because the stack is bounded and a gesture
//! longer than the limit would have been truncated mid-drag.
//!
//! ## What the tests found, which review would not have
//!
//! 1. **`in_one_step` closed the caller's gesture on its way out.**
//!    `begin_gesture` was already re-entrant; `end_gesture` was not. So the
//!    first tick of a slider drag ended the drag and the other fifty-nine each
//!    became an undo step of their own. Found by counting history entries.
//! 2. **Two rows were stored, undoable, read back by the panel — and painted by
//!    nothing.** `fill_style` and `sloppiness` reached the document and stopped
//!    there, and every test in the crate passed. This is the third costume of
//!    the same failure: Phase 7's dead key bindings, Phase 7.5's absent tool
//!    palette, and now a control that writes a field no painter reads. The
//!    tests that close it assert *what reaches the painter* — the hatch by its
//!    [`GeometryPart`](render::cache::GeometryPart), the hand by its cache key —
//!    because a test on the model would have passed either way.
//! 3. **A shape with no area produced sixty-four degenerate hatch subpaths.**
//!    A flat rectangle still has extent along the sweep's normal, so the parity
//!    was right and every span was a point. Found by asking what an empty shape
//!    fills with.
//!
//! ## And the limitations a user meets, recorded where they are caused
//!
//! 1. **A text element cannot be sent behind a shape's fill.** Text is a run of
//!    its own and always the last, and there is no promotion available for it —
//!    a glyph run has no outline form. The Layers buttons order text against
//!    other text and against nothing else. `render::scene`'s `promotes_to_path`
//!    carries it.
//! 2. **A node demoted out of the element layer loses its accent bar, its glyph
//!    and its hover feedback**, and keeps its body, its border and its label.
//!    That is the price of an ordering the user asked for and it is paid only by
//!    the elements the ordering reached. `render::snapshot`'s
//!    `place_in_depth_order` carries it.
//! 3. **Sloppiness is muted in Clean mode rather than hidden or silently
//!    stored.** It is a real per-element property that a clean drawing cannot
//!    show; the muted button's tooltip says so. That is `views::palette`'s own
//!    answer for Delete-with-nothing-selected, and being consistent with dodo
//!    beat being clever. [`properties::Availability`] carries the argument.
//! 4. **Edit points and Crop are absent rather than stubbed.** An edge stores
//!    two endpoints and a routing, never a point list — §7's waypoints are a
//!    change to the document model, the same gap that makes a free arrow point
//!    down its own diagonal — and there is no image to crop until Phase 12.
//!    [`properties::ElementAction::for_kind`] says so where the buttons are
//!    chosen.
//! 5. **A mixed selection shows the leading element's values.** Two shapes with
//!    two stroke colours have no honest single answer, so the panel shows the
//!    first and a press writes to all of them. The alternative is a tri-state on
//!    every control, for a case that resolves itself the moment anybody presses
//!    anything.
//!
//! ## What was verified by running, and what was not
//!
//! Everything above is `cargo test` with no window: the table, the round trips,
//! the depth order as the sequence the painter is handed, the coalescing as a
//! count of history entries, the layered cost as a bound, and — through
//! `commands::tests`' `selecting_each_kind_in_turn_changes_the_rows_the_panel_draws`
//! — the whole chain from a real selection to the rows the panel draws. The
//! launcher was run and presents its first frame with a selection and a panel,
//! one path batch and nothing dropped.
//!
//! **What that cannot show is whether it looks right**, and this phase is more
//! visual than any before it. `examples/flow.rs` lists the eight things only a
//! person at the keyboard can check. This is the fourth phase to hand that
//! forward and it is the same risk the plan named as "live input is
//! source-verified, never observed".
//!
//! # What the twelfth slice added: pictures, and the run that moves
//!
//! §10. The last of the captain's editing scope, and the feature is images;
//! **the deliverable is two rules that had to be made structural**, because
//! both fail silently and both are easy to write the wrong way once and never
//! notice.
//!
//! The first is the requirement's own: *do not duplicate raw image bytes per
//! element*. [`models::image`] makes that unexpressible rather than
//! remembered — an element carries an [`ImageHandle`](models::ImageHandle),
//! which is a **content hash**, and the bytes live once in
//! [`FlowDocument::images`](models::FlowDocument::images). One file inserted
//! twice collides by construction, the Duplicate action copies twenty bytes,
//! and the decode cache in [`views::images`] is keyed the same way, so two
//! elements showing one picture share the decoded pixels as well as the file.
//!
//! - **The bytes are embedded, not referenced.** A path breaks silently the
//!   moment a document is moved or sent to somebody, and dodo has no asset
//!   directory for a canvas file to sit beside — that is Phase 8's question and
//!   `docs/architecture/persistence.md` is its authority. The cost, base64's
//!   33 %, is recorded beside the decision.
//! - **Document version 3, and the rung it climbs is the identity.** The
//!   version moved for what it tells an *older* build: dropping an unknown
//!   `fill_style` loses a look a press restores; dropping an unknown `images`
//!   table loses the only copy of a photograph.
//!   [`CURRENT_VERSION`](models::CURRENT_VERSION) carries that rule, which is
//!   also why Phase 11 was right not to move it.
//! - **A crop is four fractions of the source** ([`models::ImageCrop`]) and it
//!   is spent as arithmetic on a child element's box —
//!   [`views::images::crop_layout`] — with the parent's clip doing the rest. No
//!   pixel is read and no buffer is copied, which is §10's "the original bytes
//!   are untouched and shared" holding at the render layer as well as in the
//!   file.
//!
//! ## The picture is an element, and that is not a preference
//!
//! GPUI paints a bitmap in one call and it **cannot carry an opacity**:
//! `Window::paint_image` reads the sprite's alpha from
//! `Window::element_opacity`, which is `pub(crate)` and is written by exactly
//! one thing in the framework — a styled element's own paint. The property
//! panel gives every kind an Opacity row, images included, so the raw call
//! would have shipped a control that writes a field no painter reads: Phase 7's
//! dead bindings, Phase 7.5's absent palette and Phase 11's unread `fill_style`
//! for the fourth time.
//!
//! So a picture is built and **prepainted** by [`views::images`] during the
//! canvas's prepaint — the only phase GPUI lays an element out in — and painted
//! by [`render::painter`] at the point in
//! [`PaintPlan::paint_into`](render::PaintPlan::paint_into)'s order the image
//! run occupies. It keeps its place among the bodies instead of floating above
//! everything the way the rich half does, and it gets opacity, the crop's clip
//! and a corner radius from the element tree.
//!
//! ## Z-order against the batching contract, the second time
//!
//! Phase 11 composed a per-element depth with the per-kind runs by **promoting**
//! a quad-bodied body into the path run. A bitmap has no outline form — exactly
//! as a glyph run has none — so that mechanism is unavailable, and with the
//! image run pinned anywhere the depth order is half-dead in one direction.
//!
//! **The run moves instead.** It is emitted before the paths by default, which
//! is where a picture belongs (put a screenshot down, annotate over it), and
//! after them when [`render::scene`]'s `images_belong_above_paths` finds the
//! topmost picture above the topmost path-bodied element. One contiguous run
//! either way, so the contract Phase 0 measured is untouched; an unlayered
//! document pays one `bool` read, like every other part of the depth machinery.
//!
//! ## What was missing, and had to be built before any of it could be used
//!
//! **There was no resize gesture.** `ResizeNodes` and its applier had existed
//! since Phase 7 and nothing raised one: no state, no grips, no hit test — so
//! an element's size could be changed by a command and by nothing a person
//! could do. It is built here for **every** kind rather than for images:
//! [`geometry::resize_from_corner`], [`runtime::PointerTarget::ResizeGrip`],
//! `InteractionState::Resizing`, and two commands in one gesture (a corner drag
//! moves the origin as well as the size, and the two coalesce by different
//! rules — see [`commands::EditCommand::merge`] and `supersedes`).
//!
//! **§10's aspect lock is the element's default and shift asks for the other
//! one** — locked for a picture, free for a shape. That reads as "shift
//! constrains" on a shape and "shift releases" on an image, which is one rule
//! rather than two, and it is what makes Crop an action rather than a mode:
//! shift-drag a corner to say what shape you want, then press Crop to turn that
//! stretch into a window on the source. [`properties::crop_choice`] is the
//! whole of the button's three states.
//!
//! ## The numbers
//!
//! `render::scene`'s `a_screenful_of_pictures_costs_no_path_vertices`, on the
//! same 1440×900 pane the rest of this file measures: **twenty-four pictures
//! filling the view cost 0 path vertices and 0 path batches.** A picture is a
//! textured quad; its batching cost is GPUI's, one sprite batch per *atlas
//! texture* rather than one per picture, and a sprite batch is a draw call with
//! a texture bind rather than the full-viewport intermediate pass with a clear
//! that a path batch costs. The vertex ceiling and Phase 11's layered-cost bound
//! are both untouched by images.
//!
//! ## What the launcher's own report found, which no test would have
//!
//! `DODO_FLOW_PICTURES=1` opens the camera on the demo row and the first-frame
//! report said **two pictures planned, one painted**. `render` extracts §24's
//! snapshot against the *previous* pane — it is handed no bounds — and paint
//! was where that had always been noticed. A picture is laid out one phase
//! earlier than the plan is built, so on the first frame and after every resize
//! the prepaint used the stale set and the plan the fresh one. `sync_pane` runs
//! in prepaint now. **The pattern is Phase 7's and Phase 7.5's a third time**:
//! nothing failed, every test passed, and a capability was quietly absent for
//! one frame at a time.
//!
//! ## And the limitations a user meets, recorded where they are caused
//!
//! 1. **Two pictures on opposite sides of one path-bodied body cannot both be
//!    honoured.** The image run moves as a whole, and the flag is set from the
//!    topmost picture — so a screenshot behind a diagram and a logo over it
//!    disagree, and the logo loses. [`render::plan`]'s module doc carries it.
//!    Per-picture ordering against paths needs a promotion a bitmap cannot
//!    have, or one render pass per picture.
//! 2. **A cropped picture has square corners.** The Edges row rounds the
//!    *sprite*, and a crop is a clip: GPUI's content mask is a rectangle with
//!    no radii, so the rounding a cropped picture would show is off-screen.
//!    [`views::images`] says so where it is caused.
//! 3. **The crop is centred and cannot be nudged.** "Which part of the picture"
//!    is chosen by resizing the frame, and the window it produces keeps the
//!    middle. Panning the crop inside its frame is a second drag gesture on an
//!    element that already has one, and it belongs with whatever gives the
//!    canvas modal in-place editing.
//! 4. **A picture cannot be rotated, and neither can anything else.** Nothing
//!    in the engine has an angle — `commands::edit`'s doc has had that recorded
//!    since Phase 7 — so the resize grips are the four corners of an
//!    axis-aligned box.
//! 5. **Decoding a large picture stalls the frame that first needs it.** The
//!    insert path decodes off the moment a file is chosen and primes the cache,
//!    so this is only paid by a *loaded* document's first frame, once per
//!    picture. [`views::images`] carries the trade.
//!
//! ## What was verified by running, and what was not
//!
//! Everything above is `cargo test` with no window: the sharing rule as a
//! count, the round trip with a crop in it, the resize through the real machine
//! and the real applier (locked and free), the run's position on both sides of
//! the paths as the sequence a painter is handed, and every edit as an undo
//! depth. The launcher was **run**, and its report shows two pictures planned,
//! two painted and one resource decoded — §10's rule in a real frame.
//!
//! **What that cannot show is whether it looks right, and this phase has three
//! things that are expected to look odd.** `examples/flow.rs` lists the seven
//! things only a person at the keyboard can check, and flags which of them are
//! recorded limitations rather than bugs. This is the fifth phase to hand that
//! forward and it is the same risk the plan named as "live input is
//! source-verified, never observed".
//!
//! # What the twelfth-and-a-half slice added: three bugs a person found
//!
//! The captain used the canvas and reported three faults. **Every automated
//! gate passed on all three**, which is now the fourth time this crate has
//! shipped a capability that was absent or dead with nothing failing — and the
//! three causes are worth more than the three fixes, because two of them are
//! one mechanism and none of them is a mistake in the layer that was blamed.
//!
//! ## Chrome did not block the press underneath it
//!
//! [`views::flow`] registers its mouse listeners on the **window** and gates
//! them on the canvas hitbox, because GPUI does no implicit hit testing for
//! them. GPUI's hit test keeps every hitbox under the pointer, front to back,
//! until one whose behaviour is `BlockMouse`; a plain `div()` is `Normal`. So
//! the palette, the property panel and the caret each let **one press be
//! delivered twice** — to themselves first, because bubble-phase listeners run
//! front to back, and then to the canvas.
//!
//! - Picking a tool armed it *and* was read as the press that begins a
//!   creation; the release, under [`MIN_DRAG_PIXELS`](interaction::tool::MIN_DRAG_PIXELS),
//!   made it a click, so a default-sized shape appeared under the palette.
//! - Pressing a property control applied the edit *and* was read as a press on
//!   empty canvas, which begins a rubber band whose release replaces the
//!   selection with nothing — and the panel is drawn from the selection.
//!
//! Both files carried a comment claiming the `on_mouse_down` prevented exactly
//! this. **A comment stating an invariant is not the invariant**, and this one
//! had been read and copied from one file to the other.
//!
//! ## The canvas's bare letters were live over the canvas's own text fields
//!
//! [`views::keymap`]'s doc had already worked out that a context-less binding
//! would be swallowed before every text field in dodo, and concluded that
//! scoping to `FlowCanvas` meant "they reach nothing else". That is true of
//! every field *except the canvas's own*: §9's caret and Phase 11's hex prompt
//! are descendants of the root that carries the context, so it is on their
//! dispatch path, and `gpui-component`'s `Input` context binds no bare letter
//! to outrank it. Eleven of the letters a person types were consumed as canvas
//! actions — and every one of those handlers calls `focus_handle.focus`, so the
//! *first* of them ended the edit.
//!
//! The captain asked whether re-rendering was dropping the focus. It was not,
//! and the distinction is the transferable part: the field is focused, stays
//! focused across any number of repaints, and a word made of unbound letters
//! types perfectly. `FlowCanvas && !FlowTyping` is the fix, and
//! `no_canvas_binding_survives_a_text_field_inside_the_canvas` drives GPUI's
//! own matcher over the real context stacks in both directions.
//!
//! ## One renderer too many, which is why a property "only worked in sketch"
//!
//! A rectangle at working zoom is a **rich** node, and the element painted its
//! own body from the theme. So Phase 11's whole panel wrote a stroke colour, a
//! fill, a width, an opacity, a dash and a hatch that nothing on screen read —
//! *unless* the document was in Sketch mode, where a hand-drawn border has no
//! `div` form and the canvas had to paint the body. The mode was the symptom
//! and never the cause, and the elements it spared say so: an ellipse, a
//! diamond, a line, a text element, a picture and an edge all updated in Clean
//! mode, because none of them is ever a rich node.
//!
//! `render::scene`'s `plan_rich_bodies` runs in both styles now, through the
//! one `plan_one_node` every other body goes through, and `NodeBody` is that
//! stated as a type: **one body painter, two callers.** [`views::nodes`] keeps
//! only what an element is *for*.
//!
//! Two more of the same kind, found by checking every panel row against a
//! painter rather than against the model — both broken in *both* modes and
//! neither reported: **a node's dash was never drawn** (only `render::edges`
//! read `stroke.dash`), and **a text element was never drawn in the colour its
//! Stroke row writes** (it writes `stroke.color`; every text painter read
//! `font.color`, which nothing writes). [`properties`]'s module doc now carries
//! the rule and every costume it has worn.
//!
//! ## What this phase's tests can and cannot be
//!
//! Three of the five assert behaviour with no window: the binding predicate
//! against real context stacks, and what a restyled or hatched rich node hands
//! the painter in both render styles. Two are **source** assertions — that each
//! overlay still declares `occlude()` and each text field still declares its
//! context — and that is deliberate rather than lazy. Whether two hitboxes
//! occlude is a fact about a painted frame; the crate's whole argument is that
//! it is testable *because* nothing below `views/` needs a window, and buying a
//! windowed harness to check two lines would trade that for very little. It is
//! the same trade `i18n_lint` and `the_pure_layers_name_no_ui_framework` make.
//!
//! **What only a person can confirm is unchanged in kind**: that picking a tool
//! creates nothing until you drag, that a colour applies in Clean mode with the
//! panel still open, and that a sentence can be typed into a node.
//! `examples/flow.rs` lists them.
//!
//! # What the eighth and final slice added: the app seam
//!
//! Phase 8 is deliberately small: dodo aliases this crate, calls [`init`] once
//! for the canvas-scoped bindings, and gives [`FlowView`] the last row in its
//! `tools!` table. The row uses the workflow glyph, is last in the default
//! sidebar order and declares no paste detector.
//!
//! The active diagram lives in `flow.json` beneath dodo's existing
//! `data_dir()`. [`services::document_store`] follows the same contract as the
//! app's other stores: missing means a new document, parsing uses the format's
//! existing migration ladder, writes go through a sibling temporary file, and
//! all blocking work runs on GPUI's background executor. A refused load
//! disables writes for that view, so the first edit cannot replace a corrupt or
//! newer document with an empty canvas.
//!
//! Persistence does not make `render` document-sized. [`FlowEditor`] stamps a
//! revision only when serialized state changes; the view compares that stamp
//! and clones the document only for a real save. Selection, panning and an
//! ancestor repaint leave it alone.
//!
//! # What the thirteenth slice added: connectors that have two ends
//!
//! The captain reported five faults on straight lines and arrows: they behaved
//! like rectangles, drawing them was direction-sensitive, endpoints could not
//! be attached to or detached from an element, a double-click did not put the
//! caret anywhere you could type, and committed text vanished. **Four of the
//! five were one cause**, and it is the fifth time this crate has shipped
//! something that looked finished and was not: a linear element stored an
//! origin and a size, so its geometry *was* a normalised rectangle and there
//! was nowhere for the answer to live.
//!
//! ## The rectangle was the divergence, and it is now derived
//!
//! [`models::Connector`] is the authority for `Linear(Line | Arrow)`: two
//! ordered [`ConnectorEndpoint`]s, each either free or bound to a
//! non-connector element by id plus a normalised anchor. The rectangle is
//! computed from the segment for culling, selection bounds and the coarse
//! broad phase — the three things a box is genuinely good for — and it never
//! reorders the endpoints. The one place it flows the other way is
//! [`Connector::with_bounds`], which rebuilds the segment inside an absolute
//! rectangle **keeping each endpoint on the corner it already occupies**; that
//! is what lets `commands::apply`'s position and size commands, which speak
//! rectangles and nothing else, reach a connector and still invert exactly.
//!
//! Everything downstream of the box moved with it, and each one was its own
//! visible fault:
//!
//! - [`interaction::tool::connector_endpoints`] is the ordered twin of
//!   `creation_rect`, and `creation_rect` is now *derived from it*. All three
//!   of that function's rules — a click takes the default extent, shift squares
//!   the travel, otherwise the end is the pointer — are stated once, so the
//!   preview, the committed bounds and the committed segment cannot disagree
//!   in any of the eight directions.
//! - [`render::shapes::arrow_between`] puts the head on the true `end`. The
//!   old `arrow(rect)` put it on `rect.max()`, which is why an arrow dragged
//!   leftwards pointed right.
//! - [`render::snapshot::SnapshotOverlay`] carries two endpoint handles instead
//!   of four resize grips, and `GraphWorld::hit_test_connector_endpoint`
//!   answers only those two.
//! - `GraphWorld::hit_test` measures distance to the *segment* for a connector,
//!   so the empty half of a diagonal's bounding box is canvas again.
//!
//! ## Attachment is a binding, not a coincident point
//!
//! A bound endpoint is resolved from its target's current geometry, so it
//! follows a move and a resize, and it survives save/load and undo/redo because
//! what is persisted is the id and the anchor. `GraphWorld` keeps a
//! target → bound-endpoint index beside `adjacency` for exactly the reason
//! `adjacency` exists: moving one element must not scan the document.
//! `Side::facing` plus `floating_point` choose the direction-appropriate edge
//! from the *opposite* endpoint, which is why dragging one end round a box
//! walks the attachment round with it instead of pinning it to one side.
//!
//! Two rules keep the graph sane and are asserted rather than assumed: a
//! connector may not bind to another connector or to itself, and an attachment
//! that names a missing or invalid target loads **detached at its persisted
//! point** and is reported in [`runtime::LoadReport`] — never silently moved.
//!
//! ## Format version 4, and the only honest migration
//!
//! Versions 1–3 never retained the drag direction, so there is nothing to
//! recover: the migration writes the diagonal those files already *displayed*,
//! `position` → `position + size`. That is a real loss of nothing, and stating
//! it here is cheaper than a reader later assuming the old files were reversed.
//!
//! ## The label that was written and never read
//!
//! The fifth fault was separate and simpler, and it had two independent
//! causes that produced the identical symptom — type into a shape or an arrow,
//! commit, watch the words disappear:
//!
//! 1. [`render::registry`] answered `shows_label: false` for every `Shape` and
//!    every `Linear` kind. The commit went through
//!    [`FlowEditor::commit_text`](commands::FlowEditor::commit_text) and the
//!    applier exactly as it should; no painter ever asked for the result.
//! 2. `plan_labels` insets a node's box by `LABEL_PADDING_PIXELS` to keep text
//!    off its own border. An axis-aligned connector's derived box has zero
//!    height, so the inset went negative and the label was skipped. A connector
//!    now gets a box of its own centred on the **true segment midpoint**, which
//!    is also where [`views::flow`]'s inline editor opens.
//!
//! **This is the same failure the twelfth-and-a-half slice recorded twice** —
//! a style field no painter reads — arriving through a label instead of a
//! stroke. `properties`' module doc carries the rule and now lists five
//! costumes; this one is the first that is not a style row, which is the
//! transferable part: **a field the document stores is not finished until a
//! painter reads it**, whether a panel writes it or a caret does.
//!
//! ## The caret that did not have the keyboard
//!
//! `begin_text_edit` focused the field *before* storing it on the view and
//! never set its selection, so a double-click left an empty caret at offset
//! zero and the first keystrokes reached the canvas — where a bare letter is a
//! tool binding. Those are two lines in the right order; the interesting part
//! is that it is the first thing in this crate asserted on a **real test
//! window**. `views::flow`'s
//! `a_double_click_opens_an_editor_that_already_owns_the_keyboard` drives the
//! machine transition and the effect handler the double-click actually uses,
//! because "does this element hold the keyboard?" is a window question and the
//! twelfth-and-a-half slice's trade — source assertions rather than a windowed
//! harness — buys nothing once the harness is one dev-dependency feature away.
//!
//! **Corrected later: those two lines were necessary and not sufficient, and
//! the test above could not see why.** It calls the effect handler directly, so
//! nothing else on the press runs — and what was undoing it was a listener
//! nobody in this crate registered. The canvas root carries `track_focus`, and
//! GPUI answers that by registering a bubble-phase `MouseDownEvent` listener
//! which focuses the tracked handle unless the default is prevented. Bubble
//! listeners run in **reverse** registration order and a parent registers
//! before its children paint, so that listener is the *last* thing to touch the
//! focus on the very press that opened the caret: the field was focused and the
//! canvas took it back a moment later, on the same event. `begin_text_edit`
//! now calls `Window::prevent_default`, which is the flag that listener reads.
//!
//! The transferable part is the same shape as the twelfth-and-a-half slice's:
//! a symptom that looks like "focus is being lost" is almost never the widget
//! losing it, and the way to tell is to drive the **real event** rather than
//! the handler.
//! `the_press_that_opens_a_caret_does_not_hand_the_keyboard_back` does, with
//! one wrinkle worth knowing: a *focused* `gpui_component::Input` cannot be
//! painted on a GPUI test window at all — its render asks the platform window
//! for an `NSView` and the test window answers `unimplemented!` — so the
//! assertion happens on the dispatch and the caret is closed again before the
//! frame that follows it.
//!
//! ## What only a person can confirm
//!
//! That the snap highlight reads as an invitation rather than as a selection,
//! that the two endpoint grips are findable at working zoom, and that a label
//! on a steep diagonal sits somewhere a reader would look for it.
//! `examples/flow.rs` lists them beside the earlier slices'.
//!
//! # What the fourteenth slice added: a label belongs to its element
//!
//! Three changes that are one idea. A label was a thing the document happened
//! to store beside an element; it is now part of the element — it sits in the
//! middle of it, it is drawn in its ink, and selecting the element offers the
//! controls that shape it.
//!
//! ## A label has no position, and that is the answer rather than a gap
//!
//! Nothing in this engine stores where a label sits. `render::scene` lays one
//! into its carrier's box every frame, so "where is this label?" is answered by
//! [`models::TextAlign`] and its new twin [`models::VerticalAlign`] and by
//! nothing else. That made the requirement small: **the default placement is
//! the default pair of alignments**, and centring a label is
//! [`models::FontStyle::centre_on_element`].
//!
//! It is applied where a label is *born* —
//! [`FlowEditor::commit_text`](commands::FlowEditor::commit_text), in the same
//! undo step as the words — rather than made the type's `Default`, because the
//! same default seeds a **standalone text element**, which reads from its left
//! edge like any other block of prose. A label centres; a paragraph does not.
//! And because it fires only on the first label, a user who then aligns one
//! left keeps it there through every later edit of the words.
//!
//! ## The box a label is laid into, per carrier
//!
//! | | box |
//! |---|---|
//! | a standalone text element | its whole rectangle — it has no border to keep clear of |
//! | any other node | its rectangle, inset by `LABEL_PADDING_PIXELS` |
//! | a connector, an edge | `render::scene::boxless_label_box`, centred on the true midpoint |
//!
//! The third is the one that had to be invented, and it grew a **height** this
//! slice: `EDGE_LABEL_BAND_PIXELS`, so that on a line — which has no box at all
//! — the vertical row means *above it, on it, below it*. `Middle` is the exact
//! midpoint, which is where every edge label has been drawn since §9.
//!
//! **`views::flow`'s `text_edit_bounds` is the fourth statement of that table**
//! and now returns the same box, so the caret opens over the rectangle the
//! painter will use.
//!
//! *Corrected later:* `editor_band` no longer takes the vertical alignment. The
//! field is a multi-line block placed inside that box by flexbox, so the
//! alignment is applied to the field rather than arithmetic on the band, and
//! what the band carries instead is the two **constant** differences between
//! how `render::painter` places a block of text and how a `gpui-component`
//! `Input` places one. See the section on the caret that was drawn twice.
//!
//! ## A label is drawn in its element's ink
//!
//! [`models::ElementStyle::text_color`] is one function because there was one
//! bug: §32 gives a node, an edge and a text element a Stroke row and no
//! separate text colour, and every text painter read `font.color` — which no
//! control writes. The twelfth-and-a-half slice fixed that in one painter for
//! one kind; a label on a node or an edge still drew in the theme's foreground,
//! so changing a shape's stroke moved its outline and left its words behind.
//!
//! ## The row that depends on what an element *contains*
//!
//! [`properties::SelectionKind::sections`] takes a second argument —
//! [`properties::Labelled`] — and that is a real departure from the model
//! Phase 11 built: every other row is a pure function of the kind. The four
//! text rows are offered to a node or an edge **when it has a label** and
//! withheld when it does not, because a Font size row over a shape with no text
//! is a control that does nothing.
//!
//! The intersection rule needed no second rule: a selection holding one
//! labelled shape and one unlabelled one shows no text rows, exactly as a
//! selection holding a shape and a picture shows no Stroke row. The reference
//! table in `properties`' tests states **eight** columns rather than four, so
//! the axis is stated twice like everything else in it.
//!
//! ## The two renderers, again
//!
//! A rectangle at working zoom is a `RichNode` — a GPUI element — and
//! [`views::nodes`] draws its label with a `div`. That is the renderer a person
//! is looking at while they use these four rows, and it read **none** of them:
//! the theme's foreground, the theme's face, flex-centred vertically and
//! flowed from the left. Phase 12.5 met this exact shape once already and it
//! arrived as *"the properties only work in sketch mode"*. Both halves read the
//! same style now, and `views::flow`'s
//! `the_rich_half_reads_every_text_property_the_canvas_half_does` is the
//! source assertion that keeps them in step — the same trade the occlusion and
//! typing-context tests make, and for the same reason.
//!
//! ## Format version 5, and the only rung so far that changes how a file looks
//!
//! `vertical_align` on its own would have cost no version: an older build drops
//! it and it defaults straight back to what it displayed. The version moved for
//! the **rewrite** beside it, which has to happen exactly once — every node
//! that is not a standalone text element, and every edge, with a label, is
//! written to `align: Center`.
//!
//! That is safe for one reason and it is worth stating plainly: before version
//! 5 the panel offered the Text align row to *standalone text elements only*,
//! so **no control anywhere could set a node's or an edge's alignment** and
//! every value the rung overwrites is the default arriving by default. An
//! edge's label was additionally drawn centred whatever its style said, so for
//! an edge the rung writes down what the file already displayed. A standalone
//! text element is untouched, because its alignment *was* reachable and a
//! `Left` there may well be a decision.
//!
//! ## What the reference asked for and this does not have
//!
//! The captain's panel screenshot shows **four** font-family buttons: a pen, a
//! plain `A`, `</>`, and — past a divider — a serif `A`.
//! [`models::FontFamily`] has three variants and they are the first three.
//! The fourth is left out rather than guessed at: there is no serif family in
//! this model, dodo ships no faces of its own, and inventing one would mean
//! choosing a font name for three platforms with nothing to check it against.
//!
//! ## What only a person can confirm
//!
//! That a centred label reads as belonging to its shape, that a label crossing
//! the rich/canvas rung does not visibly change, and that the vertical row's
//! three glyphs are legible at 16 px. `examples/flow.rs` lists the seven.
//!
//! The gap that used to be flagged here — the inline editor laying its own text
//! out from the left, so a centred label slid into place on commit — is closed:
//! `views::flow`'s `text_editor_element` reads the label's own face, size, ink
//! and both alignments, and draws no box at all.
//!
//! # The caret that was drawn twice
//!
//! Taking the editor's box away uncovered something that had been true since
//! §9 and invisible for exactly as long: **the canvas paints the committed
//! label underneath the caret**. The box was opaque, so it covered it. Without
//! the box the same words appear twice, a line apart, one copy carrying the
//! keystroke the other has not seen — the captain's report, and not a fault in
//! the chromeless caret.
//!
//! The mechanism is one field. A committed label is drawn in three places —
//! `render::scene`'s `plan_labels` and `plan_edge_labels`, and `views::nodes`'s
//! element — and **all three gate on the snapshot's `label_font_size`**. So
//! [`render::snapshot::RenderSnapshot::extract`] takes the
//! [`interaction::TextTarget`] the caret is open on and gives that element no
//! shaped size for the frame. One edit, every kind covered, and a fourth
//! renderer cannot forget it.
//!
//! **The regression test is on the frame, not on the source**, and that is the
//! point of it. The commit that removed the box could only assert that the
//! source builds no border, because whether a pixel was painted is a fact about
//! a window; a source assertion cannot see two elements both being drawn. A
//! *label* is not a pixel, though — it is a `TextPrimitive` in the plan and an
//! `Option<f32>` in the snapshot — so
//! `a_label_being_edited_is_not_also_drawn_by_the_canvas` asserts both, over a
//! table of every kind that can carry one. The transferable part: when a
//! windowless crate cannot see the symptom, look for the *data* the symptom is
//! made of before settling for a source grep.
//!
//! ## And the offset was a second fault
//!
//! The two runs were a line apart rather than exactly superimposed, which was
//! its own bug and would have survived hiding one of them — the survivor would
//! simply have been in the wrong place, and the text would jump when the edit
//! ended. Three disagreements, all now arithmetic in `editor_band`:
//!
//! - **The field was one line and the label wraps.** It is `auto_grow`
//!   multi-line now, soft-wrapping at the label's own width.
//! - **`gpui-component` flows lines at 1.25 × the text size and
//!   `render::painter` at 1.3** ([`render::plan::TextPrimitive::LINE_HEIGHT_RATIO`]).
//!   One number, the painter's.
//! - **The two measure a block of `n` lines differently** — `n × line_height`
//!   against `font_size + (n − 1) × line_height` — and **wrap at one width
//!   while aligning within another**, ten pixels apart. Both differences are
//!   constant in `n`, so the band is the label's box grown by each of them, and
//!   the field's own layout then lands on the painter's pixels for every `n`.
//!   Growing the box rather than sliding it is load-bearing: `VerticalAlign`
//!   clamps, and two boxes of different heights reach that clamp at different
//!   moments.
//!
//! `views::nodes` also padded a rich label by 10 px horizontally where the
//! canvas insets by `LABEL_PADDING_PIXELS`, so the same label sat four pixels
//! further in on a rectangle than on a diamond. One number now — a label is
//! drawn by three renderers in this crate and they have to agree about its box
//! as much as about its style.
//!
//! ## `Enter` is a line break, and the commit is a real binding
//!
//! A label wraps, so the caret has to. Making it a paragraph field — `Input`'s
//! `auto_grow` mode — is the same change as the wrap reconciliation above and
//! it settles the keyboard as a side effect: **`Enter` never was a binding**.
//! It reached the canvas's raw key handler only because a *single-line*
//! `gpui-component` `Input` declines that key and calls `cx.propagate()`, and
//! that handler turned it into `FinishTextEdit`. A multi-line field keeps the
//! key and inserts the break, so the fall-through is gone: a keystroke the
//! widget happened not to want is not a design, and on any path where the field
//! *does* propagate — a context menu is open — it would have committed behind
//! the user's back.
//!
//! So the ending is a row in `commands::keys` like every other: `Cmd`+`Enter`
//! on macOS, `Ctrl`+`Enter` elsewhere, both asserted from any machine.
//! **It is the one row in that table registered under a different scope.**
//! Every other canvas binding is scoped `FlowCanvas && !FlowTyping`, because
//! every one of them takes the focus back and would end an edit; this one *is*
//! the ending, so `keys::EditAction::while_typing` picks
//! [`views::flow::TYPING_BINDING_SCOPE`] instead — and that predicate names two
//! contexts rather than one for a reason worth keeping:
//!
//! **GPUI scores a binding by the depth at which its predicate matches the
//! context stack, and `gpui-component`'s `Input` binds `secondary-enter`
//! itself** — to insert a line break. The `Input` context is one node deeper
//! than the wrapper's `FlowTyping`, so `FlowTyping` alone always loses. Naming
//! both (`FlowTyping > Input`) ties on depth, and GPUI breaks a depth tie by
//! registration order with the later binding winning — which is why `src/main.rs`
//! runs `flow::init` after `gpui_component::init`, as `settings`,
//! `api_explorer`, `docker` and `database` already do.
//! `the_commit_keystroke_outranks_the_field_s_own_line_break` drives that
//! through GPUI's real `Keymap`, with the library's real action type and its
//! real predicate, in the real order — every one of which could change
//! underneath this without producing an error, only a `Cmd`+`Enter` that types
//! a line break.
//!
//! **The format does not move for this.** A label is a JSON string and JSON
//! strings carry newlines; `commit_text`'s `trim` takes the break off the end
//! that a final `Enter` left and leaves every break inside the label alone; and
//! `render::painter`'s `shape_wrapped` has split on `\n` since §9, so an older
//! build reads the paragraph and draws it as one. Nothing is discarded, which
//! is exactly the rule [`models::serialization::CURRENT_VERSION`] moves by.
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
pub mod commands;
pub mod geometry;
pub mod instrument;
pub mod interaction;
pub mod models;
pub mod paths;
pub mod properties;
pub mod render;
pub mod runtime;
pub mod scenes;
pub(crate) mod services;
pub mod spatial;
pub mod views;

pub use budgets::RenderBudgets;
pub use commands::{CommandHistory, EditCommand, EditError, FlowEditor, NodeDraft};
pub use geometry::{Rect, Vec2, Viewport};
pub use instrument::{Instruments, Probe};
pub use interaction::{InteractionEffect, InteractionEvent, InteractionMachine, InteractionState};
pub use models::{
    Connector, ConnectorAttachment, ConnectorEnd, ConnectorEndpoint, ElementId, ElementKind,
    FlowDocument,
};
pub use properties::{PanelSection, SelectionKind};
pub use render::{GridSettings, GridStyle, PaintPlan, PaintStats, SceneInk, SceneOptions};
pub use runtime::{
    BoxQuery, BoxSelectMode, ConnectionRules, EdgeEnd, GraphWorld, NodeSpec, PointerTarget,
    SelectionSet,
};
pub use scenes::SceneSpec;
pub use spatial::{SpatialIndex, UniformGrid, VisibleSet};
pub use views::{FlowView, init};

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
        ("instrument.rs", include_str!("instrument.rs")),
        ("commands/mod.rs", include_str!("commands/mod.rs")),
        ("commands/apply.rs", include_str!("commands/apply.rs")),
        ("commands/edit.rs", include_str!("commands/edit.rs")),
        ("commands/editor.rs", include_str!("commands/editor.rs")),
        ("commands/gesture.rs", include_str!("commands/gesture.rs")),
        ("commands/history.rs", include_str!("commands/history.rs")),
        ("commands/keys.rs", include_str!("commands/keys.rs")),
        ("commands/layers.rs", include_str!("commands/layers.rs")),
        ("geometry/mod.rs", include_str!("geometry/mod.rs")),
        ("geometry/arrow.rs", include_str!("geometry/arrow.rs")),
        ("geometry/bounds.rs", include_str!("geometry/bounds.rs")),
        ("geometry/curve.rs", include_str!("geometry/curve.rs")),
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
        ("paths.rs", include_str!("paths.rs")),
        ("properties.rs", include_str!("properties.rs")),
        ("render/cache.rs", include_str!("render/cache.rs")),
        ("render/edges.rs", include_str!("render/edges.rs")),
        ("render/grid.rs", include_str!("render/grid.rs")),
        ("render/hatch.rs", include_str!("render/hatch.rs")),
        ("render/lod.rs", include_str!("render/lod.rs")),
        ("render/mod.rs", include_str!("render/mod.rs")),
        ("render/plan.rs", include_str!("render/plan.rs")),
        ("render/registry.rs", include_str!("render/registry.rs")),
        ("render/scene.rs", include_str!("render/scene.rs")),
        ("render/shapes.rs", include_str!("render/shapes.rs")),
        ("render/sketch.rs", include_str!("render/sketch.rs")),
        ("render/snapshot.rs", include_str!("render/snapshot.rs")),
        ("interaction/mod.rs", include_str!("interaction/mod.rs")),
        ("interaction/state.rs", include_str!("interaction/state.rs")),
        ("interaction/tool.rs", include_str!("interaction/tool.rs")),
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
        ("runtime/selection.rs", include_str!("runtime/selection.rs")),
        ("runtime/world.rs", include_str!("runtime/world.rs")),
        ("scenes.rs", include_str!("scenes.rs")),
        ("services/mod.rs", include_str!("services/mod.rs")),
        (
            "services/document_store.rs",
            include_str!("services/document_store.rs"),
        ),
        ("spatial/mod.rs", include_str!("spatial/mod.rs")),
        ("spatial/grid.rs", include_str!("spatial/grid.rs")),
        ("spatial/index.rs", include_str!("spatial/index.rs")),
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
