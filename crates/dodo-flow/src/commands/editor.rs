//! [`FlowEditor`] — the world, the history, and **no way to reach one without
//! the other**.
//!
//! # The invariant, stated exactly
//!
//! > Every change that [`GraphWorld::to_document`] can observe in a node or an
//! > edge goes through [`FlowEditor::apply`].
//!
//! It is worth stating that precisely rather than as "everything goes through
//! commands", because two things deliberately do not and the difference is not
//! arbitrary:
//!
//! - **Selection is not document state.** [`to_document`](GraphWorld::to_document)
//!   does not write it, a reload does not restore it, and §30's command list
//!   contains nothing about it. So [`select_only`](FlowEditor::select_only) and
//!   its siblings mutate the world directly and record nothing, and a click
//!   does not become an undo step that swallows the one before it.
//! - **Document settings are the document's, but not an element's.** The render
//!   style, the sketch parameters and the routing options are per-document
//!   knobs rather than per-element edits; §30's list has no command for them,
//!   and the launcher's clean/sketch toggle would otherwise fill the undo stack
//!   with view-mode changes. They are named methods here, enumerable in one
//!   place, rather than a `settings_mut` that hands out the whole struct.
//!
//! `to_document`'s *elements* are the invariant's subject, and
//! `commands::tests` asserts it by walking a document through every
//! non-recording door and finding the nodes and edges unchanged.
//!
//! # How the bypass is made unexpressible
//!
//! This type owns its [`GraphWorld`] in a private field and **never returns
//! `&mut` to it.** That is the whole enforcement, and it is a stronger one than
//! a convention because a caller cannot write the mistake: there is no
//! reference to mutate through. [`FlowView`](crate::views::FlowView) holds a
//! `FlowEditor` rather than a `GraphWorld` for exactly that reason — before this
//! phase it held the world and called `move_node` on it from a mouse handler.
//!
//! The tests at the bottom of this file read this file's own source and fail if
//! a `&mut GraphWorld` ever escapes, in the same spirit as `lib.rs`'s
//! `the_pure_layers_name_no_ui_framework`: the line is easy to hold and very
//! hard to notice being crossed.
//!
//! The frame's *derivation* calls — rebuilding stale routes, draining the
//! spatial queues — do take `&mut self` and are passed through, because they
//! change nothing a document can see. They recompute what an edit already
//! invalidated.
//!
//! # Why undo restores the derived state without knowing about it
//!
//! An undo is an edit. It goes through the same [`apply`], which calls the same
//! `GraphWorld` mutators, which run §19's propagation: the node's dirty flags,
//! its spatial queue entry, its incident edges' geometry through the adjacency
//! index. So the spatial index, the route store and the geometry cache come
//! back not because undo restores them but because **undo never had a different
//! path to begin with**. That is the single reason this design was worth the
//! phase; `commands::tests` asserts it as a repaint-equivalence property rather
//! than trusting the argument.
//!
//! **This file names no UI framework.**

use std::sync::Arc;

use crate::{
    commands::{
        apply::{EditOutcome, apply},
        edit::{EditCommand, EditError, NodeDraft},
        history::{CommandHistory, GestureId},
        layers::{DepthSpan, LayerAction},
    },
    geometry::{RouteOptions, Vec2},
    interaction::TextTarget,
    models::{
        DocumentSettings, EdgeIndex, EdgeRouting, ElementId, ElementKind, ElementStyle,
        FlowDocument, ImageCrop, ImageResource, NodeImage, NodeIndex, RenderStyle, SketchStyle,
    },
    properties::{CropChoice, crop_choice},
    runtime::{
        BoxQuery, ConnectionRules, DirtyState, EdgeSpec, GraphWorld, LoadReport, NodeSpec,
        PointerTarget,
    },
};

/// How far a duplicate lands from what it copied, in world units.
///
/// Down and to the right, and far enough that the copy is visibly a second
/// object rather than a smear on the first — a duplicate placed exactly on top
/// of its original is indistinguishable from nothing having happened, which is
/// how somebody presses the button four times and then deletes what they think
/// is one shape.
const DUPLICATE_OFFSET: Vec2 = Vec2::new(12.0, 12.0);

/// What an applied edit tells its caller.
///
/// [`EditOutcome`]'s inverse is not here: the history took it, and a caller
/// that could see it could apply it, which is the bypass again by another door.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct EditSummary {
    pub added_nodes: Vec<NodeIndex>,
    pub added_edges: Vec<EdgeIndex>,
    /// Whether the world changed. `false` means no undo step was recorded.
    pub changed: bool,
}

/// **A [`GraphWorld`] with a [`CommandHistory`] welded to it.**
///
/// See the module doc for the invariant and for how it is enforced.
#[derive(Debug, Clone, Default)]
pub struct FlowEditor {
    world: GraphWorld,
    history: CommandHistory,
    /// Monotonic stamp for the persisted document. Selection and derived
    /// geometry leave it alone; every document write moves it.
    revision: u64,
}

impl FlowEditor {
    pub fn new() -> FlowEditor {
        FlowEditor {
            world: GraphWorld::new(),
            history: CommandHistory::new(),
            revision: 0,
        }
    }

    /// Builds an editor over a document, with an empty history — nothing that
    /// happened before the file was opened is undoable.
    pub fn from_document(document: &FlowDocument) -> (FlowEditor, LoadReport) {
        let (world, report) = GraphWorld::from_document(document);
        (
            FlowEditor {
                world,
                history: CommandHistory::new(),
                revision: 0,
            },
            report,
        )
    }

    /// Replaces the whole document, **and clears the history**.
    ///
    /// A stored delta names runtime indices, and every index means something
    /// different in a different document. Keeping the stack across a load would
    /// be the corruption this phase exists to prevent, arriving through the
    /// front door.
    pub fn load_document(&mut self, document: FlowDocument) -> LoadReport {
        let (world, report) = GraphWorld::from_document(&document);
        self.world = world;
        self.history.clear();
        self.bump_revision();
        report
    }

    // ---- reading ---------------------------------------------------------

    /// The world, **shared**. There is no `&mut` counterpart; see the module
    /// doc.
    pub fn world(&self) -> &GraphWorld {
        &self.world
    }

    pub fn history(&self) -> &CommandHistory {
        &self.history
    }

    pub fn to_document(&self) -> FlowDocument {
        self.world.to_document()
    }

    /// Changes only when the serialized document can have changed. The app's
    /// persistence seam uses this to avoid copying a whole canvas from a
    /// `render` that may run for an unrelated ancestor repaint.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    fn bump_revision(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }

    pub fn can_undo(&self) -> bool {
        self.history.can_undo()
    }

    pub fn can_redo(&self) -> bool {
        self.history.can_redo()
    }

    // ---- editing ---------------------------------------------------------

    /// **The one door.** Applies a delta and records the step that undoes it.
    ///
    /// A command that changes nothing records nothing, so a keystroke that was
    /// a no-op does not consume an undo press later.
    pub fn apply(&mut self, command: EditCommand) -> Result<EditSummary, EditError> {
        let redo = command.clone();
        let EditOutcome {
            inverse,
            added_nodes,
            added_edges,
            changed,
        } = apply(&mut self.world, command)?;

        if changed {
            self.history.push(redo, inverse);
            self.bump_revision();
        }

        Ok(EditSummary {
            added_nodes,
            added_edges,
            changed,
        })
    }

