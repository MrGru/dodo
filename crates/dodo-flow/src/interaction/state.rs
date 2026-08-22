//! [`InteractionMachine`] — **one explicit state, not a drawer of booleans**
//! (§25).
//!
//! The requirement is unusually specific about the shape, and it is specific
//! because the failure it prevents is so ordinary: `is_panning`,
//! `is_box_selecting`, `drag_start`, `did_move` accumulate one at a time, each
//! addition is locally reasonable, and the bug is always the same — two of them
//! true at once, in a combination nobody wrote down and nobody can test. An
//! enum makes that state unrepresentable, and it makes the transitions a pure
//! function of (state, event), which is the reason every one of them is
//! asserted below with no window anywhere in sight.
//!
//! # The vocabulary is deliberately not GPUI's
//!
//! [`PointerButton`], [`InputModifiers`] and [`InteractionEvent`] restate what
//! `MouseDownEvent` and friends already carry. That duplication is the price of
//! the crate's central boundary — this file is unit tested with no `App`, no
//! window and no event loop — and it buys a second thing worth having: the
//! machine takes **both** the screen and the world position of the pointer,
//! because it needs screen for panning and world for box selection, and
//! `views/` is the only place that knows the pane's origin.
//!
//! # Which space each state remembers, and why it matters
//!
//! - [`InteractionState::Panning`] remembers the last **screen** position. A
//!   pan is a screen-space displacement; remembering it in world units would
//!   make it change meaning as the pan proceeded.
//! - [`InteractionState::BoxSelecting`] remembers **world** positions. The
//!   rectangle is anchored to the document, so zooming or scrolling mid-drag
//!   leaves it over the same content instead of sliding out from under it.
//!
//! # The machine is told what is under the pointer, and never asks
//!
//! [`InteractionEvent::PointerDown`] carries a
//! [`PointerTarget`](crate::runtime::PointerTarget), resolved by whoever raised
//! the event. That is what lets one press mean four different gestures — pan,
//! box select, drag a node, drag a connection out of a handle — without this
//! file knowing a graph exists. It also keeps the hit test where §29 wants it:
//! a broad phase the caller owns, and a narrow phase in `runtime`.
//!
//! # What this phase deliberately does not do
//!
//! [`InteractionEffect::CommitBoxSelect`] hands back a world rectangle and
//! stops. Turning that rectangle into a set of element ids needs the spatial
//! index's broad phase (§28), which is Phase 4's — and a linear scan over every
//! element would be exactly the thing §40 rule 1 forbids, written in a place it
//! would be easy to forget to remove. §25's remaining states (`Resizing`,
//! `Rotating`, …) arrive with the phases that can implement them; the enum is
//! where they will go. `EditingText` is Phase 10's and is now here.
//!
//! # A press that selects and starts nothing
//!
//! Every other left press opens a state: a pan, a band, a drag, a connection, a
//! creation. A press on an edge does not, and that is the design rather than an
//! omission — an edge has no drag gesture, so the moves that follow such a
//! press mean nothing, and [`InteractionState::Idle`] already answers every
//! event with `None`. A variant entered only to ignore things is a variant to
//! keep correct for no behaviour; the phase that gives an edge a drag adds one
//! then, and does not touch the arm that selects.
//!
//! The cost is stated where it is paid, in
//! [`HitTolerance::EDGE_SCREEN_RADIUS`](crate::runtime::HitTolerance::EDGE_SCREEN_RADIUS):
//! canvas within six screen pixels of a route is canvas a rubber band can no
//! longer be started in.
//!
//! # Editing text is a state, and the text itself is deliberately not in it
//!
//! [`InteractionState::EditingText`] holds a
//! [`TextTarget`](crate::interaction::TextTarget) and nothing else. The
//! characters being typed belong to whatever widget is collecting them, which
//! is `views/`'s business and needs a `Window` to exist at all; keeping them
//! here would put a `String` — and, in practice, a text-input entity — below
//! the UI-framework line.
//!
//! What this buys is the same thing every other variant buys: *what the next
//! press means* has exactly one answer. A press while text is being edited
//! **commits it**, because that is what clicking away from a caret means
//! everywhere else, and it is one arm rather than a `if self.editing` scattered
//! through the view.
//!
//! **This file names no UI framework.**

use crate::{
    geometry::{Rect, ResizeCorner, Vec2, resize_from_corner},
    interaction::tool::{
        CanvasTool, ConnectorCreation, CreationGesture, TextTarget, connector_endpoints,
        creation_rect,
    },
    models::{Connector, ConnectorEnd, EdgeIndex, HandleIndex, NodeIndex},
    runtime::PointerTarget,
};

/// The pointer buttons the canvas distinguishes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PointerButton {
    Left,
    Middle,
    Right,
}

/// The keyboard modifiers held when a pointer event happened.
///
/// `command` and `control` are kept apart rather than collapsed into one
/// "platform modifier": §26 asks for configurable bindings, and a binding table
/// that cannot tell them apart cannot express a platform difference.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct InputModifiers {
    pub shift: bool,
    pub control: bool,
    pub alt: bool,
    pub command: bool,
}

impl InputModifiers {
    pub const NONE: InputModifiers = InputModifiers {
        shift: false,
        control: false,
        alt: false,
        command: false,
    };

    pub fn shift() -> InputModifiers {
        InputModifiers {
            shift: true,
            ..InputModifiers::NONE
        }
    }

    /// Whether this modifier set means "add to the selection rather than
    /// replace it" — §26's shift-select.
    pub fn is_additive(&self) -> bool {
        self.shift || self.command
    }
}

/// What the canvas is in the middle of.
///
/// Every variant owns the data that only makes sense while it is current, which
/// is the property that makes the illegal combinations unrepresentable rather
/// than merely unlikely.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum InteractionState {
    #[default]
    Idle,
    Panning {
        /// The button that started the pan, so only that button ends it.
        button: PointerButton,
        /// Screen pixels. See the module doc for why not world.
        last_screen: Vec2,
    },
    BoxSelecting {
        /// World units, so the rectangle stays over the same content if the
        /// view zooms or scrolls mid-drag.
        anchor_world: Vec2,
        current_world: Vec2,
        /// Captured at press time rather than read at release: a user who lets
        /// go of shift before the mouse button still meant an additive select.
        additive: bool,
    },
    /// §25's `DraggingElements`, for one node. Multi-node dragging waits for
    /// Phase 4, because a selection is a set of elements and resolving a box
    /// selection into one is the spatial index's job.
    DraggingNode {
        node: NodeIndex,
        /// World units: where the pointer was at the last move, so every move
        /// contributes a *delta*. Anchoring to the pointer rather than to the
        /// node is what stops the node jumping to centre itself under the
        /// cursor on the first move.
        last_world: Vec2,
        /// Everything the node has moved so far, so [`InteractionEvent::Cancel`]
        /// can put it back exactly.
        total: Vec2,
    },
    /// §25's `CreatingShape`: a creating tool is drawing a new element out of
    /// its bounding box, and nothing has been added to the document yet.
    ///
    /// **§45's rule is enforced by this variant holding the whole gesture.**
    /// The document hears about a creation exactly once, on the release, so
    /// there is no draft element to clean up if the drag is abandoned and no
    /// half-created node another gesture could reach.
    CreatingShape {
        tool: CanvasTool,
        gesture: CreationGesture,
        /// A semantic target under pointer-down for a straight connector.
        start_target: Option<NodeIndex>,
    },
    /// §25's `Connecting`: an edge is being dragged out of a handle and has not
    /// landed yet (§8's connection preview).
    Connecting {
        source: ConnectionSource,
        /// Where the pointer is, in world units — the loose end of the preview.
        current_world: Vec2,
    },
    /// **§25's `Resizing`** (Phase 12): a corner grip is being dragged.
    ///
    /// The whole gesture is in the variant, exactly as a creation's is: the
    /// frame the drag started from, which corner is moving, and whether the
    /// proportions are being kept. Nothing is read back out of the world
    /// mid-drag, so a resize is arithmetic on numbers captured at the press —
    /// which is what makes every rule below assertable with no world at all.
    ///
    /// `aspect` is captured at press time rather than recomputed per move for
    /// the same reason `additive` is: it is the ratio of the frame the user
    /// grabbed, and recomputing it from the rectangle being produced would feed
    /// the lock into itself and let the shape drift.
    Resizing {
        node: NodeIndex,
        corner: ResizeCorner,
        /// Where the element was when the grip was pressed. The anchor corner
        /// comes from this, and so does [`InteractionEvent::Cancel`]'s exact
        /// restore.
        start: Rect,
        /// `Some(width / height)` while the proportions are locked.
        aspect: Option<f32>,
        /// The rectangle the last move produced, so the release can say whether
        /// anything actually changed.
        current: Rect,
    },
    /// One of a straight connector's two ordered endpoints is being moved.
    DraggingConnectorEndpoint {
        node: NodeIndex,
        end: ConnectorEnd,
        original: Connector,
        current_world: Vec2,
        target: Option<NodeIndex>,
    },
    /// §25's `EditingText` (§9): a caret is in something.
    ///
    /// **Not a drag**, which is why it is the only state a `PointerUp` does not
    /// end and the only one entered by something other than a press. It is left
    /// by a commit, a cancel, or a press anywhere — see the module doc.
    EditingText { target: TextTarget },
}

/// The handle a pending connection started from.
///
/// A connection always starts at a **handle**, never at a node body, because a
/// press on a body is already the drag gesture. §4's whole-node connection mode
/// applies at the other end: a connection may be *dropped* on a node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ConnectionSource {
    pub node: NodeIndex,
    pub handle: HandleIndex,
}

/// A connection in progress, for the painter.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PendingConnection {
    pub source: ConnectionSource,
    /// World units.
    pub current_world: Vec2,
}

/// What happened, in the machine's own vocabulary.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InteractionEvent {
    PointerDown {
        screen: Vec2,
        world: Vec2,
        button: PointerButton,
        modifiers: InputModifiers,
        /// Whether the "pan instead" key — space, in dodo's default binding —
        /// was held. Passed in rather than read from `modifiers` because it is
        /// a key rather than a modifier, and because §26 wants it rebindable.
        pan_key_held: bool,
        /// What the press landed on, resolved by the caller. See the module
        /// doc: this is what lets one press mean four gestures.
        target: PointerTarget,
    },
    PointerMove {
        screen: Vec2,
        world: Vec2,
    },
    PointerUp {
        button: PointerButton,
        /// Where the release happened, in world units. Only a pending
        /// connection reads it, and only to say where it was dropped.
        world: Vec2,
        /// What was under the pointer at the release. A connection lands on
        /// this; every other gesture ignores it.
        target: PointerTarget,
    },
    /// **A resize grip was pressed** (Phase 12).
    ///
    /// An event of its own rather than an arm of
    /// [`PointerDown`](InteractionEvent::PointerDown), because a resize needs
    /// one thing the machine deliberately cannot reach: **the element's current
    /// rectangle**. `PointerDown` already carries a `target` the caller
    /// resolved against the world, and this carries the frame for the same
    /// reason and by the same rule — the machine stays world-free, and what it
    /// cannot look up is handed to it.
    ///
    /// The caller raises this *instead of* the press, having seen
    /// [`PointerTarget::ResizeGrip`]; sending both would start a drag and a
    /// resize on one press.
    BeginResize {
        node: NodeIndex,
        corner: ResizeCorner,
        /// The element's rectangle right now, in world units.
        frame: Rect,
        /// Whether this drag keeps the element's proportions —
        /// [`resize_keeps_aspect`](super::resize_keeps_aspect)'s answer, which
        /// folds the element's own default together with the modifier.
        keeps_aspect: bool,
    },
    /// One of exactly two connector endpoint handles was pressed.
    BeginConnectorEndpointDrag {
        node: NodeIndex,
        end: ConnectorEnd,
        connector: Connector,
    },
    /// A connector endpoint moved, with the nearest valid snap target already
    /// resolved by the caller's spatial broad phase.
    MoveConnectorEndpoint {
        world: Vec2,
        target: Option<NodeIndex>,
    },
    /// **§45's tool activation**, from the palette or from a key binding.
    ///
    /// An event rather than a setter so that it goes through the same total
    /// transition function as everything else — a tool change is a state change
    /// and the file's whole argument is that state changes belong in one
    /// `match`.
    SelectTool(CanvasTool),
    /// **The tool lock** — Excalidraw's "keep the selected tool active after
    /// drawing", as a toggle rather than as a modifier.
    ///
    /// Unlike [`InteractionEvent::SelectTool`] this is accepted *while a
    /// gesture is running*, and deliberately: it changes nothing about the
    /// gesture in progress, only what happens when that gesture finishes. A
    /// user who starts a rectangle and then decides they want three more must
    /// not have to abandon the first one to say so.
    SetToolLock(bool),
    /// **A double-click** (§9): the gesture that opens a text editor.
    ///
    /// Its own event rather than a `click_count` on
    /// [`PointerDown`](InteractionEvent::PointerDown), because the two mean
    /// entirely different things and folding them together would put a
    /// `count == 2` branch in front of every gesture in the machine. The caller
    /// raises this *instead of* a press, and does so only when the platform
    /// says the click was a double one — which is a question about the mouse
    /// and its timing, and therefore `views/`'s.
    DoubleClick {
        /// Where it landed, in world units. Only the `Empty` arm reads it, to
        /// place a new text element under the pointer.
        world: Vec2,
        target: PointerTarget,
    },
    /// **The text being edited is finished** — `Cmd`/`Ctrl`+`Enter`, a click
    /// away, or a commit from anywhere else. The caller has the characters;
    /// this only closes the state.
    ///
    /// It was plain `Enter` until §9's caret became a paragraph field. That key
    /// belongs to the field now — it inserts a line break — and the keyboard's
    /// way here is `commands::keys`'s `CommitText` row.
    FinishTextEdit,
    /// `Esc`, a lost window focus, or anything else that means "stop".
    Cancel,
}

