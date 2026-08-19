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

use crate::{
    commands::{
        apply::{EditOutcome, apply},
        edit::{EditCommand, EditError},
        history::{CommandHistory, GestureId},
    },
    geometry::RouteOptions,
    models::{
        DocumentSettings, EdgeIndex, ElementId, FlowDocument, NodeIndex, RenderStyle, SketchStyle,
    },
    runtime::{BoxQuery, ConnectionRules, DirtyState, GraphWorld, LoadReport},
};

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
}

impl FlowEditor {
    pub fn new() -> FlowEditor {
        FlowEditor {
            world: GraphWorld::new(),
            history: CommandHistory::new(),
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
        }

        Ok(EditSummary {
            added_nodes,
            added_edges,
            changed,
        })
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
        self.world.settings_mut().render_style = style;
    }

    pub fn set_sketch_style(&mut self, sketch: SketchStyle) {
        self.world.settings_mut().sketch = sketch;
    }

    pub fn set_route_options(&mut self, options: RouteOptions) {
        self.world.set_route_options(options);
    }

    pub fn set_rules(&mut self, rules: ConnectionRules) {
        self.world.set_rules(rules);
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