    /// **Removes whatever is selected**, and says whether anything went.
    ///
    /// The whole of `Delete` and `Backspace`, and of the tool palette's Delete
    /// action beside them, is this one method: read §28's selection, hand it to §30's
    /// `SetPresence`, and let the one applier do the rest. It is here rather
    /// than in the view because the selection and the history are both this
    /// type's, and a view that assembled the command itself would be the
    /// second place a removal could be got wrong.
    ///
    /// **It is written to survive the next three phases without changing.**
    /// A selection is a set of node and edge indices and nothing else — it does
    /// not know what kind an element is — so a text element (Phase 10) and an
    /// image (Phase 12) are deleted by this method the day they can be
    /// selected, with no line here to add. That is the same property that makes
    /// [`EditCommand::SetPresence`] kind-blind, and it is the reason the
    /// removal was not written as "delete the selected *nodes*".
    ///
    /// **What it does not do is spelled out, because it is easy to add by
    /// mistake**: it does not clear the selection afterwards. Removal already
    /// deselects each element as it goes
    /// ([`GraphWorld::remove_node`](crate::runtime::GraphWorld::remove_node)),
    /// and an extra clear here would deselect the elements an *undo* is about
    /// to bring back — which is how a restored element comes back invisible to
    /// every control that reads the selection.
    ///
    /// One edit and therefore one undo step, however many elements went, and
    /// the incident edges of a removed node go with it because the applier
    /// records the cascade rather than the request.
    pub fn delete_selection(&mut self) -> bool {
        let nodes = self.world.selection().nodes().to_vec();
        let edges = self.world.selection().edges().to_vec();
        if nodes.is_empty() && edges.is_empty() {
            return false;
        }

        self.apply(EditCommand::remove(nodes, edges))
            .is_ok_and(|summary| summary.changed)
    }

    /// **The text a target currently holds**, so an editor can be seeded with
    /// it (§9).
    ///
    /// This is the whole of "existing text must be editable again": without it
    /// the only honest editor is a blank one, and a blank one *replaces* rather
    /// than edits. `None` for a target that has no text yet — a pending
    /// element, or an element nobody has typed into — which the caller shows as
    /// an empty field with a placeholder rather than as the word "None".
    /// **Liveness, not mere presence.** A removed element keeps its whole row —
    /// removal is a tombstone, so an undo entry can still name it — and its
    /// words are still sitting in that row. Reading them back would let a caret
    /// open on a deleted element and, worse, would make "is there text here?"
    /// answer yes for something nobody can see. `commit_text` refuses the same
    /// targets, and the two agree because they ask the same question.
    pub fn text_of(&self, target: TextTarget) -> Option<&str> {
        match target {
            TextTarget::Node(node) if self.world.node_is_live(node) => {
                self.world.nodes().cold(node).label.as_deref()
            }
            TextTarget::Edge(edge) if self.world.edge_is_live(edge) => {
                self.world.edges().label(edge).map(Arc::as_ref)
            }
            _ => None,
        }
    }

    /// **§30's `EditText`, through the one door** (§9).
    ///
    /// Three targets, one undo step each, and every one of them an ordinary
    /// [`apply`](FlowEditor::apply) — so an undone text edit restores the
    /// label, the node's version, its geometry-cache key and its shaped line
    /// with nothing here knowing those exist.
    ///
    /// **Empty is not "no change", and the three targets disagree about what it
    /// means**, which is exactly why this decision is here rather than in the
    /// view:
    ///
    /// - On a **node** or an **edge**, empty *clears* the label. That is an
    ///   edit and it is undoable; a user who selects all and deletes has said
    ///   something.
    /// - On a node whose kind is [`ElementKind::Text`], empty **removes the
    ///   element**. A text element is its glyphs, so an empty one is invisible,
    ///   unselectable-by-eye and impossible to find again — the exact
    ///   "control that produces nothing" failure Phase 7.5 recorded, arriving
    ///   through an edit instead of through a palette.
    /// - On a **pending** element, empty does nothing at all, because nothing
    ///   was ever created. That is what makes an abandoned Text-tool gesture
    ///   free rather than a create-then-delete pair on the undo stack.
    ///
    /// Answers whether the document changed, so a caller can decide about
    /// repainting without diffing.
    pub fn commit_text(&mut self, target: TextTarget, text: &str) -> bool {
        let text = text.trim();
        let value = (!text.is_empty()).then(|| text.to_owned());

        match target {
            TextTarget::Node(node) => {
                if !self.world.nodes().contains(node) || !self.world.nodes().is_live(node) {
                    return false;
                }
                // Emptying a *text element* is deleting it. `SetPresence` rather
                // than a bare label clear, so the node's edges — a text element
                // has none today, and §11's frames will — cascade the same way
                // every other removal does.
                if value.is_none() && *self.world.nodes().kind(node) == ElementKind::Text {
                    return self
                        .apply(EditCommand::remove(vec![node], Vec::new()))
                        .is_ok_and(|summary| summary.changed);
                }
                self.apply(EditCommand::SetNodeLabels(vec![(node, value)]))
                    .is_ok_and(|summary| summary.changed)
            }
            TextTarget::Edge(edge) => {
                if !self.world.edges().contains(edge) || !self.world.edges().is_live(edge) {
                    return false;
                }
                self.apply(EditCommand::SetEdgeLabels(vec![(edge, value)]))
                    .is_ok_and(|summary| summary.changed)
            }
            TextTarget::New(rect) => {
                let Some(value) = value else {
                    // Nothing was created, so there is nothing to undo — see
                    // this method's doc, and `CanvasTool::Text`'s.
                    return false;
                };
                let rect = rect.normalized();
                let mut spec =
                    NodeSpec::new(ElementId::NONE, ElementKind::Text, rect.origin, rect.size);
                spec.label = Some(value);
                spec.style = self.world.settings().default_style.clone();

                let Ok(summary) = self.apply(EditCommand::AddNodes(vec![NodeDraft::new(spec)]))
                else {
                    return false;
                };
                // Selecting what was just typed is not an edit — see
                // `create` in `commands::gesture`, which does the same thing
                // for every other tool — so the creation stays one undo step.
                if let Some(node) = summary.added_nodes.first().copied() {
                    self.select_only(Some(node));
                }
                summary.changed
            }
        }
    }

    /// **Inserts §10's picture**, centred on `center`, sized to fit `room`, and
    /// selects it.
    ///
    /// Two things happen and only one of them is an edit, which is the part
    /// worth reading carefully:
    ///
    /// - **The bytes are registered on the world and recorded nowhere.** See
    ///   [`GraphWorld::insert_image`](crate::runtime::GraphWorld::insert_image):
    ///   a resource nothing references is written to no file, so registering
    ///   one changes nothing [`to_document`](FlowEditor::to_document) can
    ///   observe — and un-registering it on undo would break the redo that is
    ///   about to name it.
    /// - **The element is an ordinary `AddNodes`.** So one press of undo
    ///   removes the picture, one press of redo brings it back at its own
    ///   index, and neither line here knows that a file picker exists.
    ///
    /// Inserting the *same file* twice produces two elements sharing one
    /// resource, because the handle is a content hash — see
    /// [`image`](crate::models::image).
    ///
    /// Answers the new element, or `None` if the applier refused it.
    pub fn insert_image(
        &mut self,
        resource: ImageResource,
        center: Vec2,
        room: Vec2,
    ) -> Option<NodeIndex> {
        let size = resource.placed_size(room);
        let handle = self.world.insert_image(resource);

        let mut spec = NodeSpec::new(
            ElementId::NONE,
            ElementKind::Image,
            center - Vec2::new(size.x * 0.5, size.y * 0.5),
            size,
        );
        spec.image = Some(NodeImage::new(handle));
        spec.style = self.world.settings().default_style.clone();

        let summary = self
            .apply(EditCommand::AddNodes(vec![NodeDraft::new(spec)]))
            .ok()?;
        let node = summary.added_nodes.first().copied()?;
        // Selecting what was just inserted is not an edit — the same rule
        // `commit_text` and `commands::gesture` follow — so the insertion stays
        // one undo step.
        self.select_only(Some(node));
        Some(node)
    }