/// A completed box selection, handed to whatever can resolve it into elements.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BoxSelection {
    /// World units, always normalised — dragging up-left gives the same
    /// rectangle as dragging down-right.
    pub rect: Rect,
    pub additive: bool,
}

/// What the caller must do about a transition.
///
/// One effect per event, which is enough because no transition here needs two.
/// The view's whole job is a `match` over this, so a new variant is a compile
/// error at the one place that has to handle it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InteractionEffect {
    /// Nothing to do, and — importantly — nothing to repaint.
    None,
    /// A pan started. The view captures the pointer so the drag survives
    /// leaving the pane.
    BeginPan,
    /// Move the viewport by this screen-space delta.
    PanBy(Vec2),
    EndPan,
    /// A box selection started, with its (degenerate) initial rectangle.
    BeginBoxSelect(Rect),
    /// The selection rectangle changed; repaint it.
    UpdateBoxSelect(Rect),
    /// The drag finished. Phase 4 resolves this into element ids.
    CommitBoxSelect(BoxSelection),
    /// The drag was abandoned; stop drawing the rectangle.
    CancelBoxSelect,

    /// A node drag started. The view captures the pointer and marks the node
    /// selected — replacing the selection, or adding to it under shift.
    ///
    /// `additive` is captured at press time rather than read at release, for
    /// the same reason [`InteractionState::BoxSelecting`] captures it: a user
    /// who lets go of shift before the mouse button still meant an additive
    /// select.
    BeginNodeDrag {
        node: NodeIndex,
        additive: bool,
    },
    /// Move this node by this **world** delta — [`GraphWorld::move_node`](crate::runtime::GraphWorld::move_node),
    /// and nothing else in the graph.
    DragNodeBy {
        node: NodeIndex,
        delta: Vec2,
    },
    /// The drag finished. `moved` is false for a press-and-release that never
    /// travelled, which is a *click* and which a later phase turns into a
    /// selection rather than into an undo entry.
    EndNodeDrag {
        node: NodeIndex,
        moved: bool,
    },
    /// The drag was abandoned; move the node by this delta to put it back
    /// exactly where it started.
    CancelNodeDrag {
        node: NodeIndex,
        revert: Vec2,
    },

    /// A resize started. The view captures the pointer, exactly as it does for
    /// a node drag, so the gesture survives the pointer leaving the pane.
    BeginResize {
        node: NodeIndex,
    },
    /// **Put this node in this rectangle** — position and size together, in
    /// world units.
    ///
    /// Both halves, because dragging a top-left grip moves the origin as well
    /// as changing the size, and splitting them would be two commands whose
    /// intermediate state is an element in a place it was never in.
    ResizeNodeTo {
        node: NodeIndex,
        rect: Rect,
    },
    /// The resize finished. `changed` is false for a press and release that
    /// never travelled, which must not become an undo entry.
    EndResize {
        node: NodeIndex,
        changed: bool,
    },
    BeginConnectorEndpointDrag {
        node: NodeIndex,
    },
    MoveConnectorEndpoint {
        node: NodeIndex,
        end: ConnectorEnd,
        point: Vec2,
        target: Option<NodeIndex>,
    },
    EndConnectorEndpointDrag {
        node: NodeIndex,
        end: ConnectorEnd,
    },
    CancelConnectorEndpointDrag,
    /// The resize was abandoned; put the node back in exactly this rectangle.
    CancelResize {
        node: NodeIndex,
        rect: Rect,
    },

    /// **An edge was clicked** (Phase 10.5): make it the selection, or add it
    /// to the selection under shift.
    ///
    /// No matching `End`, because there is no gesture to end — the press
    /// selects and the machine never leaves [`InteractionState::Idle`]. That is
    /// the difference between this and [`BeginNodeDrag`](InteractionEffect::BeginNodeDrag),
    /// which selects *and* opens a drag.
    SelectEdge {
        edge: EdgeIndex,
        additive: bool,
    },

    /// A connection started being dragged out of a handle.
    BeginConnect(ConnectionSource),
    /// The loose end moved; repaint the preview.
    UpdateConnect(PendingConnection),
    /// The connection was dropped. **The machine does not know whether it is
    /// valid** — validation is §4's and lives in
    /// [`GraphWorld::validate_connection`](crate::runtime::GraphWorld::validate_connection),
    /// so the view asks the world and the world refuses or connects.
    CommitConnect {
        source: ConnectionSource,
        target: PointerTarget,
    },
    /// The connection was abandoned; stop drawing the preview.
    CancelConnect,

    /// **The active tool changed** (§45). Nothing about the document changed;
    /// the repaint is for the palette's active state and the canvas's cursor.
    ///
    /// **A tool that changed itself does not raise this**, and cannot: the
    /// return to Select after a drawing happens *inside* the same transition
    /// that commits the element, and this file's rule is one effect per event.
    /// The commit already repaints, so the palette is redrawn from
    /// [`InteractionMachine::tool`] on the next frame either way — which is the
    /// property that makes the machine the only copy of the active tool worth
    /// having.
    ToolChanged(CanvasTool),
    /// The tool lock was switched. Nothing about the document changed; the
    /// repaint is for the toggle's own state.
    ToolLockChanged(bool),
    /// A creation drag started. The rectangle is already the one
    /// [`creation_rect`] resolved, so a painter draws exactly what will be
    /// committed.
    BeginCreate {
        tool: CanvasTool,
        rect: Rect,
    },
    /// The pending element's bounding box changed; repaint the preview.
    UpdateCreate {
        tool: CanvasTool,
        rect: Rect,
    },
    /// **Create the element.** The one effect in this file that means an edit,
    /// and [`apply_gesture`](crate::commands::gesture::apply_gesture) turns it
    /// into §30's `AddNodes` — this file does not know a document exists.
    CommitCreate {
        tool: CanvasTool,
        rect: Rect,
        connector: Option<ConnectorCreation>,
    },
    /// The creation was abandoned. **Nothing to undo**: no element was ever
    /// added, which is the point of the tool never touching the document until
    /// the release.
    CancelCreate,

    /// **Put a caret in this** (§9). The view opens a text editor over the
    /// target and seeds it with whatever text the target already has — which
    /// is what makes existing text editable again rather than merely
    /// replaceable.
    ///
    /// Raised by a double-click on a node, an edge or empty canvas, and by the
    /// Text tool finishing its drag. Those two paths differ only in which
    /// [`TextTarget`] they carry, which is the whole reason the target is a
    /// type rather than three booleans.
    BeginTextEdit(TextTarget),
    /// **Commit whatever is in the editor.** The characters are the view's; the
    /// target is here so it does not have to remember which one it opened.
    ///
    /// Committing *empty* text is meaningful and not a no-op: it clears an
    /// existing label, and it abandons a pending element. Which of those it is
    /// is [`FlowEditor::commit_text`](crate::commands::FlowEditor::commit_text)'s
    /// decision, not this file's.
    CommitTextEdit(TextTarget),
    /// The edit was abandoned — `Esc`. Nothing reaches the document, including
    /// for a pending element, which is why an abandoned Text-tool gesture
    /// leaves nothing behind.
    CancelTextEdit,
}

impl InteractionEffect {
    /// Whether the view should call `Window::capture_pointer`, so the drag
    /// keeps receiving moves once the pointer leaves the canvas. Capture
    /// auto-releases on mouse up, so there is no matching "release" question.
    pub fn starts_a_drag(&self) -> bool {
        matches!(
            self,
            InteractionEffect::BeginPan
                | InteractionEffect::BeginBoxSelect(_)
                | InteractionEffect::BeginNodeDrag { .. }
                | InteractionEffect::BeginConnect(_)
                | InteractionEffect::BeginCreate { .. }
                | InteractionEffect::BeginResize { .. }
                | InteractionEffect::BeginConnectorEndpointDrag { .. }
        )
    }

    /// **Every effect that opens a dragging state**, for the test below. A
    /// gesture that opens a state and is not captured is one that stops the
    /// moment the pointer leaves the pane — silently, and only for the users
    /// with small windows.
    #[cfg(test)]
    const OPENS_A_GESTURE: &'static [fn() -> InteractionEffect] = &[
        || InteractionEffect::BeginPan,
        || InteractionEffect::BeginBoxSelect(Rect::new(Vec2::ZERO, Vec2::ZERO)),
        || InteractionEffect::BeginNodeDrag {
            node: NodeIndex::new(0),
            additive: false,
        },
        || InteractionEffect::BeginResize {
            node: NodeIndex::new(0),
        },
    ];

    /// Whether the canvas has to be repainted. dodo repaints on change and
    /// never on a timer (§35, §40 rule 15), so this is the whole condition for
    /// a `cx.notify()`.
    pub fn needs_repaint(&self) -> bool {
        !matches!(self, InteractionEffect::None)
    }
}

/// The canvas's interaction state, and the transitions between them.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct InteractionMachine {
    state: InteractionState,
    /// §45's active tool. **View state, never the document's** — see
    /// [`crate::interaction::tool`] for the rule and what honouring it costs.
    tool: CanvasTool,
    /// Whether finishing a drawing keeps the tool.
    ///
    /// Beside the tool rather than on the view for the same reason the tool is
    /// here: it is read at exactly one point — the transition that commits a
    /// creation — and a copy on the view would be a second answer to "what
    /// happens when this drag ends?" that nothing forces to agree.
    tool_locked: bool,
}

impl InteractionMachine {
    pub fn new() -> InteractionMachine {
        InteractionMachine::default()
    }

    pub fn state(&self) -> &InteractionState {
        &self.state
    }

    /// **The active tool** (§45): what the next press means.
    pub fn tool(&self) -> CanvasTool {
        self.tool
    }

    /// **The tool lock**: whether finishing a drawing keeps the tool rather
    /// than returning to [`CanvasTool::Select`].
    pub fn tool_locked(&self) -> bool {
        self.tool_locked
    }

    /// The element being drawn and the box it currently occupies, or `None`.
    /// The painter's only question about a creation in progress, and the same
    /// shape [`selection_rect`](InteractionMachine::selection_rect) takes.
    pub fn creation_preview(&self) -> Option<(CanvasTool, Rect)> {
        match self.state {
            InteractionState::CreatingShape { tool, gesture, .. } => {
                Some((tool, creation_rect(tool, gesture)))
            }
            _ => None,
        }
    }

