//! The Flow Canvas: an infinite canvas and node-graph engine — React-Flow-style
//! graphs, Excalidraw-style drawing and a hand-drawn render mode — built
//! directly on GPUI, with no WebView and no foreign UI framework.
//!
//! **Nothing here reaches the running app yet** — by design. The canvas is
//! built in phases, each one buildable, testable and reviewable on its own, and
//! the sidebar row (Phase 8) is deliberately held until last so nobody meets a
//! half-built tool. It was eight phases when this file was started; the captain
//! reviewed the running canvas after Phase 7.5 and specified a second scope, so
//! Phases 9 to 12 — editing, text, the property panel, images — now come first
//! and the row lands after them. The per-slice sections below are the record,
//! in order. Until the row lands, the canvas runs through its own launcher:
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
//!   the selected-or-hovered node, a selection ring and a toolbar.
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
//!   `Delete`, `Backspace` and the toolbar's action are the same method, which
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
pub mod render;
pub mod runtime;
pub mod scenes;
pub mod spatial;
pub mod views;

pub use budgets::RenderBudgets;
pub use commands::{CommandHistory, EditCommand, EditError, FlowEditor, NodeDraft};
pub use geometry::{Rect, Vec2, Viewport};
pub use instrument::{Instruments, Probe};
pub use interaction::{InteractionEffect, InteractionEvent, InteractionMachine, InteractionState};
pub use models::{ElementId, ElementKind, FlowDocument};
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
        ("render/cache.rs", include_str!("render/cache.rs")),
        ("render/edges.rs", include_str!("render/edges.rs")),
        ("render/grid.rs", include_str!("render/grid.rs")),
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