    /// The picture an element is showing, if it is showing one. **Liveness, not
    /// presence**, for the reason [`text_of`](FlowEditor::text_of) gives.
    pub fn image_of(&self, node: NodeIndex) -> Option<NodeImage> {
        self.world
            .node_is_live(node)
            .then(|| self.world.nodes().cold(node).image)
            .flatten()
    }

    /// **What the panel's Crop button would do**, or `None` when it would do
    /// nothing and the panel should mute it.
    ///
    /// Read off the *leading* selected image, which is the same answer every
    /// other row on a mixed selection gives — see `properties`' decision 3. A
    /// press still applies to every selected image, each on its own terms.
    pub fn selection_crop(&self) -> Option<CropChoice> {
        self.world
            .selection()
            .nodes()
            .iter()
            .copied()
            .find_map(|node| self.crop_choice_for(node))
            .map(|(_, choice)| choice)
    }

    /// One element's Crop decision, with the source aspect the caller needs to
    /// carry out.
    fn crop_choice_for(&self, node: NodeIndex) -> Option<(f32, CropChoice)> {
        let image = self.image_of(node)?;
        let source = self.world.image(image.handle)?.aspect();
        let size = self.world.nodes().size(node);
        let frame = size.x / size.y;
        crop_choice(source, frame, image.crop).map(|choice| (source, choice))
    }

    /// **§10's crop, through the one door** — the panel's Crop action.
    ///
    /// Every selected image decides for itself
    /// ([`crop_choice`](crate::properties::crop_choice)), because a selection
    /// holding a stretched picture and a cropped one has no single honest
    /// answer and each of them has an obvious one. One gesture, so however many
    /// pictures were selected, one press of undo puts them all back.
    ///
    /// **No pixels are touched and no resource is written.** A crop is four
    /// numbers on the element; the bytes it is a window on are shared, and a
    /// second element showing the same picture is unaffected.
    pub fn crop_selection(&mut self) -> bool {
        let decisions: Vec<(NodeIndex, f32, CropChoice)> = self
            .world
            .selection()
            .nodes()
            .iter()
            .copied()
            .filter_map(|node| {
                self.crop_choice_for(node)
                    .map(|(source, choice)| (node, source, choice))
            })
            .collect();

        if decisions.is_empty() {
            return false;
        }

        let mut images: Vec<(NodeIndex, Option<NodeImage>)> = Vec::new();
        let mut sizes: Vec<(NodeIndex, Vec2)> = Vec::new();

        for (node, source, choice) in decisions {
            let Some(image) = self.image_of(node) else {
                continue;
            };
            let size = self.world.nodes().size(node);

            match choice {
                CropChoice::ToFrame => {
                    let frame = size.x / size.y;
                    images.push((
                        node,
                        Some(image.with_crop(image.crop.cropped_to_aspect(source, frame))),
                    ));
                }
                // **The frame follows the picture back.** Restoring the whole
                // source into a frame shaped like the crop would un-stretch
                // nothing — it would stretch the picture the other way — so the
                // height is recomputed from the source's own ratio and the
                // width, which is the dimension the user placed, is kept.
                CropChoice::Reset => {
                    images.push((node, Some(image.with_crop(ImageCrop::FULL))));
                    sizes.push((node, Vec2::new(size.x, size.x / source.max(f32::EPSILON))));
                }
            }
        }

        self.in_one_step(|editor| {
            let mut changed = editor
                .apply(EditCommand::SetNodeImages(images))
                .is_ok_and(|summary| summary.changed);
            changed |= editor
                .apply(EditCommand::ResizeNodes(sizes))
                .is_ok_and(|summary| summary.changed);
            changed
        })
    }

    /// **Restyles whatever is selected, through the one door** — every control
    /// on the property panel's style rows.
    ///
    /// The panel hands in a closure over an [`ElementStyle`] rather than a
    /// finished style, and that is the whole reason a fifteen-row panel needs
    /// no fifteen methods here: a row knows one field, the editor knows the
    /// selection and the history, and neither has to learn the other's job.
    ///
    /// **One gesture, and therefore one undo step**, however many elements are
    /// selected and however the selection splits between nodes and edges —
    /// otherwise a mixed selection restyled once would take two presses of undo
    /// to put back, which is the sort of thing nobody notices until they are
    /// annoyed by it.
    ///
    /// Answers whether the document changed, so a caller can decide about
    /// repainting without diffing. A control set to the value it already holds
    /// changes nothing, records nothing and consumes no undo.
    pub fn restyle_selection(&mut self, mut edit: impl FnMut(&mut ElementStyle)) -> bool {
        let nodes: Vec<(NodeIndex, ElementStyle)> = self
            .world
            .selection()
            .nodes()
            .iter()
            .filter(|&&node| self.world.node_is_live(node))
            .map(|&node| {
                let mut style = self.world.nodes().style(node).clone();
                edit(&mut style);
                (node, style)
            })
            .collect();

        let edges: Vec<(EdgeIndex, ElementStyle)> = self
            .world
            .selection()
            .edges()
            .iter()
            .filter(|&&edge| self.world.edge_is_live(edge))
            .map(|&edge| {
                let mut style = self.world.edges().style(edge).clone();
                edit(&mut style);
                (edge, style)
            })
            .collect();

        self.in_one_step(|editor| {
            let mut changed = editor
                .apply(EditCommand::SetNodeStyles(nodes))
                .is_ok_and(|summary| summary.changed);
            changed |= editor
                .apply(EditCommand::SetEdgeStyles(edges))
                .is_ok_and(|summary| summary.changed);
            changed
        })
    }

    /// **§8's routing, for the panel's Arrow type row.** Edges only — a node
    /// has no route — so a selection with no edge in it changes nothing.
    pub fn reroute_selection(&mut self, routing: EdgeRouting) -> bool {
        let edges: Vec<(EdgeIndex, EdgeRouting)> = self
            .world
            .selection()
            .edges()
            .iter()
            .filter(|&&edge| self.world.edge_is_live(edge))
            .map(|&edge| (edge, routing))
            .collect();

        self.apply(EditCommand::SetEdgeRouting(edges))
            .is_ok_and(|summary| summary.changed)
    }