    /// An in-progress straight connector creation, preserving pointer-down as
    /// start even when the derived bounds normalize in the opposite direction.
    pub fn connector_creation(&self) -> Option<(CanvasTool, ConnectorCreation)> {
        match self.state {
            InteractionState::CreatingShape {
                tool,
                gesture,
                start_target,
            } if matches!(tool, CanvasTool::Line | CanvasTool::Arrow) => {
                let (start, end) = connector_endpoints(tool, gesture);
                Some((
                    tool,
                    ConnectorCreation {
                        start,
                        end,
                        start_target,
                        end_target: None,
                    },
                ))
            }
            _ => None,
        }
    }

    /// The connector endpoint currently being dragged and the opposite point
    /// used to choose a direction-appropriate snap anchor.
    pub fn dragging_connector_endpoint(
        &self,
    ) -> Option<(NodeIndex, ConnectorEnd, Vec2, Option<NodeIndex>)> {
        match self.state {
            InteractionState::DraggingConnectorEndpoint {
                node,
                end,
                original,
                target,
                ..
            } => Some((node, end, original.opposite(end).point, target)),
            _ => None,
        }
    }

    pub fn is_idle(&self) -> bool {
        matches!(self.state, InteractionState::Idle)
    }

    pub fn is_panning(&self) -> bool {
        matches!(self.state, InteractionState::Panning { .. })
    }

    /// The node being dragged, for a painter that wants to show it differently.
    pub fn dragging_node(&self) -> Option<NodeIndex> {
        match self.state {
            InteractionState::DraggingNode { node, .. } => Some(node),
            _ => None,
        }
    }

    /// The connection being dragged, or `None`. **The painter's only question**
    /// about a pending connection — where it comes from and where its loose end
    /// is.
    pub fn pending_connection(&self) -> Option<PendingConnection> {
        match self.state {
            InteractionState::Connecting {
                source,
                current_world,
            } => Some(PendingConnection {
                source,
                current_world,
            }),
            _ => None,
        }
    }

    /// The selection rectangle to draw, in **world** units, or `None` when no
    /// box selection is in progress. The painter's only question.
    pub fn selection_rect(&self) -> Option<Rect> {
        match self.state {
            InteractionState::BoxSelecting {
                anchor_world,
                current_world,
                ..
            } => Some(Rect::from_corners(anchor_world, current_world)),
            _ => None,
        }
    }

    /// **The transition function.** Pure, total, and the only way the state
    /// changes.
    pub fn handle(&mut self, event: InteractionEvent) -> InteractionEffect {
        match (self.state, event) {
            // ---- starting something, from rest ----
            (
                InteractionState::Idle,
                InteractionEvent::PointerDown {
                    screen,
                    world,
                    button,
                    modifiers,
                    pan_key_held,
                    target,
                },
            ) => match button {
                // Middle-drag always pans, and space-drag turns a left press
                // into a pan — the two bindings every infinite canvas shares.
                PointerButton::Middle => {
                    self.state = InteractionState::Panning {
                        button,
                        last_screen: screen,
                    };
                    InteractionEffect::BeginPan
                }
                PointerButton::Left if pan_key_held || self.tool == CanvasTool::Hand => {
                    self.state = InteractionState::Panning {
                        button,
                        last_screen: screen,
                    };
                    InteractionEffect::BeginPan
                }
                // **§45's tool decides first, and what the press landed on
                // decides only under `Select`.** A creating tool draws on top
                // of whatever is there — an Excalidraw user drawing a rectangle
                // over a node expects a rectangle, not a node drag — so the
                // target is not even consulted.
                PointerButton::Left if self.tool.creates() => {
                    let tool = self.tool;
                    let gesture = CreationGesture {
                        anchor_world: world,
                        current_world: world,
                        anchor_screen: screen,
                        current_screen: screen,
                        constrain: modifiers.shift,
                    };
                    self.state = InteractionState::CreatingShape {
                        tool,
                        gesture,
                        start_target: target.node(),
                    };
                    InteractionEffect::BeginCreate {
                        tool,
                        rect: creation_rect(tool, gesture),
                    }
                }
                // What the press landed on decides which of the three
                // left-button gestures this is. The order is the order of
                // specificity: a handle sits on a node, and a node sits on the
                // canvas.
                PointerButton::Left => match target {
                    PointerTarget::Handle { node, handle } => {
                        let source = ConnectionSource { node, handle };
                        self.state = InteractionState::Connecting {
                            source,
                            current_world: world,
                        };
                        InteractionEffect::BeginConnect(source)
                    }
                    PointerTarget::Node(node) => {
                        self.state = InteractionState::DraggingNode {
                            node,
                            last_world: world,
                            total: Vec2::ZERO,
                        };
                        InteractionEffect::BeginNodeDrag {
                            node,
                            additive: modifiers.is_additive(),
                        }
                    }
                    // **An edge is selected and no gesture is started**
                    // (Phase 10.5). The machine stays `Idle` on purpose rather
                    // than entering a state that would do nothing: an edge has
                    // no drag, so the moves after this press mean nothing, and
                    // `Idle` already says that for every event without a single
                    // arm to maintain. A phase that gives an edge a drag adds
                    // its own state here and does not touch this line.
                    PointerTarget::Edge(edge) => InteractionEffect::SelectEdge {
                        edge,
                        additive: modifiers.is_additive(),
                    },
                    // A grip press arrives as `BeginResize`, not as this —
                    // see that event. Reaching here means the caller resolved
                    // a grip and then sent the press anyway, and starting a
                    // rubber band from a corner of the selection would be the
                    // worst of the available guesses.
                    PointerTarget::ResizeGrip { .. } | PointerTarget::ConnectorEndpoint { .. } => {
                        InteractionEffect::None
                    }
                    PointerTarget::Empty => {
                        self.state = InteractionState::BoxSelecting {
                            anchor_world: world,
                            current_world: world,
                            additive: modifiers.is_additive(),
                        };
                        InteractionEffect::BeginBoxSelect(Rect::from_corners(world, world))
                    }
                },
                // The context menu is a later phase's, and swallowing the press
                // here would make it impossible to add without changing this
                // match — which is the point of it being explicit.
                PointerButton::Right => InteractionEffect::None,
            },

            // ---- §12's resize ----
            //
            // From rest only, like every other gesture that opens a state. A
            // grip press that arrives mid-drag is a stray event and starting a
            // second gesture under a moving hand is the mode-switch this enum
            // exists to prevent.
            (
                InteractionState::Idle,
                InteractionEvent::BeginResize {
                    node,
                    corner,
                    frame,
                    keeps_aspect,
                },
            ) => {
                let frame = frame.normalized();
                let aspect = keeps_aspect
                    .then(|| frame.width() / frame.height())
                    .filter(|it| it.is_finite() && *it > 0.0);
                self.state = InteractionState::Resizing {
                    node,
                    corner,
                    start: frame,
                    aspect,
                    current: frame,
                };
                InteractionEffect::BeginResize { node }
            }
            (_, InteractionEvent::BeginResize { .. }) => InteractionEffect::None,

            (
                InteractionState::Idle,
                InteractionEvent::BeginConnectorEndpointDrag {
                    node,
                    end,
                    connector,
                },
            ) => {
                self.state = InteractionState::DraggingConnectorEndpoint {
                    node,
                    end,
                    original: connector,
                    current_world: connector.endpoint(end).point,
                    target: None,
                };
                InteractionEffect::BeginConnectorEndpointDrag { node }
            }
            (_, InteractionEvent::BeginConnectorEndpointDrag { .. }) => InteractionEffect::None,

            (
                InteractionState::DraggingConnectorEndpoint {
                    node,
                    end,
                    original,
                    ..
                },
                InteractionEvent::MoveConnectorEndpoint { world, target },
            ) => {
                self.state = InteractionState::DraggingConnectorEndpoint {
                    node,
                    end,
                    original,
                    current_world: world,
                    target,
                };
                InteractionEffect::MoveConnectorEndpoint {
                    node,
                    end,
                    point: world,
                    target,
                }
            }
            (_, InteractionEvent::MoveConnectorEndpoint { .. }) => InteractionEffect::None,

            (
                InteractionState::Resizing {
                    node,
                    corner,
                    start,
                    aspect,
                    ..
                },
                InteractionEvent::PointerMove { world, .. },
            ) => {
                let rect = resize_from_corner(start, corner, world, aspect);
                self.state = InteractionState::Resizing {
                    node,
                    corner,
                    start,
                    aspect,
                    current: rect,
                };
                InteractionEffect::ResizeNodeTo { node, rect }
            }

            (
                InteractionState::Resizing {
                    node,
                    start,
                    current,
                    ..
                },
                InteractionEvent::PointerUp { button, .. },
            ) => {
                if button == PointerButton::Left {
                    self.state = InteractionState::Idle;
                    InteractionEffect::EndResize {
                        node,
                        // A press and release that never travelled is a click
                        // on a corner, not a zero-length resize — and the undo
                        // history must not gain an entry for it.
                        changed: current != start,
                    }
                } else {
                    InteractionEffect::None
                }
            }

            (InteractionState::Resizing { node, start, .. }, InteractionEvent::Cancel) => {
                self.state = InteractionState::Idle;
                InteractionEffect::CancelResize { node, rect: start }
            }

            // ---- a press while a caret is out ----
            //
            // **Commits, rather than being ignored.** Clicking away from a text
            // cursor means "I am done typing" in every editor there is, and the
            // press itself is consumed: this file's rule is one effect per
            // event, and committing is unambiguously the more important of the
            // two. A user who wanted the press as well presses again, on a
            // canvas that is no longer editing.
            (InteractionState::EditingText { target }, InteractionEvent::PointerDown { .. }) => {
                self.state = InteractionState::Idle;
                InteractionEffect::CommitTextEdit(target)
            }

            // ---- a press while already busy ----
            //
            // Ignored rather than treated as a restart. A second button going
            // down mid-drag is almost always accidental, and the alternative —
            // silently switching modes under the user's hand — is the exact
            // class of bug the enum exists to prevent.
            (_, InteractionEvent::PointerDown { .. }) => InteractionEffect::None,

            // ---- §9's double-click: the one gesture that opens a caret ----
            //
            // From rest only. A double-click that arrives mid-drag is a stray
            // second press the platform coalesced, and opening an editor under
            // a moving hand is worse than ignoring it.
            (InteractionState::Idle, InteractionEvent::DoubleClick { world, target }) => {
                // A handle sits *on* its node, and there is no text on a
                // handle — so double-clicking one edits the node it belongs to
                // rather than doing nothing, which is what a user aiming at the
                // edge of a small node has actually asked for.
                let target = match target {
                    PointerTarget::Node(node) | PointerTarget::Handle { node, .. } => {
                        TextTarget::Node(node)
                    }
                    PointerTarget::Edge(edge) => TextTarget::Edge(edge),
                    // A double-click on a grip is two presses on a corner, and
                    // it means the resize the first one started rather than a
                    // caret on whatever is underneath.
                    PointerTarget::ResizeGrip { node, .. }
                    | PointerTarget::ConnectorEndpoint { node, .. } => TextTarget::Node(node),
                    // **Empty canvas creates text**, centred on the pointer,
                    // exactly as a click with the Text tool does — one rule,
                    // through `creation_rect`, so the two cannot disagree about
                    // where a clicked text box goes.
                    PointerTarget::Empty => TextTarget::New(creation_rect(
                        CanvasTool::Text,
                        CreationGesture {
                            anchor_world: world,
                            current_world: world,
                            anchor_screen: Vec2::ZERO,
                            current_screen: Vec2::ZERO,
                            constrain: false,
                        },
                    )),
                };
                self.state = InteractionState::EditingText { target };
                InteractionEffect::BeginTextEdit(target)
            }

            // Anywhere else a double-click is a press the machine has already
            // handled; ignoring it is what stops a second editor opening over
            // the first.
            (_, InteractionEvent::DoubleClick { .. }) => InteractionEffect::None,

            (InteractionState::EditingText { target }, InteractionEvent::FinishTextEdit) => {
                self.state = InteractionState::Idle;
                InteractionEffect::CommitTextEdit(target)
            }
            (_, InteractionEvent::FinishTextEdit) => InteractionEffect::None,

            // ---- dragging ----
            (
                InteractionState::Panning {
                    button,
                    last_screen,
                },
                InteractionEvent::PointerMove { screen, .. },
            ) => {
                self.state = InteractionState::Panning {
                    button,
                    last_screen: screen,
                };
                InteractionEffect::PanBy(screen - last_screen)
            }

            (
                InteractionState::BoxSelecting {
                    anchor_world,
                    additive,
                    ..
                },
                InteractionEvent::PointerMove { world, .. },
            ) => {
                self.state = InteractionState::BoxSelecting {
                    anchor_world,
                    current_world: world,
                    additive,
                };
                InteractionEffect::UpdateBoxSelect(Rect::from_corners(anchor_world, world))
            }

            (
                InteractionState::DraggingNode {
                    node,
                    last_world,
                    total,
                },
                InteractionEvent::PointerMove { world, .. },
            ) => {
                let delta = world - last_world;
                self.state = InteractionState::DraggingNode {
                    node,
                    last_world: world,
                    total: total + delta,
                };
                InteractionEffect::DragNodeBy { node, delta }
            }

            (
                InteractionState::CreatingShape {
                    tool,
                    gesture,
                    start_target,
                },
                InteractionEvent::PointerMove { screen, world },
            ) => {
                let gesture = CreationGesture {
                    current_world: world,
                    current_screen: screen,
                    ..gesture
                };
                self.state = InteractionState::CreatingShape {
                    tool,
                    gesture,
                    start_target,
                };
                InteractionEffect::UpdateCreate {
                    tool,
                    rect: creation_rect(tool, gesture),
                }
            }

            (
                InteractionState::DraggingConnectorEndpoint {
                    node,
                    end,
                    original,
                    target,
                    ..
                },
                InteractionEvent::PointerMove { world, .. },
            ) => {
                self.state = InteractionState::DraggingConnectorEndpoint {
                    node,
                    end,
                    original,
                    current_world: world,
                    target,
                };
                InteractionEffect::MoveConnectorEndpoint {
                    node,
                    end,
                    point: world,
                    target,
                }
            }

            (
                InteractionState::Connecting { source, .. },
                InteractionEvent::PointerMove { world, .. },
            ) => {
                self.state = InteractionState::Connecting {
                    source,
                    current_world: world,
                };
                InteractionEffect::UpdateConnect(PendingConnection {
                    source,
                    current_world: world,
                })
            }

            // A caret is not a drag, so the pointer moving over it and any
            // stray release both mean nothing. Stated rather than swept into a
            // wildcard, because "does a move end a text edit?" is a real
            // question and this is where the answer belongs.
            (
                InteractionState::Idle | InteractionState::EditingText { .. },
                InteractionEvent::PointerMove { .. },
            ) => InteractionEffect::None,
            (InteractionState::EditingText { .. }, InteractionEvent::PointerUp { .. }) => {
                InteractionEffect::None
            }

            // ---- finishing ----
            (
                InteractionState::Panning {
                    button: started_with,
                    ..
                },
                InteractionEvent::PointerUp { button, .. },
            ) => {
                if button == started_with {
                    self.state = InteractionState::Idle;
                    InteractionEffect::EndPan
                } else {
                    InteractionEffect::None
                }
            }

            (
                InteractionState::BoxSelecting {
                    anchor_world,
                    current_world,
                    additive,
                },
                InteractionEvent::PointerUp { button, .. },
            ) => {
                if button == PointerButton::Left {
                    self.state = InteractionState::Idle;
                    InteractionEffect::CommitBoxSelect(BoxSelection {
                        rect: Rect::from_corners(anchor_world, current_world),
                        additive,
                    })
                } else {
                    InteractionEffect::None
                }
            }

            (
                InteractionState::DraggingNode { node, total, .. },
                InteractionEvent::PointerUp { button, .. },
            ) => {
                if button == PointerButton::Left {
                    self.state = InteractionState::Idle;
                    InteractionEffect::EndNodeDrag {
                        node,
                        // A press and release that never travelled is a click,
                        // not a zero-length drag, and the difference matters to
                        // the undo history Phase 7 builds on this.
                        moved: total != Vec2::ZERO,
                    }
                } else {
                    InteractionEffect::None
                }
            }

            (
                InteractionState::CreatingShape {
                    tool,
                    gesture,
                    start_target,
                },
                InteractionEvent::PointerUp { button, target, .. },
            ) => {
                if button == PointerButton::Left {
                    self.state = InteractionState::Idle;
                    // **Draw, finish, and land back on Select** — Excalidraw's
                    // default, and the reason the lock exists to switch it off.
                    // A drawing tool is almost always wanted once: the thing a
                    // user does next is move, resize or label what they just
                    // drew, and every one of those is the Select tool. Staying
                    // armed means the next press draws a second rectangle on
                    // top of the first.
                    //
                    // It happens here rather than in the view because the tool
                    // is this machine's state and there is no second copy —
                    // whoever reads `tool()` on the next frame sees it, and a
                    // view that forgot to reset would be a palette disagreeing
                    // with what the next press does.
                    if !self.tool_locked {
                        self.tool = CanvasTool::Select;
                    }
                    let rect = creation_rect(tool, gesture);
                    // **The Text tool's release opens a caret instead of
                    // adding an element** (§9). The document is untouched
                    // until there is text to put in it, so an abandoned text
                    // gesture leaves nothing — the same promise every other
                    // tool keeps by not writing until the release.
                    if tool.edits_text_on_release() {
                        let target = TextTarget::New(rect);
                        self.state = InteractionState::EditingText { target };
                        return InteractionEffect::BeginTextEdit(target);
                    }
                    // **The ordered endpoints, not the released pointer.** A
                    // click with a linear tool has no travel and rule 1 gives
                    // it a default-length segment; taking `world` here would
                    // have collapsed it to a point. The view snapped its
                    // `target` at this same end — it reads `connector_creation`
                    // before sending the release — so the two agree.
                    let connector =
                        matches!(tool, CanvasTool::Line | CanvasTool::Arrow).then(|| {
                            let (start, end) = connector_endpoints(tool, gesture);
                            ConnectorCreation {
                                start,
                                end,
                                start_target,
                                end_target: target.node(),
                            }
                        });
                    InteractionEffect::CommitCreate {
                        tool,
                        rect,
                        connector,
                    }
                } else {
                    InteractionEffect::None
                }
            }

            (
                InteractionState::DraggingConnectorEndpoint { node, end, .. },
                InteractionEvent::PointerUp { button, .. },
            ) => {
                if button == PointerButton::Left {
                    self.state = InteractionState::Idle;
                    InteractionEffect::EndConnectorEndpointDrag { node, end }
                } else {
                    InteractionEffect::None
                }
            }

            (
                InteractionState::Connecting { source, .. },
                InteractionEvent::PointerUp { button, target, .. },
            ) => {
                if button == PointerButton::Left {
                    self.state = InteractionState::Idle;
                    InteractionEffect::CommitConnect { source, target }
                } else {
                    InteractionEffect::None
                }
            }

            (InteractionState::Idle, InteractionEvent::PointerUp { .. }) => InteractionEffect::None,

            // ---- giving up ----
            (InteractionState::Idle, InteractionEvent::Cancel) => InteractionEffect::None,
            (InteractionState::Panning { .. }, InteractionEvent::Cancel) => {
                self.state = InteractionState::Idle;
                InteractionEffect::EndPan
            }
            (InteractionState::BoxSelecting { .. }, InteractionEvent::Cancel) => {
                self.state = InteractionState::Idle;
                InteractionEffect::CancelBoxSelect
            }
            (InteractionState::DraggingNode { node, total, .. }, InteractionEvent::Cancel) => {
                self.state = InteractionState::Idle;
                InteractionEffect::CancelNodeDrag {
                    node,
                    revert: Vec2::ZERO - total,
                }
            }
            (InteractionState::Connecting { .. }, InteractionEvent::Cancel) => {
                self.state = InteractionState::Idle;
                InteractionEffect::CancelConnect
            }
            (InteractionState::CreatingShape { .. }, InteractionEvent::Cancel) => {
                self.state = InteractionState::Idle;
                InteractionEffect::CancelCreate
            }
            (InteractionState::DraggingConnectorEndpoint { .. }, InteractionEvent::Cancel) => {
                self.state = InteractionState::Idle;
                InteractionEffect::CancelConnectorEndpointDrag
            }
            (InteractionState::EditingText { .. }, InteractionEvent::Cancel) => {
                self.state = InteractionState::Idle;
                InteractionEffect::CancelTextEdit
            }

            // ---- picking up a tool (§45) ----
            //
            // **Ignored while a gesture is in progress**, for the same reason a
            // second press is: switching tools under the user's hand would put
            // the machine in a state nobody asked for, and a half-drawn
            // rectangle would commit as an ellipse. `Esc` is the way out, and
            // the view sends `Cancel` *then* `SelectTool` for exactly that —
            // which keeps this file's one-effect-per-event rule intact instead
            // of inventing a compound effect for one keystroke.
            (InteractionState::Idle, InteractionEvent::SelectTool(tool)) => {
                self.tool = tool;
                InteractionEffect::ToolChanged(tool)
            }
            (_, InteractionEvent::SelectTool(_)) => InteractionEffect::None,

            // ---- the tool lock ----
            //
            // Accepted in every state — see [`InteractionEvent::SetToolLock`]
            // — and silent when it changes nothing, so a toggle clicked twice
            // does not repaint the canvas the second time.
            (_, InteractionEvent::SetToolLock(locked)) => {
                if self.tool_locked == locked {
                    return InteractionEffect::None;
                }
                self.tool_locked = locked;
                InteractionEffect::ToolLockChanged(locked)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NODE: NodeIndex = NodeIndex::new(7);
    const HANDLE: HandleIndex = HandleIndex::new(3);

    /// **Every gesture that opens a state is captured**, or it stops the moment
    /// the pointer leaves the pane.
    ///
    /// `views::flow` calls `Window::capture_pointer` on
    /// [`InteractionEffect::starts_a_drag`], and a `Begin` effect left out of
    /// that list fails in the one way this crate keeps meeting: not at all on a
    /// large window, and for everybody else as a drag that dies at the edge.
    /// §12's resize was left out of it when it was written.
    #[test]
    fn every_effect_that_opens_a_gesture_captures_the_pointer() {
        for build in InteractionEffect::OPENS_A_GESTURE {
            let effect = build();
            assert!(
                effect.starts_a_drag(),
                "{effect:?} opens a gesture and is not captured"
            );
        }
    }

    /// §12's resize, as the machine sees it: one state, driven by the moves
    /// after it, ended by the release.
    #[test]
    fn a_resize_runs_from_a_grip_press_to_the_release() {
        use crate::geometry::ResizeCorner;

        let frame = Rect::new(Vec2::new(10.0, 10.0), Vec2::new(200.0, 100.0));
        let mut machine = InteractionMachine::new();

        let effect = machine.handle(InteractionEvent::BeginResize {
            node: NODE,
            corner: ResizeCorner::BottomRight,
            frame,
            keeps_aspect: true,
        });
        assert_eq!(effect, InteractionEffect::BeginResize { node: NODE });
        assert!(matches!(
            machine.state(),
            InteractionState::Resizing {
                aspect: Some(_),
                ..
            }
        ));

        let effect = machine.handle(InteractionEvent::PointerMove {
            screen: Vec2::new(410.0, 120.0),
            world: Vec2::new(410.0, 120.0),
        });
        let InteractionEffect::ResizeNodeTo { rect, .. } = effect else {
            panic!("a move during a resize did not resize: {effect:?}");
        };
        assert!(
            (rect.width() / rect.height() - 2.0).abs() < 1e-3,
            "the lock did not hold: {rect:?}"
        );

        // A press arriving mid-resize is ignored rather than starting a second
        // gesture under a moving hand — the rule every other state follows.
        assert_eq!(
            machine.handle(down_on(PointerButton::Left, PointerTarget::Empty)),
            InteractionEffect::None
        );

        let effect = machine.handle(InteractionEvent::PointerUp {
            button: PointerButton::Left,
            world: Vec2::new(410.0, 120.0),
            target: PointerTarget::Empty,
        });
        assert_eq!(
            effect,
            InteractionEffect::EndResize {
                node: NODE,
                changed: true
            }
        );
        assert_eq!(machine.state(), &InteractionState::Idle);
    }

    /// A grip press that never travelled is a click on a corner, and must not
    /// become an undo step.
    #[test]
    fn a_resize_that_never_moved_reports_no_change() {
        use crate::geometry::ResizeCorner;

        let mut machine = InteractionMachine::new();
        machine.handle(InteractionEvent::BeginResize {
            node: NODE,
            corner: ResizeCorner::TopLeft,
            frame: Rect::new(Vec2::ZERO, Vec2::new(50.0, 50.0)),
            keeps_aspect: false,
        });

        assert_eq!(
            machine.handle(InteractionEvent::PointerUp {
                button: PointerButton::Left,
                world: Vec2::ZERO,
                target: PointerTarget::Empty,
            }),
            InteractionEffect::EndResize {
                node: NODE,
                changed: false
            }
        );
    }

    /// A press on empty canvas.
    fn down(button: PointerButton) -> InteractionEvent {
        down_on(button, PointerTarget::Empty)
    }

    fn down_on(button: PointerButton, target: PointerTarget) -> InteractionEvent {
        InteractionEvent::PointerDown {
            screen: Vec2::new(100.0, 100.0),
            world: Vec2::new(10.0, 10.0),
            button,
            modifiers: InputModifiers::NONE,
            pan_key_held: false,
            target,
        }
    }

    fn move_to(screen: Vec2, world: Vec2) -> InteractionEvent {
        InteractionEvent::PointerMove { screen, world }
    }

    fn up(button: PointerButton) -> InteractionEvent {
        up_on(button, PointerTarget::Empty)
    }

    fn up_on(button: PointerButton, target: PointerTarget) -> InteractionEvent {
        InteractionEvent::PointerUp {
            button,
            world: Vec2::new(10.0, 10.0),
            target,
        }
    }

    // ---- node dragging (§19, §25) ---------------------------------------

    /// A press on a node body drags it. The deltas are **world** deltas and
    /// they add up to the total travel, which is what `GraphWorld::move_node`
    /// consumes one at a time.
    #[test]
    fn a_press_on_a_node_drags_it_by_the_pointers_own_delta() {
        let mut machine = InteractionMachine::new();

        let begin = machine.handle(down_on(PointerButton::Left, PointerTarget::Node(NODE)));
        assert_eq!(
            begin,
            InteractionEffect::BeginNodeDrag {
                node: NODE,
                additive: false
            }
        );
        assert_eq!(machine.dragging_node(), Some(NODE));
        assert!(begin.starts_a_drag(), "the pointer has to be captured");

        // The press was at world (10, 10); each move contributes its own step.
        let first = machine.handle(move_to(Vec2::ZERO, Vec2::new(30.0, 10.0)));
        let second = machine.handle(move_to(Vec2::ZERO, Vec2::new(30.0, 40.0)));

        assert_eq!(
            first,
            InteractionEffect::DragNodeBy {
                node: NODE,
                delta: Vec2::new(20.0, 0.0)
            }
        );
        assert_eq!(
            second,
            InteractionEffect::DragNodeBy {
                node: NODE,
                delta: Vec2::new(0.0, 30.0)
            }
        );
    }

    /// A press and release that never travelled is a *click*, not a
    /// zero-length drag — the distinction Phase 7's undo history needs.
    #[test]
    fn a_drag_that_never_moved_reports_itself_as_a_click() {
        let mut machine = InteractionMachine::new();
        machine.handle(down_on(PointerButton::Left, PointerTarget::Node(NODE)));

        let end = machine.handle(up(PointerButton::Left));

        assert_eq!(
            end,
            InteractionEffect::EndNodeDrag {
                node: NODE,
                moved: false
            }
        );
        assert!(machine.is_idle());
        assert_eq!(machine.dragging_node(), None);
    }

    #[test]
    fn a_drag_that_moved_says_so_when_it_ends() {
        let mut machine = InteractionMachine::new();
        machine.handle(down_on(PointerButton::Left, PointerTarget::Node(NODE)));
        machine.handle(move_to(Vec2::ZERO, Vec2::new(40.0, 10.0)));

        assert_eq!(
            machine.handle(up(PointerButton::Left)),
            InteractionEffect::EndNodeDrag {
                node: NODE,
                moved: true
            }
        );
    }

    /// **Cancel puts the node back exactly.** The revert is the negated total
    /// travel, however many moves it took to get there — so `Esc` mid-drag is
    /// lossless rather than approximately lossless.
    #[test]
    fn cancelling_a_drag_reverts_the_whole_travel_in_one_delta() {
        let mut machine = InteractionMachine::new();
        machine.handle(down_on(PointerButton::Left, PointerTarget::Node(NODE)));
        machine.handle(move_to(Vec2::ZERO, Vec2::new(30.0, 10.0)));
        machine.handle(move_to(Vec2::ZERO, Vec2::new(30.0, 55.0)));
        machine.handle(move_to(Vec2::ZERO, Vec2::new(12.0, 55.0)));

        assert_eq!(
            machine.handle(InteractionEvent::Cancel),
            InteractionEffect::CancelNodeDrag {
                node: NODE,
                // Pressed at (10, 10), ended at (12, 55): travelled (2, 45).
                revert: Vec2::new(-2.0, -45.0)
            }
        );
        assert!(machine.is_idle());
    }

    /// A locked node reads as empty canvas to the caller, so this is the
    /// caller's decision and not the machine's — but a press on a node while
    /// the pan key is held must still pan, because that binding wins.
    #[test]
    fn the_pan_key_wins_over_whatever_is_under_the_pointer() {
        let mut machine = InteractionMachine::new();

        let effect = machine.handle(InteractionEvent::PointerDown {
            screen: Vec2::ZERO,
            world: Vec2::ZERO,
            button: PointerButton::Left,
            modifiers: InputModifiers::NONE,
            pan_key_held: true,
            target: PointerTarget::Node(NODE),
        });

        assert_eq!(effect, InteractionEffect::BeginPan);
        assert!(machine.is_panning());
    }

    /// A middle press pans wherever it lands. Dragging the canvas out from
    /// under a node is a pan, not a node drag.
    #[test]
    fn a_middle_press_on_a_node_still_pans() {
        let mut machine = InteractionMachine::new();

        let effect = machine.handle(down_on(PointerButton::Middle, PointerTarget::Node(NODE)));

        assert_eq!(effect, InteractionEffect::BeginPan);
    }

    /// **The gesture and the propagation rule, end to end.**
    ///
    /// Every other test here checks a transition and every test in
    /// `runtime::world` checks the invalidation; this is the seam between them,
    /// which is the part a screenshot cannot verify and an interactive check
    /// would only verify once. A press on a node, three moves, a release — and
    /// the only routes rebuilt are the dragged node's own.
    #[test]
    fn dragging_a_node_through_the_machine_reroutes_only_its_own_edges() {
        use crate::{
            geometry::Vec2 as V,
            models::{ElementKind, GraphNodeKind},
            runtime::{EdgeEnd, GraphWorld},
        };

        let mut world = GraphWorld::new();
        let node = |world: &mut GraphWorld, x: f32| {
            world.create_node(
                ElementKind::GraphNode(GraphNodeKind::Default),
                V::new(x, 0.0),
                V::new(160.0, 60.0),
            )
        };
        let dragged = node(&mut world, 0.0);
        let neighbour = node(&mut world, 400.0);
        let bystander_a = node(&mut world, 800.0);
        let bystander_b = node(&mut world, 1_200.0);

        world
            .connect(EdgeEnd::node(dragged), EdgeEnd::node(neighbour))
            .expect("valid");
        let untouched = world
            .connect(EdgeEnd::node(bystander_a), EdgeEnd::node(bystander_b))
            .expect("valid");
        world.rebuild_all_geometry();
        world.dirty_mut().clear_all();

        let before = world.geometry().rebuild_count();
        let start = world.nodes().position(dragged);

        let mut machine = InteractionMachine::new();
        let mut effects = vec![machine.handle(InteractionEvent::PointerDown {
            screen: Vec2::ZERO,
            world: V::new(10.0, 10.0),
            button: PointerButton::Left,
            modifiers: InputModifiers::NONE,
            pan_key_held: false,
            target: PointerTarget::Node(dragged),
        })];
        for step in 1..=3 {
            effects
                .push(machine.handle(move_to(Vec2::ZERO, V::new(10.0 + step as f32 * 15.0, 10.0))));
        }
        effects.push(machine.handle(up(PointerButton::Left)));

        // The view's `apply`, in miniature: the only effect that touches the
        // world is the move, and it goes straight to `GraphWorld::move_node`.
        for effect in effects {
            if let InteractionEffect::DragNodeBy { node, delta } = effect {
                world.move_node(node, delta);
            }
        }

        assert_eq!(
            world.nodes().position(dragged),
            start + V::new(45.0, 0.0),
            "the node ends where the pointer left it"
        );
        assert_eq!(world.dirty().dirty_edges().len(), 1);
        assert_eq!(world.dirty().spatial_updates(), &[dragged]);
        assert!(
            world.geometry().is_valid(untouched),
            "an edge between two other nodes was never invalidated"
        );
        assert_eq!(world.rebuild_dirty_geometry(), 1);
        assert_eq!(world.geometry().rebuild_count() - before, 1);
    }

    // ---- connecting (§4, §8) --------------------------------------------

    /// A press on a **handle** starts a connection rather than a drag — the
    /// specificity order that lets one button mean four gestures.
    #[test]
    fn a_press_on_a_handle_starts_a_connection_rather_than_a_drag() {
        let mut machine = InteractionMachine::new();

        let effect = machine.handle(down_on(
            PointerButton::Left,
            PointerTarget::Handle {
                node: NODE,
                handle: HANDLE,
            },
        ));

        let source = ConnectionSource {
            node: NODE,
            handle: HANDLE,
        };
        assert_eq!(effect, InteractionEffect::BeginConnect(source));
        assert_eq!(machine.dragging_node(), None, "a handle is not a body");
        assert_eq!(
            machine.pending_connection(),
            Some(PendingConnection {
                source,
                current_world: Vec2::new(10.0, 10.0)
            })
        );
    }

    /// The preview's loose end follows the pointer — the painter reads it from
    /// the machine rather than being told, so a repaint from any cause draws
    /// the preview in the right place.
    #[test]
    fn the_pending_connection_follows_the_pointer() {
        let mut machine = InteractionMachine::new();
        machine.handle(down_on(
            PointerButton::Left,
            PointerTarget::Handle {
                node: NODE,
                handle: HANDLE,
            },
        ));

        let effect = machine.handle(move_to(Vec2::ZERO, Vec2::new(300.0, 120.0)));

        assert!(matches!(effect, InteractionEffect::UpdateConnect(_)));
        assert_eq!(
            machine.pending_connection().map(|p| p.current_world),
            Some(Vec2::new(300.0, 120.0))
        );
    }

    /// **The machine does not validate.** It says where the drop landed and
    /// hands the pair to whoever owns §4's rules; a drop on a full handle and a
    /// drop on an empty one are the same transition here.
    #[test]
    fn a_dropped_connection_reports_where_it_landed_and_judges_nothing() {
        let mut machine = InteractionMachine::new();
        machine.handle(down_on(
            PointerButton::Left,
            PointerTarget::Handle {
                node: NODE,
                handle: HANDLE,
            },
        ));

        let landing = PointerTarget::Handle {
            node: NodeIndex::new(9),
            handle: HandleIndex::new(2),
        };
        let effect = machine.handle(up_on(PointerButton::Left, landing));

        assert_eq!(
            effect,
            InteractionEffect::CommitConnect {
                source: ConnectionSource {
                    node: NODE,
                    handle: HANDLE
                },
                target: landing
            }
        );
        assert!(machine.is_idle());
        assert_eq!(machine.pending_connection(), None);
    }

    /// §4's whole-node connection mode reaches the world as a node target, so
    /// dropping on a body is a commit rather than a cancel.
    #[test]
    fn a_connection_dropped_on_a_body_still_commits() {
        let mut machine = InteractionMachine::new();
        machine.handle(down_on(
            PointerButton::Left,
            PointerTarget::Handle {
                node: NODE,
                handle: HANDLE,
            },
        ));

        let effect = machine.handle(up_on(
            PointerButton::Left,
            PointerTarget::Node(NodeIndex::new(4)),
        ));

        assert!(matches!(
            effect,
            InteractionEffect::CommitConnect {
                target: PointerTarget::Node(_),
                ..
            }
        ));
    }

    #[test]
    fn cancelling_a_connection_leaves_no_trace() {
        let mut machine = InteractionMachine::new();
        machine.handle(down_on(
            PointerButton::Left,
            PointerTarget::Handle {
                node: NODE,
                handle: HANDLE,
            },
        ));
        machine.handle(move_to(Vec2::ZERO, Vec2::new(200.0, 200.0)));

        assert_eq!(
            machine.handle(InteractionEvent::Cancel),
            InteractionEffect::CancelConnect
        );
        assert!(machine.is_idle());
        assert_eq!(machine.pending_connection(), None);
    }

    /// Every gesture that begins a drag has to capture the pointer, or it dies
    /// the moment the cursor leaves the pane. One assertion over all four so a
    /// fifth cannot be added without noticing.
    #[test]
    fn every_beginning_gesture_captures_the_pointer() {
        for target in [
            PointerTarget::Empty,
            PointerTarget::Node(NODE),
            PointerTarget::Handle {
                node: NODE,
                handle: HANDLE,
            },
        ] {
            let mut machine = InteractionMachine::new();
            let effect = machine.handle(down_on(PointerButton::Left, target));
            assert!(effect.starts_a_drag(), "{target:?} -> {effect:?}");
            assert!(effect.needs_repaint(), "{target:?} -> {effect:?}");
        }

        let mut machine = InteractionMachine::new();
        assert!(
            machine
                .handle(down_on(PointerButton::Middle, PointerTarget::Empty))
                .starts_a_drag()
        );
    }

    #[test]
    fn a_fresh_machine_is_idle_and_draws_no_rectangle() {
        let machine = InteractionMachine::new();
        assert!(machine.is_idle());
        assert!(!machine.is_panning());
        assert_eq!(machine.selection_rect(), None);
    }

    #[test]
    fn a_middle_press_begins_a_pan_and_a_release_ends_it() {
        let mut machine = InteractionMachine::new();

        assert_eq!(
            machine.handle(down(PointerButton::Middle)),
            InteractionEffect::BeginPan
        );
        assert!(machine.is_panning());

        assert_eq!(
            machine.handle(move_to(Vec2::new(140.0, 130.0), Vec2::ZERO)),
            InteractionEffect::PanBy(Vec2::new(40.0, 30.0))
        );

        assert_eq!(
            machine.handle(up(PointerButton::Middle)),
            InteractionEffect::EndPan
        );
        assert!(machine.is_idle());
    }

    /// Pan deltas are relative to the last move, not to the press. If they were
    /// absolute the view would accelerate away from the pointer.
    #[test]
    fn pan_deltas_are_incremental() {
        let mut machine = InteractionMachine::new();
        machine.handle(down(PointerButton::Middle));

        assert_eq!(
            machine.handle(move_to(Vec2::new(110.0, 100.0), Vec2::ZERO)),
            InteractionEffect::PanBy(Vec2::new(10.0, 0.0))
        );
        assert_eq!(
            machine.handle(move_to(Vec2::new(115.0, 100.0), Vec2::ZERO)),
            InteractionEffect::PanBy(Vec2::new(5.0, 0.0)),
        );
    }

    #[test]
    fn space_turns_a_left_press_into_a_pan() {
        let mut machine = InteractionMachine::new();
        let effect = machine.handle(InteractionEvent::PointerDown {
            screen: Vec2::ZERO,
            world: Vec2::ZERO,
            button: PointerButton::Left,
            modifiers: InputModifiers::NONE,
            pan_key_held: true,
            target: PointerTarget::Empty,
        });

        assert_eq!(effect, InteractionEffect::BeginPan);
        assert!(machine.is_panning());
        assert_eq!(machine.selection_rect(), None);
    }

    #[test]
    fn a_plain_left_press_begins_a_box_selection() {
        let mut machine = InteractionMachine::new();
        let effect = machine.handle(down(PointerButton::Left));

        assert_eq!(
            effect,
            InteractionEffect::BeginBoxSelect(Rect::new(Vec2::new(10.0, 10.0), Vec2::ZERO))
        );
        assert_eq!(
            machine.selection_rect(),
            Some(Rect::new(Vec2::new(10.0, 10.0), Vec2::ZERO))
        );
    }

    /// Dragging up and to the left has to give the same rectangle as dragging
    /// down and to the right — the reason the corners are normalised rather
    /// than subtracted.
    #[test]
    fn a_backwards_drag_normalises() {
        let mut machine = InteractionMachine::new();
        machine.handle(down(PointerButton::Left));
        machine.handle(move_to(Vec2::ZERO, Vec2::new(-30.0, -50.0)));

        let rect = machine
            .selection_rect()
            .expect("a rectangle is in progress");
        assert_eq!(rect.origin, Vec2::new(-30.0, -50.0));
        assert_eq!(rect.size, Vec2::new(40.0, 60.0));
    }

    #[test]
    fn releasing_commits_the_selection_and_returns_to_idle() {
        let mut machine = InteractionMachine::new();
        machine.handle(down(PointerButton::Left));
        machine.handle(move_to(Vec2::ZERO, Vec2::new(60.0, 40.0)));

        let effect = machine.handle(up(PointerButton::Left));

        assert_eq!(
            effect,
            InteractionEffect::CommitBoxSelect(BoxSelection {
                rect: Rect::new(Vec2::new(10.0, 10.0), Vec2::new(50.0, 30.0)),
                additive: false,
            })
        );
        assert!(machine.is_idle());
        assert_eq!(machine.selection_rect(), None);
    }

    /// Shift is read at press time. Letting go of it before the mouse button
    /// must not silently turn an additive select into a replacing one.
    #[test]
    fn additive_is_captured_when_the_drag_starts_not_when_it_ends() {
        let mut machine = InteractionMachine::new();
        machine.handle(InteractionEvent::PointerDown {
            screen: Vec2::ZERO,
            world: Vec2::ZERO,
            button: PointerButton::Left,
            modifiers: InputModifiers::shift(),
            pan_key_held: false,
            target: PointerTarget::Empty,
        });
        machine.handle(move_to(Vec2::ZERO, Vec2::splat(10.0)));

        match machine.handle(up(PointerButton::Left)) {
            InteractionEffect::CommitBoxSelect(selection) => assert!(selection.additive),
            other => panic!("expected a commit, got {other:?}"),
        }
    }

    #[test]
    fn command_is_additive_too() {
        assert!(InputModifiers::shift().is_additive());
        assert!(
            InputModifiers {
                command: true,
                ..InputModifiers::NONE
            }
            .is_additive()
        );
        assert!(!InputModifiers::NONE.is_additive());
        assert!(
            !InputModifiers {
                alt: true,
                ..InputModifiers::NONE
            }
            .is_additive()
        );
    }

    #[test]
    fn escape_abandons_a_box_selection_without_committing_it() {
        let mut machine = InteractionMachine::new();
        machine.handle(down(PointerButton::Left));
        machine.handle(move_to(Vec2::ZERO, Vec2::splat(40.0)));

        assert_eq!(
            machine.handle(InteractionEvent::Cancel),
            InteractionEffect::CancelBoxSelect
        );
        assert!(machine.is_idle());
        assert_eq!(machine.selection_rect(), None);
    }

    #[test]
    fn escape_ends_a_pan() {
        let mut machine = InteractionMachine::new();
        machine.handle(down(PointerButton::Middle));

        assert_eq!(
            machine.handle(InteractionEvent::Cancel),
            InteractionEffect::EndPan
        );
        assert!(machine.is_idle());
    }

    /// Only the button that started the drag ends it. Right-clicking during a
    /// left-drag is not a release.
    #[test]
    fn a_different_button_does_not_end_the_drag() {
        let mut machine = InteractionMachine::new();
        machine.handle(down(PointerButton::Middle));

        assert_eq!(
            machine.handle(up(PointerButton::Left)),
            InteractionEffect::None
        );
        assert!(machine.is_panning());

        let mut machine = InteractionMachine::new();
        machine.handle(down(PointerButton::Left));
        assert_eq!(
            machine.handle(up(PointerButton::Right)),
            InteractionEffect::None
        );
        assert!(machine.selection_rect().is_some());
    }

    /// A second press mid-drag must not switch modes under the user's hand.
    #[test]
    fn a_press_while_busy_changes_nothing() {
        let mut machine = InteractionMachine::new();
        machine.handle(down(PointerButton::Middle));
        let before = *machine.state();

        assert_eq!(
            machine.handle(down(PointerButton::Left)),
            InteractionEffect::None
        );
        assert_eq!(*machine.state(), before);
    }

    #[test]
    fn a_right_press_starts_nothing_yet() {
        let mut machine = InteractionMachine::new();
        assert_eq!(
            machine.handle(down(PointerButton::Right)),
            InteractionEffect::None
        );
        assert!(machine.is_idle());
    }

    #[test]
    fn moving_while_idle_costs_nothing_and_repaints_nothing() {
        let mut machine = InteractionMachine::new();
        let effect = machine.handle(move_to(Vec2::splat(400.0), Vec2::splat(40.0)));

        assert_eq!(effect, InteractionEffect::None);
        assert!(!effect.needs_repaint());
        assert!(machine.is_idle());
    }

    /// Which effects the view has to act on. Stated as a test because the two
    /// predicates are the whole of the view's `match`, and getting
    /// `needs_repaint` wrong is either a stuck frame or an idle repaint loop.
    #[test]
    fn the_effects_that_capture_the_pointer_and_the_ones_that_repaint() {
        let rect = Rect::ZERO;
        let selection = BoxSelection {
            rect,
            additive: false,
        };

        for effect in [
            InteractionEffect::BeginPan,
            InteractionEffect::BeginBoxSelect(rect),
        ] {
            assert!(effect.starts_a_drag(), "{effect:?}");
        }
        for effect in [
            InteractionEffect::None,
            InteractionEffect::PanBy(Vec2::ZERO),
            InteractionEffect::EndPan,
            InteractionEffect::UpdateBoxSelect(rect),
            InteractionEffect::CommitBoxSelect(selection),
            InteractionEffect::CancelBoxSelect,
        ] {
            assert!(!effect.starts_a_drag(), "{effect:?}");
        }

        assert!(!InteractionEffect::None.needs_repaint());
        for effect in [
            InteractionEffect::BeginPan,
            InteractionEffect::PanBy(Vec2::ZERO),
            InteractionEffect::EndPan,
            InteractionEffect::BeginBoxSelect(rect),
            InteractionEffect::UpdateBoxSelect(rect),
            InteractionEffect::CommitBoxSelect(selection),
            InteractionEffect::CancelBoxSelect,
        ] {
            assert!(effect.needs_repaint(), "{effect:?}");
        }
    }

    /// The machine has to survive being driven by a stream it did not expect —
    /// a lost mouse-up, a move with no press, a cancel out of nowhere. It ends
    /// idle every time, and never panics.
    #[test]
    fn any_sequence_of_events_leaves_it_in_a_state_it_can_recover_from() {
        let events = [
            down(PointerButton::Left),
            down(PointerButton::Middle),
            move_to(Vec2::splat(1.0), Vec2::splat(1.0)),
            up(PointerButton::Right),
            InteractionEvent::Cancel,
            up(PointerButton::Left),
            move_to(Vec2::splat(9.0), Vec2::splat(9.0)),
            down(PointerButton::Right),
            up(PointerButton::Middle),
        ];

        // Every rotation of the sequence, so no ordering is privileged.
        for start in 0..events.len() {
            let mut machine = InteractionMachine::new();
            for offset in 0..events.len() {
                machine.handle(events[(start + offset) % events.len()]);
            }
            machine.handle(InteractionEvent::Cancel);
            assert!(machine.is_idle(), "stuck after rotation {start}");
        }
    }

    // ---- §45's tools ----------------------------------------------------

    fn press(
        button: PointerButton,
        target: PointerTarget,
        screen: Vec2,
        modifiers: InputModifiers,
    ) -> InteractionEvent {
        InteractionEvent::PointerDown {
            screen,
            world: screen,
            button,
            modifiers,
            pan_key_held: false,
            target,
        }
    }

    /// **The whole point of the tool**: the same press means a different
    /// gesture depending only on what is selected in the palette, and the
    /// document is not consulted.
    #[test]
    fn the_same_press_means_a_different_gesture_under_each_tool() {
        let cases = [
            (CanvasTool::Select, "box select"),
            (CanvasTool::Hand, "pan"),
            (CanvasTool::Rectangle, "create"),
        ];

        let mut seen = Vec::new();
        for (tool, name) in cases {
            let mut machine = InteractionMachine::new();
            machine.handle(InteractionEvent::SelectTool(tool));
            seen.push((
                name,
                machine.handle(press(
                    PointerButton::Left,
                    PointerTarget::Empty,
                    Vec2::new(5.0, 5.0),
                    InputModifiers::NONE,
                )),
            ));
        }

        assert!(matches!(seen[0].1, InteractionEffect::BeginBoxSelect(_)));
        assert_eq!(seen[1].1, InteractionEffect::BeginPan);
        assert!(matches!(seen[2].1, InteractionEffect::BeginCreate { .. }));
    }

    /// **A creating tool draws over whatever is there.** Pressing on a node
    /// with the rectangle tool must not start a node drag — an Excalidraw user
    /// drawing across a diagram expects a rectangle, and a tool that sometimes
    /// moved the thing underneath would be unusable over a dense document.
    #[test]
    fn a_creating_tool_ignores_what_the_press_landed_on() {
        for target in [
            PointerTarget::Node(NODE),
            PointerTarget::Handle {
                node: NODE,
                handle: HANDLE,
            },
            PointerTarget::Empty,
        ] {
            let mut machine = InteractionMachine::new();
            machine.handle(InteractionEvent::SelectTool(CanvasTool::Ellipse));
            let effect = machine.handle(press(
                PointerButton::Left,
                target,
                Vec2::ZERO,
                InputModifiers::NONE,
            ));
            assert!(
                matches!(effect, InteractionEffect::BeginCreate { .. }),
                "{target:?} diverted the ellipse tool"
            );
        }
    }

    /// The full creation gesture, and the rectangle that comes out of it: a
    /// press, some moves, a release, one element's worth of geometry.
    #[test]
    fn a_press_moves_and_a_release_produce_the_dragged_rectangle() {
        let mut machine = InteractionMachine::new();
        machine.handle(InteractionEvent::SelectTool(CanvasTool::Rectangle));

        let begin = machine.handle(press(
            PointerButton::Left,
            PointerTarget::Empty,
            Vec2::new(10.0, 10.0),
            InputModifiers::NONE,
        ));
        assert!(begin.starts_a_drag(), "the pointer has to be captured");

        for step in 1..=4 {
            let at = Vec2::new(10.0 + 20.0 * step as f32, 10.0 + 10.0 * step as f32);
            let effect = machine.handle(move_to(at, at));
            assert!(matches!(effect, InteractionEffect::UpdateCreate { .. }));
        }

        // The preview and the commit must agree; a user who sees one rectangle
        // and gets another has been lied to.
        let previewed = machine.creation_preview().expect("a drag is in progress");
        let effect = machine.handle(up(PointerButton::Left));

        let InteractionEffect::CommitCreate { tool, rect, .. } = effect else {
            panic!("a released creation must commit, got {effect:?}");
        };
        assert_eq!(tool, CanvasTool::Rectangle);
        assert_eq!((tool, rect), previewed);
        assert_eq!(rect.origin, Vec2::new(10.0, 10.0));
        assert_eq!(rect.size, Vec2::new(80.0, 40.0));
        assert!(machine.is_idle());
    }

    /// A click with a creating tool places the tool's default size — the
    /// gesture the brief names for the graph node, and the one every other
    /// creating tool answers too.
    #[test]
    fn a_click_with_a_creating_tool_places_a_default_sized_element() {
        let mut machine = InteractionMachine::new();
        machine.handle(InteractionEvent::SelectTool(CanvasTool::GraphNode));
        machine.handle(press(
            PointerButton::Left,
            PointerTarget::Empty,
            Vec2::new(100.0, 100.0),
            InputModifiers::NONE,
        ));

        let InteractionEffect::CommitCreate { rect, .. } = machine.handle(up(PointerButton::Left))
        else {
            panic!("a click must still create");
        };
        assert_eq!(rect.size, CanvasTool::GraphNode.default_size());
        assert_eq!(rect.center(), Vec2::new(100.0, 100.0));
    }

    /// Shift held at the press squares the box, and it stays squared for the
    /// whole drag — the modifier is captured, not re-read.
    #[test]
    fn shift_at_the_press_constrains_the_whole_drag() {
        let mut machine = InteractionMachine::new();
        machine.handle(InteractionEvent::SelectTool(CanvasTool::Ellipse));
        machine.handle(press(
            PointerButton::Left,
            PointerTarget::Empty,
            Vec2::ZERO,
            InputModifiers::shift(),
        ));
        machine.handle(move_to(Vec2::new(200.0, 40.0), Vec2::new(200.0, 40.0)));

        let InteractionEffect::CommitCreate { rect, .. } = machine.handle(up(PointerButton::Left))
        else {
            panic!("expected a commit");
        };
        assert_eq!(rect.size.x, rect.size.y, "shift must give a circle");
    }

    /// **An abandoned creation leaves nothing**: the tool never wrote to the
    /// document, so there is no element to remove and no undo step to discard.
    #[test]
    fn cancelling_a_creation_says_so_and_returns_to_rest() {
        let mut machine = InteractionMachine::new();
        machine.handle(InteractionEvent::SelectTool(CanvasTool::Diamond));
        machine.handle(press(
            PointerButton::Left,
            PointerTarget::Empty,
            Vec2::ZERO,
            InputModifiers::NONE,
        ));
        machine.handle(move_to(Vec2::splat(60.0), Vec2::splat(60.0)));

        assert_eq!(
            machine.handle(InteractionEvent::Cancel),
            InteractionEffect::CancelCreate
        );
        assert!(machine.is_idle());
        assert_eq!(machine.creation_preview(), None);
        // The tool survives the cancel; `Esc` back to Select is the *view's*
        // second event, not this one's business.
        assert_eq!(machine.tool(), CanvasTool::Diamond);
    }

    /// **Picking a tool arms it, with no second click.**
    ///
    /// The requirement reads like a behaviour to add and is really a property
    /// to protect: one `SelectTool` and the very next press is already a
    /// creation. An "arm the tool" step would show up here as a first press
    /// that produced something other than `BeginCreate`.
    #[test]
    fn a_tool_draws_on_the_first_press_after_it_is_picked_up() {
        for tool in CanvasTool::ALL.iter().filter(|tool| tool.creates()) {
            let mut machine = InteractionMachine::new();
            machine.handle(InteractionEvent::SelectTool(*tool));

            let effect = machine.handle(press(
                PointerButton::Left,
                PointerTarget::Empty,
                Vec2::ZERO,
                InputModifiers::NONE,
            ));
            assert!(
                matches!(effect, InteractionEffect::BeginCreate { .. }),
                "{} needed a second press before it drew: {effect:?}",
                tool.name()
            );
            assert!(machine.creation_preview().is_some());
        }
    }

    /// **Draw, finish, land back on Select** — the Excalidraw default, and the
    /// behaviour the lock below switches off.
    ///
    /// Asserted for every creating tool rather than for a rectangle, because
    /// the rule is about creation and not about shapes: an edge tool that kept
    /// itself while the rectangle did not would be exactly the inconsistency
    /// this is here to prevent.
    #[test]
    fn finishing_a_drawing_returns_to_the_select_tool() {
        for tool in CanvasTool::ALL
            .iter()
            .filter(|tool| tool.creates() && !tool.edits_text_on_release())
        {
            let mut machine = InteractionMachine::new();
            machine.handle(InteractionEvent::SelectTool(*tool));
            machine.handle(press(
                PointerButton::Left,
                PointerTarget::Empty,
                Vec2::ZERO,
                InputModifiers::NONE,
            ));
            machine.handle(move_to(Vec2::splat(80.0), Vec2::splat(80.0)));

            let InteractionEffect::CommitCreate { tool: drawn, .. } =
                machine.handle(up(PointerButton::Left))
            else {
                panic!("{} did not commit", tool.name());
            };

            assert_eq!(drawn, *tool, "the gesture committed as another tool");
            assert_eq!(
                machine.tool(),
                CanvasTool::Select,
                "{} stayed armed after drawing",
                tool.name()
            );
        }
    }

    /// **With the lock on, the tool survives the drawing** — so a user drawing
    /// six rectangles picks the tool up once.
    #[test]
    fn a_locked_tool_survives_the_drawing_that_finishes() {
        for tool in CanvasTool::ALL.iter().filter(|tool| tool.creates()) {
            let mut machine = InteractionMachine::new();
            assert_eq!(
                machine.handle(InteractionEvent::SetToolLock(true)),
                InteractionEffect::ToolLockChanged(true)
            );
            machine.handle(InteractionEvent::SelectTool(*tool));

            // Twice, because "it stayed once" and "it stays" are different
            // claims and only the second is the feature.
            for _ in 0..2 {
                machine.handle(press(
                    PointerButton::Left,
                    PointerTarget::Empty,
                    Vec2::ZERO,
                    InputModifiers::NONE,
                ));
                machine.handle(move_to(Vec2::splat(80.0), Vec2::splat(80.0)));
                machine.handle(up(PointerButton::Left));
                assert_eq!(
                    machine.tool(),
                    *tool,
                    "{} was dropped despite the lock",
                    tool.name()
                );
            }
        }
    }

    /// The lock is off to begin with, is idempotent, and is accepted in the
    /// middle of a gesture — a user who decides mid-rectangle that they want
    /// three more must not have to abandon the first one to say so.
    #[test]
    fn the_lock_starts_off_and_can_be_set_during_a_gesture() {
        let mut machine = InteractionMachine::new();
        assert!(!machine.tool_locked());
        assert_eq!(
            machine.handle(InteractionEvent::SetToolLock(false)),
            InteractionEffect::None,
            "setting the lock to what it already is must not repaint"
        );

        machine.handle(InteractionEvent::SelectTool(CanvasTool::Rectangle));
        machine.handle(press(
            PointerButton::Left,
            PointerTarget::Empty,
            Vec2::ZERO,
            InputModifiers::NONE,
        ));
        assert_eq!(
            machine.handle(InteractionEvent::SetToolLock(true)),
            InteractionEffect::ToolLockChanged(true),
            "the lock must be accepted mid-gesture, unlike a tool change"
        );

        machine.handle(move_to(Vec2::splat(80.0), Vec2::splat(80.0)));
        machine.handle(up(PointerButton::Left));
        assert_eq!(machine.tool(), CanvasTool::Rectangle);
    }

    /// The lock is about *creation* and nothing else: it must not change what
    /// the two navigating tools do, and it must not turn a node drag into
    /// something that reverts the tool.
    #[test]
    fn the_lock_does_not_touch_a_gesture_that_creates_nothing() {
        let mut machine = InteractionMachine::new();
        machine.handle(InteractionEvent::SetToolLock(true));
        machine.handle(down_on(PointerButton::Left, PointerTarget::Node(NODE)));
        machine.handle(up(PointerButton::Left));

        assert_eq!(machine.tool(), CanvasTool::Select);
        assert!(machine.tool_locked());
    }

    /// A tool change mid-gesture is ignored rather than applied, for the same
    /// reason a second press is: switching under the user's hand would commit a
    /// half-drawn rectangle as something else.
    #[test]
    fn a_tool_change_during_a_gesture_is_ignored() {
        let mut machine = InteractionMachine::new();
        machine.handle(InteractionEvent::SelectTool(CanvasTool::Rectangle));
        machine.handle(press(
            PointerButton::Left,
            PointerTarget::Empty,
            Vec2::ZERO,
            InputModifiers::NONE,
        ));

        assert_eq!(
            machine.handle(InteractionEvent::SelectTool(CanvasTool::Ellipse)),
            InteractionEffect::None
        );
        assert_eq!(machine.tool(), CanvasTool::Rectangle);

        let InteractionEffect::CommitCreate { tool, .. } = machine.handle(up(PointerButton::Left))
        else {
            panic!("expected a commit");
        };
        assert_eq!(tool, CanvasTool::Rectangle, "the gesture kept its own tool");
    }

    /// **`Esc` as the view sends it**: cancel, then Select. Written here rather
    /// than only in the view because the ordering is the contract — the second
    /// event is refused if the first has not settled the machine.
    #[test]
    fn cancel_then_select_is_how_escape_gets_back_to_the_select_tool() {
        let mut machine = InteractionMachine::new();
        machine.handle(InteractionEvent::SelectTool(CanvasTool::Arrow));
        machine.handle(press(
            PointerButton::Left,
            PointerTarget::Empty,
            Vec2::ZERO,
            InputModifiers::NONE,
        ));

        machine.handle(InteractionEvent::Cancel);
        assert_eq!(
            machine.handle(InteractionEvent::SelectTool(CanvasTool::Select)),
            InteractionEffect::ToolChanged(CanvasTool::Select)
        );
        assert_eq!(machine.tool(), CanvasTool::Select);
        assert!(machine.is_idle());
    }

    /// §45's rule, as far as this file can see it: activating a tool produces
    /// an effect that says only "repaint", never one that edits.
    #[test]
    fn activating_a_tool_produces_no_editing_effect() {
        for tool in CanvasTool::ALL {
            let mut machine = InteractionMachine::new();
            assert_eq!(
                machine.handle(InteractionEvent::SelectTool(*tool)),
                InteractionEffect::ToolChanged(*tool)
            );
            assert!(machine.is_idle(), "{} left the machine busy", tool.name());
        }
    }

    // ---- §9's text editing ----------------------------------------------

    use crate::models::EdgeIndex;

    fn double_click(target: PointerTarget, world: Vec2) -> InteractionEvent {
        InteractionEvent::DoubleClick { world, target }
    }

    /// **The three things a double-click can land on, and the three targets
    /// they mean.** This is requirement 4 in one test: a node edits its text,
    /// an edge its label, empty canvas places new text.
    #[test]
    fn a_double_click_opens_a_caret_on_whatever_it_landed_on() {
        let edge = EdgeIndex::new(4);
        let cases = [
            (PointerTarget::Node(NODE), TextTarget::Node(NODE)),
            (PointerTarget::Edge(edge), TextTarget::Edge(edge)),
            (
                // A handle is *on* its node and carries no text of its own, so
                // it edits the node — which is what a user aiming at the edge
                // of a small node actually asked for.
                PointerTarget::Handle {
                    node: NODE,
                    handle: HANDLE,
                },
                TextTarget::Node(NODE),
            ),
        ];

        for (landed, expected) in cases {
            let mut machine = InteractionMachine::new();
            assert_eq!(
                machine.handle(double_click(landed, Vec2::new(50.0, 50.0))),
                InteractionEffect::BeginTextEdit(expected),
                "{landed:?}"
            );
            assert!(matches!(
                machine.state(),
                InteractionState::EditingText { .. }
            ));
        }

        // Empty canvas is the fourth: a *pending* element, centred on the
        // pointer, through the same `creation_rect` a Text-tool click uses.
        let mut machine = InteractionMachine::new();
        let InteractionEffect::BeginTextEdit(TextTarget::New(rect)) =
            machine.handle(double_click(PointerTarget::Empty, Vec2::new(300.0, -50.0)))
        else {
            panic!("empty canvas must place new text");
        };
        assert_eq!(rect.center(), Vec2::new(300.0, -50.0));
        assert_eq!(rect.size, CanvasTool::Text.default_size());
    }

    /// **The Text tool's release opens a caret rather than committing.**
    ///
    /// The negative half is what stops an invisible element: no `CommitCreate`
    /// is ever emitted for text, so nothing downstream can add one.
    #[test]
    fn the_text_tool_finishes_a_drag_by_opening_a_caret() {
        let mut machine = InteractionMachine::new();
        machine.handle(InteractionEvent::SelectTool(CanvasTool::Text));
        machine.handle(press(
            PointerButton::Left,
            PointerTarget::Empty,
            Vec2::new(20.0, 30.0),
            InputModifiers::NONE,
        ));
        machine.handle(move_to(Vec2::new(220.0, 52.0), Vec2::new(220.0, 52.0)));

        let InteractionEffect::BeginTextEdit(TextTarget::New(rect)) =
            machine.handle(up(PointerButton::Left))
        else {
            panic!("the text tool must not commit an element");
        };
        assert_eq!(rect.origin, Vec2::new(20.0, 30.0));
        assert_eq!(rect.size, Vec2::new(200.0, 22.0));
        assert_eq!(
            machine.tool(),
            CanvasTool::Select,
            "it still lands back on Select, lock aside"
        );
    }

    /// **A caret is left three ways, and only two of them reach the document.**
    #[test]
    fn a_caret_is_committed_by_enter_or_a_press_and_abandoned_by_escape() {
        let target = TextTarget::Node(NODE);

        let mut by_enter = InteractionMachine::new();
        by_enter.handle(double_click(PointerTarget::Node(NODE), Vec2::ZERO));
        assert_eq!(
            by_enter.handle(InteractionEvent::FinishTextEdit),
            InteractionEffect::CommitTextEdit(target)
        );
        assert!(by_enter.is_idle());

        // Clicking away commits, which is what it means everywhere else — and
        // the press is consumed rather than also starting a gesture, because
        // this file's rule is one effect per event.
        let mut by_press = InteractionMachine::new();
        by_press.handle(double_click(PointerTarget::Node(NODE), Vec2::ZERO));
        assert_eq!(
            by_press.handle(press(
                PointerButton::Left,
                PointerTarget::Empty,
                Vec2::splat(400.0),
                InputModifiers::NONE
            )),
            InteractionEffect::CommitTextEdit(target)
        );
        assert!(by_press.is_idle());

        let mut by_escape = InteractionMachine::new();
        by_escape.handle(double_click(PointerTarget::Node(NODE), Vec2::ZERO));
        assert_eq!(
            by_escape.handle(InteractionEvent::Cancel),
            InteractionEffect::CancelTextEdit
        );
        assert!(by_escape.is_idle());
    }

    /// A caret is not a drag, so the pointer moving over one and any stray
    /// release both mean nothing — and a second double-click cannot open an
    /// editor over the first.
    #[test]
    fn a_caret_ignores_moves_releases_and_a_second_double_click() {
        let mut machine = InteractionMachine::new();
        machine.handle(double_click(PointerTarget::Node(NODE), Vec2::ZERO));

        for event in [
            move_to(Vec2::splat(9.0), Vec2::splat(9.0)),
            up(PointerButton::Left),
            double_click(PointerTarget::Node(NODE), Vec2::ZERO),
        ] {
            assert_eq!(machine.handle(event), InteractionEffect::None);
            assert!(matches!(
                machine.state(),
                InteractionState::EditingText { .. }
            ));
        }
    }

    /// A double-click that arrives mid-drag is a stray second press the
    /// platform coalesced. Opening an editor under a moving hand is worse than
    /// ignoring it.
    #[test]
    fn a_double_click_during_a_gesture_is_ignored() {
        let mut machine = InteractionMachine::new();
        machine.handle(down_on(PointerButton::Left, PointerTarget::Node(NODE)));

        assert_eq!(
            machine.handle(double_click(PointerTarget::Node(NODE), Vec2::ZERO)),
            InteractionEffect::None
        );
        assert!(machine.dragging_node().is_some());
    }

    /// **A press on an edge selects it** (Phase 10.5) and starts no gesture.
    ///
    /// Staying `Idle` is the assertion that matters: an edge has no drag, so
    /// the moves after this press must mean nothing, and `Idle` already says
    /// that for every event. A state entered only to ignore everything would be
    /// a state to keep correct for no behaviour.
    #[test]
    fn a_press_on_an_edge_selects_it_and_starts_no_gesture() {
        let mut machine = InteractionMachine::new();
        let edge = EdgeIndex::new(1);

        let effect = machine.handle(down_on(PointerButton::Left, PointerTarget::Edge(edge)));

        assert_eq!(
            effect,
            InteractionEffect::SelectEdge {
                edge,
                additive: false
            }
        );
        assert!(machine.is_idle(), "an edge press opened a gesture");
        assert!(!effect.starts_a_drag(), "there is nothing to capture for");
        assert!(effect.needs_repaint(), "the selection ring has to be drawn");

        // The moves and the release after it are ordinary `Idle` events.
        assert_eq!(
            machine.handle(move_to(Vec2::splat(50.0), Vec2::splat(50.0))),
            InteractionEffect::None
        );
        assert_eq!(
            machine.handle(up(PointerButton::Left)),
            InteractionEffect::None
        );
    }

    /// **A press on empty canvas still starts a band**, which is the half of
    /// this that is easy to trade away: the edge arm was carved out of the arm
    /// that does this.
    #[test]
    fn a_press_on_empty_canvas_still_starts_a_band() {
        let mut machine = InteractionMachine::new();

        let effect = machine.handle(down_on(PointerButton::Left, PointerTarget::Empty));

        assert!(matches!(effect, InteractionEffect::BeginBoxSelect(_)));
        assert!(matches!(
            machine.state(),
            InteractionState::BoxSelecting { .. }
        ));
    }

    /// **Shift extends, for an edge and for a node alike.**
    ///
    /// One test over both because the requirement is that they agree — a canvas
    /// where shift means "add" on one kind of element and "replace" on another
    /// is worse than one where it never worked.
    #[test]
    fn shift_makes_a_press_additive_for_both_kinds() {
        for target in [
            PointerTarget::Edge(EdgeIndex::new(1)),
            PointerTarget::Node(NODE),
        ] {
            let mut machine = InteractionMachine::new();
            let effect = machine.handle(InteractionEvent::PointerDown {
                screen: Vec2::splat(100.0),
                world: Vec2::splat(10.0),
                button: PointerButton::Left,
                modifiers: InputModifiers::shift(),
                pan_key_held: false,
                target,
            });

            let additive = match effect {
                InteractionEffect::SelectEdge { additive, .. } => additive,
                InteractionEffect::BeginNodeDrag { additive, .. } => additive,
                other => panic!("{target:?} produced {other:?}"),
            };
            assert!(additive, "{target:?} lost the shift modifier");
        }

        // And the command modifier means the same thing, because
        // `InputModifiers::is_additive` is the one place that decides.
        let mut machine = InteractionMachine::new();
        let effect = machine.handle(InteractionEvent::PointerDown {
            screen: Vec2::splat(100.0),
            world: Vec2::splat(10.0),
            button: PointerButton::Left,
            modifiers: InputModifiers {
                command: true,
                ..InputModifiers::NONE
            },
            pan_key_held: false,
            target: PointerTarget::Edge(EdgeIndex::new(2)),
        });
        assert_eq!(
            effect,
            InteractionEffect::SelectEdge {
                edge: EdgeIndex::new(2),
                additive: true
            }
        );
    }

    /// A creating tool still draws over an edge. §45's rule is that the tool
    /// decides first, and the new arm sits *inside* the `Select` branch — an
    /// easy place to break that by accident.
    #[test]
    fn a_creating_tool_draws_over_an_edge_rather_than_selecting_it() {
        let mut machine = InteractionMachine::new();
        machine.handle(InteractionEvent::SelectTool(CanvasTool::Rectangle));

        let effect = machine.handle(down_on(
            PointerButton::Left,
            PointerTarget::Edge(EdgeIndex::new(1)),
        ));

        assert!(matches!(effect, InteractionEffect::BeginCreate { .. }));
    }

    /// The middle button still pans under every tool. A creating tool that
    /// swallowed the pan would leave a user unable to scroll while drawing.
    #[test]
    fn the_middle_button_still_pans_under_every_tool() {
        for tool in CanvasTool::ALL {
            let mut machine = InteractionMachine::new();
            machine.handle(InteractionEvent::SelectTool(*tool));
            assert_eq!(
                machine.handle(down(PointerButton::Middle)),
                InteractionEffect::BeginPan,
                "{} swallowed the middle button",
                tool.name()
            );
        }
    }
}