    /// **Moves the selection through the paint order** — the panel's Layers
    /// row, and the one genuinely new thing this phase adds to the document.
    ///
    /// The arithmetic is [`LayerAction::shift`]'s and is asserted with no world
    /// at all; what is here is the part that needs one. It walks the **live**
    /// elements once, folding two [`DepthSpan`]s — the selection's depths and
    /// everything else's, the latter measured against the former's interval —
    /// and then shifts every selected element by the one number that falls out.
    ///
    /// **One walk of the document per press, and none per frame.** That is the
    /// trade this is written around: §40 rule 1 forbids a per-frame scan, and a
    /// button press is not a frame. The alternative — keeping a sorted depth
    /// index up to date — would be a second structure to invalidate on every
    /// edit, for a question asked four times a session.
    ///
    /// One gesture, so a multiple selection is one undo step.
    pub fn reorder_selection(&mut self, action: LayerAction) -> bool {
        let selected_nodes: Vec<NodeIndex> = self
            .world
            .selection()
            .nodes()
            .iter()
            .copied()
            .filter(|&node| self.world.node_is_live(node))
            .collect();
        let selected_edges: Vec<EdgeIndex> = self
            .world
            .selection()
            .edges()
            .iter()
            .copied()
            .filter(|&edge| self.world.edge_is_live(edge))
            .collect();

        if selected_nodes.is_empty() && selected_edges.is_empty() {
            return false;
        }

        // Pass one: the selection's own interval, which pass two is measured
        // against. Two passes rather than one because `observe` needs the
        // interval up front and the interval is what the first pass computes.
        let mut selection = DepthSpan::EMPTY;
        for &node in &selected_nodes {
            selection.observe(self.world.nodes().z(node), i32::MAX, i32::MIN);
        }
        for &edge in &selected_edges {
            selection.observe(self.world.edges().z(edge), i32::MAX, i32::MIN);
        }
        let (Some(low), Some(high)) = (selection.min, selection.max) else {
            return false;
        };

        let mut others = DepthSpan::EMPTY;
        for node in self.world.nodes().live_indices() {
            if !selected_nodes.contains(&node) {
                others.observe(self.world.nodes().z(node), low, high);
            }
        }
        for edge in self.world.edges().live_indices() {
            if !selected_edges.contains(&edge) {
                others.observe(self.world.edges().z(edge), low, high);
            }
        }

        let shift = action.shift(selection, others);
        if shift == 0 {
            return false;
        }

        let nodes: Vec<(NodeIndex, i32)> = selected_nodes
            .iter()
            .map(|&node| (node, self.world.nodes().z(node).saturating_add(shift)))
            .collect();
        let edges: Vec<(EdgeIndex, i32)> = selected_edges
            .iter()
            .map(|&edge| (edge, self.world.edges().z(edge).saturating_add(shift)))
            .collect();

        self.in_one_step(|editor| {
            let mut changed = editor
                .apply(EditCommand::SetNodeZ(nodes))
                .is_ok_and(|summary| summary.changed);
            changed |= editor
                .apply(EditCommand::SetEdgeZ(edges))
                .is_ok_and(|summary| summary.changed);
            changed
        })
    }

    /// **The hyperlink on whatever is selected**, or `None` when the selection
    /// is empty, holds more than one element, or holds one with no link.
    ///
    /// One element rather than a set, because a link is a value a control shows
    /// and edits — and there is no honest thing to show for two elements with
    /// two different links. The panel's Link button is drawn as "set" from this.
    pub fn selection_link(&self) -> Option<&str> {
        let selection = self.world.selection();
        match (selection.nodes(), selection.edges()) {
            ([node], []) if self.world.node_is_live(*node) => {
                self.world.nodes().cold(*node).link.as_deref()
            }
            ([], [edge]) if self.world.edge_is_live(*edge) => self.world.edges().link(*edge),
            _ => None,
        }
    }

    /// **The hyperlink on whatever a press landed on**, or `None`.
    ///
    /// Here rather than in the view because it is the same question
    /// [`selection_link`](FlowEditor::selection_link) asks from the other end,
    /// and two places that read a link out of a target are two places that can
    /// disagree about a tombstone. A handle answers its node's link: the handle
    /// is part of the node as far as a user pressing it is concerned.
    ///
    /// **Liveness, not presence** — the same rule
    /// [`text_of`](FlowEditor::text_of) follows, and for the same reason: a
    /// removed element keeps its whole row, and following a link on something
    /// nobody can see is worse than finding none.
    pub fn link_at(&self, target: PointerTarget) -> Option<&str> {
        match target {
            PointerTarget::Node(node) | PointerTarget::Handle { node, .. }
                if self.world.node_is_live(node) =>
            {
                self.world.nodes().cold(node).link.as_deref()
            }
            PointerTarget::Edge(edge) if self.world.edge_is_live(edge) => {
                self.world.edges().link(edge)
            }
            _ => None,
        }
        .filter(|link| !link.trim().is_empty())
    }

    /// Sets or clears the hyperlink on every selected element. An empty string
    /// clears, exactly as an empty text commit does — see
    /// [`commit_text`](FlowEditor::commit_text) for why "empty" is a decision
    /// this type makes rather than the view.
    pub fn set_selection_link(&mut self, link: &str) -> bool {
        let link = link.trim();
        let link = (!link.is_empty()).then(|| link.to_owned());

        let nodes: Vec<(NodeIndex, Option<String>)> = self
            .world
            .selection()
            .nodes()
            .iter()
            .filter(|&&node| self.world.node_is_live(node))
            .map(|&node| (node, link.clone()))
            .collect();
        let edges: Vec<(EdgeIndex, Option<String>)> = self
            .world
            .selection()
            .edges()
            .iter()
            .filter(|&&edge| self.world.edge_is_live(edge))
            .map(|&edge| (edge, link.clone()))
            .collect();

        self.in_one_step(|editor| {
            let mut changed = editor
                .apply(EditCommand::SetNodeLinks(nodes))
                .is_ok_and(|summary| summary.changed);
            changed |= editor
                .apply(EditCommand::SetEdgeLinks(edges))
                .is_ok_and(|summary| summary.changed);
            changed
        })
    }

    /// **Duplicates the selection**, offset by `DUPLICATE_OFFSET`, and
    /// selects the copies.
    ///
    /// Three decisions, each of which is visible the first time somebody uses
    /// it:
    ///
    /// - **An edge is copied only when both of its endpoints are.** A duplicate
    ///   of one end of a connection is not a copy of anything; it would attach
    ///   the new edge to the *original* node and leave two edges converging on
    ///   it, which is never what the button meant.
    /// - **Handles are copied with their node**, so a duplicated graph node is
    ///   connectable in the same places the original is. Without them the copy
    ///   looks identical and cannot be wired up.
    /// - **The copies become the selection**, so a second press duplicates the
    ///   copy and walks the offset across the canvas, which is what every
    ///   editor with this button does. Selecting is not an edit — see
    ///   `commands::gesture` — so the duplication stays one undo step.
    pub fn duplicate_selection(&mut self) -> bool {
        let sources: Vec<NodeIndex> = self
            .world
            .selection()
            .nodes()
            .iter()
            .copied()
            .filter(|&node| self.world.node_is_live(node))
            .collect();
        if sources.is_empty() {
            return false;
        }

        let drafts: Vec<NodeDraft> = sources
            .iter()
            .map(|&node| {
                let cold = self.world.nodes().cold(node);
                let spec = NodeSpec {
                    id: ElementId::NONE,
                    kind: cold.kind.clone(),
                    position: self.world.nodes().position(node) + DUPLICATE_OFFSET,
                    size: self.world.nodes().size(node),
                    z: self.world.nodes().z(node),
                    style: self.world.nodes().style(node).clone(),
                    label: cold.label.as_deref().map(str::to_owned),
                    parent: None,
                    link: cold.link.clone(),
                    // **The handle, not the bytes** — §10's rule, and the
                    // reason a duplicated photograph costs sixteen bytes. See
                    // [`GraphWorld::insert_image`](crate::runtime::GraphWorld::insert_image).
                    image: cold.image,
                    hidden: self.world.nodes().is_hidden(node),
                    locked: self.world.nodes().is_locked(node),
                };
                NodeDraft::new(spec).with_handles(
                    self.world
                        .nodes()
                        .handles(node)
                        .map(|handle| self.world.handles().spec(handle))
                        .collect(),
                )
            })
            .collect();

        // Every edge whose *both* ends are being copied, with the ends
        // remembered as offsets into `sources` — the new indices are not known
        // until the nodes have been added.
        let joins: Vec<(usize, usize, EdgeIndex)> = self
            .world
            .edges()
            .live_indices()
            .filter_map(|edge| {
                let source = self.world.edges().source(edge).node;
                let target = self.world.edges().target(edge).node;
                let from = sources.iter().position(|&node| node == source)?;
                let to = sources.iter().position(|&node| node == target)?;
                Some((from, to, edge))
            })
            .collect();

        self.in_one_step(|editor| {
            let Ok(summary) = editor.apply(EditCommand::AddNodes(drafts)) else {
                return false;
            };
            let added = summary.added_nodes;
            if added.len() != sources.len() {
                // A partial add cannot happen through `AddNodes`, but the copy
                // below indexes into `added` and a wrong length would panic.
                return summary.changed;
            }

            let specs: Vec<EdgeSpec> = joins
                .iter()
                .map(|&(from, to, edge)| EdgeSpec {
                    id: ElementId::NONE,
                    source: editor.copied_end(editor.world.edges().source(edge), added[from]),
                    target: editor.copied_end(editor.world.edges().target(edge), added[to]),
                    routing: editor.world.edges().routing(edge),
                    style: editor.world.edges().style(edge).clone(),
                    label: editor.world.edges().label(edge).map(|it| it.to_string()),
                    link: editor.world.edges().link(edge).map(str::to_owned),
                    z: editor.world.edges().z(edge),
                    hidden: editor.world.edges().is_hidden(edge),
                })
                .collect();
            let _ = editor.apply(EditCommand::Connect(specs));

            editor.world.clear_selection();
            for node in added {
                editor.world.set_node_selected(node, true);
            }
            true
        })
    }

    /// One end of a copied edge: the same handle *slot* on the copied node.
    ///
    /// A handle is identified by index in the world's arena, so the original's
    /// index means nothing on the copy; the position in the node's own handle
    /// list is what carries over, and a duplicated node's handles were pushed
    /// in the same order.
    fn copied_end(&self, end: crate::runtime::EdgeEnd, node: NodeIndex) -> crate::runtime::EdgeEnd {
        let Some(handle) = end.handle.get() else {
            return crate::runtime::EdgeEnd::node(node);
        };
        let slot = self
            .world
            .nodes()
            .handles(end.node)
            .position(|it| it == handle);
        match slot.and_then(|slot| self.world.nodes().handles(node).nth(slot)) {
            Some(copied) => crate::runtime::EdgeEnd::handle(node, copied),
            None => crate::runtime::EdgeEnd::node(node),
        }
    }

    /// Runs `body` inside one gesture, so everything it applies undoes together.
    ///
    /// A shared helper rather than a pattern each method repeats: the pairing
    /// is the whole correctness of "one press, one undo", and a method that
    /// returned early between the two calls would leave a gesture open for the
    /// next unrelated edit to join.
    ///
    /// **It nests**, and it has to. [`begin_gesture`](FlowEditor::begin_gesture)
    /// was already re-entrant — it hands back the gesture that is open rather
    /// than starting a second — but [`end_gesture`](FlowEditor::end_gesture)
    /// closes unconditionally, so a helper that always closed would end the
    /// caller's gesture on its way out. That is not theoretical: a slider drag
    /// opens a gesture and then calls `restyle_selection` sixty times, and
    /// without this the first tick would close the drag and the other
    /// fifty-nine would each become an undo step of their own. It was found by
    /// counting the entries in a test rather than by reading this.
    pub(crate) fn in_one_step(&mut self, body: impl FnOnce(&mut FlowEditor) -> bool) -> bool {
        let outer = self.history.open_gesture();
        self.begin_gesture();
        let changed = body(self);
        if outer.is_none() {
            self.end_gesture();
        }
        changed
    }

    /// Opens a gesture: every edit until [`end_gesture`](FlowEditor::end_gesture)
    /// is one undo step. A node drag calls this on the press and closes it on
    /// the release.
    pub fn begin_gesture(&mut self) -> GestureId {
        self.history.begin_gesture()
    }

    pub fn end_gesture(&mut self) {
        self.history.end_gesture();
    }

    /// **Takes one step back.** Answers whether anything moved, so a caller can
    /// decide whether the frame needs repainting without diffing.
    ///
    /// Each entry's undo delta goes through the same [`apply`] as an ordinary
    /// edit — which is what makes the spatial index, the route store and the
    /// dirty flags come back with the document rather than beside it.
    pub fn undo(&mut self) -> bool {
        // A gesture left open by an interrupted drag must not swallow the undo
        // into itself; close it first, so the step is taken against a settled
        // stack.
        self.history.end_gesture();

        let step = self.history.take_undo();
        let mut changed = false;
        for mut entry in step {
            let outcome = apply(&mut self.world, entry.undo.clone())
                .expect("a recorded inverse is always applicable");
            changed |= outcome.changed;
            // The redo side is replaced by the exact inverse of what was just
            // applied, rather than trusting the command as it was submitted:
            // the applier filters a delta to what it could really touch, and
            // this is where the entry becomes canonical.
            entry.redo = outcome.inverse;
            self.history.record_undone(entry);
        }
        if changed {
            self.bump_revision();
        }
        changed
    }

    /// Takes one step forward again.
    pub fn redo(&mut self) -> bool {
        self.history.end_gesture();

        let step = self.history.take_redo();
        let mut changed = false;
        for mut entry in step {
            let outcome = apply(&mut self.world, entry.redo.clone())
                .expect("a recorded inverse is always applicable");
            changed |= outcome.changed;
            entry.undo = outcome.inverse;
            self.history.record_redone(entry);
        }
        if changed {
            self.bump_revision();
        }
        changed
    }

    /// **Throws away an abandoned gesture**, putting the world back where the
    /// gesture found it.
    ///
    /// For `Esc` during a drag. The entries are applied in reverse and then
    /// dropped: a cancelled drag is not an undo step and not a redo step, and
    /// leaving one on the stack would make the next undo walk back through a
    /// move the user already cancelled.
    ///
    /// A gesture that recorded nothing — a click — costs one branch and reaches
    /// nothing below it.
    pub fn abandon_gesture(&mut self) -> bool {
        let mut changed = false;
        for entry in self.history.abandon_gesture() {
            let outcome = apply(&mut self.world, entry.undo)
                .expect("a recorded inverse is always applicable");
            changed |= outcome.changed;
        }
        if changed {
            self.bump_revision();
        }
        changed
    }

    /// Issues a fresh document id, for a caller assembling a draft that wants
    /// to name its own.
    pub fn next_id(&mut self) -> ElementId {
        self.world.next_id()
    }

    // ---- selection: view state, never recorded ---------------------------

    pub fn select_only(&mut self, node: Option<NodeIndex>) {
        self.world.select_only(node);
    }

    pub fn clear_selection(&mut self) {
        self.world.clear_selection();
    }

    pub fn set_node_selected(&mut self, node: NodeIndex, selected: bool) {
        self.world.set_node_selected(node, selected);
    }

    pub fn set_edge_selected(&mut self, edge: EdgeIndex, selected: bool) {
        self.world.set_edge_selected(edge, selected);
    }

    /// §28's box selection, narrow phase. The broad phase is the caller's.
    pub fn apply_box_selection(
        &mut self,
        query: BoxQuery,
        nodes: impl IntoIterator<Item = NodeIndex>,
        edges: impl IntoIterator<Item = EdgeIndex>,
    ) -> u32 {
        self.world.apply_box_selection(query, nodes, edges)
    }

    // ---- document settings: not element edits ----------------------------

    pub fn settings(&self) -> &DocumentSettings {
        self.world.settings()
    }

    pub fn set_render_style(&mut self, style: RenderStyle) {
        if self.world.settings().render_style != style {
            self.world.settings_mut().render_style = style;
            self.bump_revision();
        }
    }

    pub fn set_sketch_style(&mut self, sketch: SketchStyle) {
        if self.world.settings().sketch != sketch {
            self.world.settings_mut().sketch = sketch;
            self.bump_revision();
        }
    }

    pub fn set_route_options(&mut self, options: RouteOptions) {
        if self.world.route_options() != &options {
            self.world.set_route_options(options);
            self.bump_revision();
        }
    }

    pub fn set_rules(&mut self, rules: ConnectionRules) {
        if self.world.rules() != rules {
            self.world.set_rules(rules);
            self.bump_revision();
        }
    }

    // ---- derivation: recomputes, never changes ---------------------------

    /// Rebuilds the routes an edit invalidated. Once at the top of a frame,
    /// before the spatial sync — see [`GraphWorld::clear_spatial_updates`] for
    /// the order and why it is that way round.
    pub fn rebuild_dirty_geometry(&mut self) -> u32 {
        self.world.rebuild_dirty_geometry()
    }

    pub fn rebuild_all_geometry(&mut self) -> u32 {
        self.world.rebuild_all_geometry()
    }

    pub fn clear_spatial_updates(&mut self) {
        self.world.clear_spatial_updates();
    }

    /// The dirty state, for the consumer of an invalidation — the renderer
    /// clearing what it has drawn. Marking something dirty cannot change the
    /// document, so this is derivation rather than an edit.
    pub fn dirty_mut(&mut self) -> &mut DirtyState {
        self.world.dirty_mut()
    }
}

#[cfg(test)]
mod tests {
    use super::FlowEditor;
    use crate::{
        commands::edit::{EditCommand, NodeDraft},
        geometry::Vec2,
        interaction::TextTarget,
        models::{ElementId, ElementKind, NodeIndex, ShapeKind},
        runtime::NodeSpec,
    };

    /// **The enforcement, checked rather than remembered.**
    ///
    /// The whole design rests on there being no way to obtain `&mut GraphWorld`
    /// from an editor: with one, a mouse handler mutates the world and the
    /// history never hears, and the corruption surfaces three undos later with
    /// nothing to trace it to. This is the tripwire that notices the accessor
    /// being added, while deleting it is still a one-line edit.
    #[test]
    fn no_mutable_reference_to_the_world_escapes_the_editor() {
        let source = include_str!("editor.rs");
        for (number, line) in source.lines().enumerate() {
            let code = line.trim_start();
            if code.starts_with("//") {
                continue;
            }
            assert!(
                !code.contains(concat!("&mut ", "GraphWorld")),
                "editor.rs:{} lends the world mutably, which the module doc \
                 forbids:\n  {code}",
                number + 1
            );
        }
    }

    fn editor_with_a_node() -> (FlowEditor, NodeIndex) {
        let mut editor = FlowEditor::new();
        let summary = editor
            .apply(EditCommand::AddNodes(vec![NodeDraft::new(NodeSpec::new(
                ElementId::NONE,
                ElementKind::Shape(ShapeKind::Rectangle),
                Vec2::new(0.0, 0.0),
                Vec2::new(50.0, 30.0),
            ))]))
            .expect("adding a node cannot fail");
        let node = summary.added_nodes[0];
        (editor, node)
    }

    #[test]
    fn the_persistence_revision_moves_only_for_document_changes() {
        let mut editor = FlowEditor::new();
        let initial = editor.revision();
        editor.clear_selection();
        assert_eq!(editor.revision(), initial, "selection is view state");

        let (mut editor, _) = editor_with_a_node();
        let added = editor.revision();
        assert_ne!(added, initial);
        assert!(editor.undo());
        assert_ne!(editor.revision(), added);

        let undone = editor.revision();
        editor.set_render_style(crate::models::RenderStyle::Sketch);
        assert_ne!(editor.revision(), undone);
        let sketch = editor.revision();
        editor.set_render_style(crate::models::RenderStyle::Sketch);
        assert_eq!(editor.revision(), sketch, "an identical write is a no-op");
    }

    // ---- §9's text -----------------------------------------------------

    /// **Existing text is editable again, not merely replaceable.**
    ///
    /// The phase brief calls this out by name, and the failure it prevents is
    /// quiet: an editor that always opened blank looks identical until somebody
    /// double-clicks a label to *read* it, presses Escape, and finds it gone —
    /// or types one correction and loses the rest of the sentence.
    /// `text_of` is what an editor is seeded from, so this is that whole
    /// requirement asserted with no window.
    #[test]
    fn a_label_can_be_typed_edited_and_edited_again() {
        let (mut editor, node) = editor_with_a_node();
        let target = TextTarget::Node(node);

        assert_eq!(editor.text_of(target), None, "nothing typed yet");

        assert!(editor.commit_text(target, "first"));
        assert_eq!(editor.text_of(target), Some("first"));

        // The second edit sees the first — which is the whole point.
        assert!(editor.commit_text(target, "first, corrected"));
        assert_eq!(editor.text_of(target), Some("first, corrected"));

        // Committing the same text again is not an edit.
        assert!(!editor.commit_text(target, "first, corrected"));

        // And clearing is an edit, because a user who selected all and deleted
        // said something.
        assert!(editor.commit_text(target, ""));
        assert_eq!(editor.text_of(target), None);
    }

    /// The same, for an edge — the carrier §9 adds and the one nothing else in
    /// the engine had a door to.
    #[test]
    fn an_edge_label_can_be_typed_edited_and_cleared() {
        use crate::runtime::{ConnectionRules, EdgeEnd};

        let (mut editor, first) = editor_with_a_node();
        editor.set_rules(ConnectionRules::PERMISSIVE);
        let second = editor
            .apply(EditCommand::AddNodes(vec![NodeDraft::new(NodeSpec::new(
                ElementId::NONE,
                ElementKind::Shape(ShapeKind::Rectangle),
                Vec2::new(300.0, 0.0),
                Vec2::new(50.0, 30.0),
            ))]))
            .unwrap()
            .added_nodes[0];
        let edge = editor
            .apply(EditCommand::Connect(vec![crate::runtime::EdgeSpec::new(
                ElementId::NONE,
                EdgeEnd::node(first),
                EdgeEnd::node(second),
            )]))
            .unwrap()
            .added_edges[0];

        let target = TextTarget::Edge(edge);
        assert_eq!(editor.text_of(target), None);
        assert!(editor.commit_text(target, "yes"));
        assert_eq!(editor.text_of(target), Some("yes"));
        assert!(editor.commit_text(target, "no"));
        assert_eq!(editor.text_of(target), Some("no"));
        assert!(editor.commit_text(target, "   "));
        assert_eq!(editor.text_of(target), None, "whitespace is empty");
    }

    /// **Every text edit is one undo step, and undo walks back through them.**
    ///
    /// The property the phase asks for, and the reason `commit_text` is on the
    /// editor rather than in the view: it is an ordinary `apply`, so the label,
    /// the node's text version and its shaped line all come back together with
    /// no line in `commands/` knowing a caret exists.
    #[test]
    fn text_edits_undo_and_redo_one_press_at_a_time() {
        let (mut editor, node) = editor_with_a_node();
        let target = TextTarget::Node(node);
        let depth = editor.history().undo_depth();

        editor.commit_text(target, "one");
        editor.commit_text(target, "two");
        assert_eq!(editor.history().undo_depth(), depth + 2);

        assert!(editor.undo());
        assert_eq!(editor.text_of(target), Some("one"));
        assert!(editor.undo());
        assert_eq!(editor.text_of(target), None);

        assert!(editor.redo());
        assert_eq!(editor.text_of(target), Some("one"));
        assert!(editor.redo());
        assert_eq!(editor.text_of(target), Some("two"));
    }

    /// **Emptying a text element removes it, and one undo brings it back.**
    ///
    /// A text element is its glyphs, so an empty one is invisible — and an
    /// invisible, selectable, undoable element is exactly the failure Phase 7.5
    /// recorded for Line and Arrow. Here it would arrive through an edit rather
    /// than through a palette, which is why the decision is in `commit_text`
    /// and not in whatever widget collected the characters.
    #[test]
    fn emptying_a_text_element_removes_it_and_undo_brings_it_back() {
        let mut editor = FlowEditor::new();
        let pending = TextTarget::New(crate::geometry::Rect::new(
            Vec2::new(10.0, 10.0),
            Vec2::new(200.0, 22.0),
        ));

        assert!(editor.commit_text(pending, "temporary"));
        let node = NodeIndex::new(0);
        assert!(editor.world().nodes().is_live(node));

        assert!(editor.commit_text(TextTarget::Node(node), ""));
        assert!(
            !editor.world().nodes().is_live(node),
            "an empty text element would be invisible, so it goes"
        );

        assert!(editor.undo());
        assert!(editor.world().nodes().is_live(node));
        assert_eq!(editor.text_of(TextTarget::Node(node)), Some("temporary"));

        // Clearing a *labelled shape* is the other answer: the element stays,
        // because a rectangle with no label is still a rectangle.
        let (mut editor, shape) = editor_with_a_node();
        editor.commit_text(TextTarget::Node(shape), "labelled");
        assert!(editor.commit_text(TextTarget::Node(shape), ""));
        assert!(editor.world().nodes().is_live(shape));
    }

    /// A target that has gone away answers rather than panicking. An undo
    /// between the double-click and the commit is an ordinary race, not a bug.
    #[test]
    fn committing_text_to_something_that_is_gone_changes_nothing() {
        let (mut editor, node) = editor_with_a_node();
        editor
            .apply(EditCommand::remove(vec![node], Vec::new()))
            .unwrap();

        assert_eq!(editor.text_of(TextTarget::Node(node)), None);
        assert!(!editor.commit_text(TextTarget::Node(node), "ghost"));
        assert!(!editor.commit_text(TextTarget::Node(NodeIndex::new(99)), "ghost"));
        assert!(!editor.commit_text(TextTarget::Edge(crate::models::EdgeIndex::new(99)), "x"));
    }

    // ---- §10's images -------------------------------------------------

    /// A picture whose "bytes" are a recognisable pattern. Nothing here decodes
    /// them — a decoder is `views/`'s, and every rule this file holds is about
    /// the handle rather than the pixels.
    fn a_picture(width: u32, height: u32, tag: u8) -> crate::models::ImageResource {
        crate::models::ImageResource::new(
            crate::models::ImageFormat::Png,
            width,
            height,
            vec![tag; 64],
        )
    }

    /// **Insertion is one undo step**, and the undo takes the element rather
    /// than the picture: the resource stays in the store so the redo has
    /// something to name.
    #[test]
    fn inserting_an_image_undoes_and_redoes_in_one_press() {
        let mut editor = FlowEditor::new();
        let depth = editor.history().undo_depth();

        let node = editor
            .insert_image(a_picture(400, 200, 1), Vec2::ZERO, Vec2::new(800.0, 600.0))
            .expect("an insert cannot fail on an empty document");

        assert_eq!(editor.history().undo_depth(), depth + 1);
        assert!(editor.world().node_is_live(node));
        assert_eq!(
            editor.world().nodes().size(node),
            Vec2::new(400.0, 200.0),
            "a picture that fits arrives at its own size"
        );
        assert!(editor.image_of(node).is_some());
        assert_eq!(
            editor.world().selection().single_node(),
            Some(node),
            "what was just inserted is what is selected"
        );

        assert!(editor.undo());
        assert!(!editor.world().node_is_live(node));
        assert_eq!(
            editor.world().image_count(),
            1,
            "the bytes must survive an undo, or the redo restores a hole"
        );

        assert!(editor.redo());
        assert!(editor.world().node_is_live(node));
        assert!(editor.image_of(node).is_some());
    }

    /// **Deleting an image is Phase 9's method, unchanged** — which is the
    /// claim that method's own doc made three phases ago, asserted now that
    /// there is an image to make it with.
    #[test]
    fn deleting_an_image_undoes_like_any_other_element() {
        let mut editor = FlowEditor::new();
        let node = editor
            .insert_image(a_picture(100, 100, 2), Vec2::ZERO, Vec2::new(800.0, 600.0))
            .unwrap();

        assert!(editor.delete_selection());
        assert!(!editor.world().node_is_live(node));

        assert!(editor.undo());
        assert!(editor.world().node_is_live(node));
        assert!(
            editor.image_of(node).is_some(),
            "the restored element lost its picture"
        );
    }

    /// **§10's rule through the Duplicate action**: a copy is a handle, so the
    /// store holds one picture however many elements show it.
    #[test]
    fn duplicating_an_image_shares_its_bytes_rather_than_copying_them() {
        let mut editor = FlowEditor::new();
        let first = editor
            .insert_image(a_picture(120, 60, 3), Vec2::ZERO, Vec2::new(800.0, 600.0))
            .unwrap();

        assert!(editor.duplicate_selection());

        let live: Vec<NodeIndex> = editor.world().nodes().live_indices().collect();
        assert_eq!(live.len(), 2);
        assert_eq!(
            editor.world().image_count(),
            1,
            "the duplicate copied the bytes"
        );

        let copy = live
            .into_iter()
            .find(|&node| node != first)
            .expect("the copy is there");
        assert_eq!(
            editor.image_of(copy).map(|it| it.handle),
            editor.image_of(first).map(|it| it.handle),
            "the copy shows a different picture"
        );

        // And the same file inserted a second time is still one resource,
        // because the handle is a content hash.
        editor
            .insert_image(a_picture(120, 60, 3), Vec2::ZERO, Vec2::new(800.0, 600.0))
            .unwrap();
        assert_eq!(editor.world().image_count(), 1);
    }

    /// **Crop, both directions, one undo press each** — and the pixels are
    /// never touched, which is asserted as the bytes being the same `Arc`
    /// afterwards.
    #[test]
    fn cropping_is_undoable_and_leaves_the_bytes_alone() {
        use crate::properties::CropChoice;

        let mut editor = FlowEditor::new();
        let node = editor
            .insert_image(a_picture(400, 200, 4), Vec2::ZERO, Vec2::new(800.0, 600.0))
            .unwrap();
        let handle = editor.image_of(node).unwrap().handle;
        let before = std::sync::Arc::clone(editor.world().image(handle).unwrap());

        // Nothing to do yet: the frame is the picture's own shape.
        assert_eq!(editor.selection_crop(), None);
        assert!(!editor.crop_selection());

        // Squash the frame — what a shift-drag on a corner produces — and the
        // button offers to crop to it.
        editor
            .apply(EditCommand::resize_node(node, Vec2::new(200.0, 200.0)))
            .unwrap();
        assert_eq!(editor.selection_crop(), Some(CropChoice::ToFrame));

        let depth = editor.history().undo_depth();
        assert!(editor.crop_selection());
        assert_eq!(
            editor.history().undo_depth(),
            depth + 1,
            "a crop is one press of undo"
        );

        let crop = editor.image_of(node).unwrap().crop;
        assert!(!crop.is_full(), "{crop:?}");
        assert!((crop.width - 0.5).abs() < 1e-3, "{crop:?}");
        assert!(
            std::sync::Arc::ptr_eq(&before, editor.world().image(handle).unwrap()),
            "cropping rewrote the picture"
        );

        // The same button now offers the whole picture back, and taking it
        // restores the frame's shape as well as the crop.
        assert_eq!(editor.selection_crop(), Some(CropChoice::Reset));
        assert!(editor.crop_selection());
        assert!(editor.image_of(node).unwrap().crop.is_full());
        let size = editor.world().nodes().size(node);
        assert!(
            (size.x / size.y - 2.0).abs() < 1e-3,
            "the frame did not follow the picture back: {size:?}"
        );

        // And one press of undo walks each of those back.
        assert!(editor.undo());
        assert!(!editor.image_of(node).unwrap().crop.is_full());
        assert!(editor.undo());
        assert!(editor.image_of(node).unwrap().crop.is_full());
    }

    /// A picture too big for the room it is given arrives shrunk, and one
    /// small enough arrives at its own size — the placement rule, through the
    /// door that uses it.
    #[test]
    fn an_inserted_picture_is_centred_and_fitted() {
        let mut editor = FlowEditor::new();
        let node = editor
            .insert_image(
                a_picture(4000, 2000, 5),
                Vec2::new(100.0, 100.0),
                Vec2::new(800.0, 600.0),
            )
            .unwrap();

        let bounds = editor.world().nodes().bounds(node);
        assert!((bounds.size.x - 800.0).abs() < 1e-3, "{bounds:?}");
        assert!((bounds.center().x - 100.0).abs() < 1e-3, "{bounds:?}");
        assert!((bounds.center().y - 100.0).abs() < 1e-3, "{bounds:?}");
    }

    #[test]
    fn an_edit_that_changed_nothing_records_no_step() {
        let (mut editor, node) = editor_with_a_node();
        let depth = editor.history().undo_depth();

        let summary = editor
            .apply(EditCommand::move_node(node, Vec2::ZERO))
            .unwrap();

        assert!(!summary.changed);
        assert_eq!(editor.history().undo_depth(), depth);
    }

    /// Selection is view state: it must not consume an undo step, or clicking
    /// around before pressing undo would undo the clicks.
    #[test]
    fn selecting_records_nothing() {
        let (mut editor, node) = editor_with_a_node();
        let depth = editor.history().undo_depth();

        editor.select_only(Some(node));
        editor.set_node_selected(node, false);
        editor.clear_selection();

        assert_eq!(editor.history().undo_depth(), depth);
    }

    /// Loading a document must drop the history: every stored delta names
    /// runtime indices, and they mean something else in the new document.
    #[test]
    fn loading_a_document_clears_the_history() {
        let (mut editor, node) = editor_with_a_node();
        editor
            .apply(EditCommand::move_node(node, Vec2::new(10.0, 0.0)))
            .unwrap();
        assert!(editor.can_undo());

        editor.load_document(crate::models::FlowDocument::new());
        assert!(!editor.can_undo() && !editor.can_redo());
    }

    /// Pressing Delete with nothing selected must be a no-op that costs no undo
    /// press. It is the most common way the key is hit.
    #[test]
    fn deleting_an_empty_selection_records_nothing() {
        let (mut editor, _) = editor_with_a_node();
        let depth = editor.history().undo_depth();

        assert!(!editor.delete_selection());
        assert_eq!(editor.history().undo_depth(), depth);
    }

    /// **Deleting must not clear the selection**, and this is the test that
    /// pins it. Removal deselects each element on its way out, so a clear here
    /// would be redundant going forward and wrong coming back: the undo
    /// restores the elements, and a selection cleared by the *delete* is not
    /// something the undo knows to restore.
    #[test]
    fn deleting_removes_the_selected_nodes_and_undo_brings_them_back() {
        let (mut editor, node) = editor_with_a_node();
        editor.select_only(Some(node));

        assert!(editor.delete_selection());
        assert!(!editor.world().node_is_live(node));
        assert!(
            editor.world().selection().is_empty(),
            "a removed element stayed in the selection set"
        );

        assert!(editor.undo());
        assert!(editor.world().node_is_live(node));
    }

    /// Several elements go in one command and therefore in one undo press —
    /// which is what a rubber band followed by Delete has to feel like.
    #[test]
    fn deleting_many_elements_is_one_undo_press() {
        let mut editor = FlowEditor::new();
        let added = editor
            .apply(EditCommand::AddNodes(vec![
                NodeDraft::new(NodeSpec::new(
                    ElementId::NONE,
                    ElementKind::Shape(ShapeKind::Rectangle),
                    Vec2::new(0.0, 0.0),
                    Vec2::new(50.0, 30.0),
                )),
                NodeDraft::new(NodeSpec::new(
                    ElementId::NONE,
                    ElementKind::Shape(ShapeKind::Ellipse),
                    Vec2::new(100.0, 0.0),
                    Vec2::new(50.0, 30.0),
                )),
            ]))
            .unwrap()
            .added_nodes;

        for node in &added {
            editor.set_node_selected(*node, true);
        }
        let depth = editor.history().undo_depth();

        assert!(editor.delete_selection());
        assert_eq!(editor.history().undo_depth(), depth + 1);
        assert!(added.iter().all(|node| !editor.world().node_is_live(*node)));

        assert!(editor.undo());
        assert!(added.iter().all(|node| editor.world().node_is_live(*node)));
    }

    /// An interrupted drag leaves a gesture open. Pressing undo then must take
    /// a step rather than fold the undo into the abandoned gesture.
    #[test]
    fn undo_closes_a_gesture_left_open() {
        let (mut editor, node) = editor_with_a_node();
        editor.begin_gesture();
        editor
            .apply(EditCommand::move_node(node, Vec2::new(10.0, 0.0)))
            .unwrap();

        assert!(editor.undo());
        assert_eq!(editor.history().open_gesture(), None);
        assert_eq!(editor.world().nodes().position(node), Vec2::new(0.0, 0.0));
    }
}
