//! [`CanvasTool`] — §45's tool system, and **the answer to "what does the next
//! press mean?"**
//!
//! # Why this is a phase of its own, seven phases late
//!
//! §45 was folded into "the interaction state machine" when the plan was
//! written, and the state machine landed in Phase 2 without it. Seven phases
//! later the canvas could pan, zoom, drag, select, connect, simplify, sketch
//! and undo, and **a user still could not create a single element**, because
//! nothing let them say "now I am drawing a rectangle". Nothing failed; a
//! capability was simply absent, which is the failure mode Phase 7's dead key
//! bindings had already demonstrated once.
//!
//! # The rule §45 states, and what it costs to honour
//!
//! > Tool activation drives interaction state and must not alter the document
//! > model.
//!
//! So selecting a tool is a transition in [`InteractionMachine`](super::InteractionMachine)
//! and nothing else: no draft element is inserted on activation, no placeholder
//! is written, and [`FlowEditor`](crate::commands::FlowEditor) does not hear
//! about it at all. The document changes exactly once per creation, on the
//! release, through §30's one applier — which is what makes a created element
//! undoable without a single line in `commands/` knowing that a tool exists.
//!
//! # Which tools are here, and why the obvious three are not
//!
//! Every variant below creates something the engine can *draw*. [`Frame`] and
//! freehand are deliberately absent: their [`ElementKind`]s exist and their
//! painters do not, so [`NodeShape::of`](crate::runtime::NodeShape::of) maps
//! them to `NodeShape::Other` and
//! [`RenderSnapshot`](crate::render::RenderSnapshot) counts them as
//! `unsupported_nodes` rather than drawing them. A palette button for one would
//! create an element the canvas then refuses to paint — **a control that
//! appears to work and produces nothing**, which is strictly worse than an
//! absent one and much harder to notice.
//!
//! [`Text`] was one of the four until Phase 10, and it left the list the only
//! way anything may: [`NodeShape::Text`](crate::runtime::NodeShape::Text) and a
//! painter arrived first, and the tool followed.
//!
//! **[`Image`] is the one that has a painter and still has no tool**, and that
//! is a decision rather than an omission. A tool answers "what does the next
//! press mean?"; inserting a picture answers nothing about the next press. There
//! is no rectangle to drag out — the size comes from the file's own dimensions,
//! and letting a drag choose it would mean the first thing a user does to every
//! photograph is squash it. So it is an *action* beside the tools, like Delete:
//! it opens a file picker and drops the element in the middle of the view. See
//! [`views::palette`](crate::views::palette).
//!
//! [`Text`]: crate::models::ElementKind::Text
//! [`Frame`]: crate::models::ElementKind::Frame
//! [`Image`]: crate::models::ElementKind::Image
//!
//! Adding one later is a variant here, a row in
//! [`commands::keys`](crate::commands::keys), and whatever the painter needs —
//! the enum is `#[non_exhaustive]`-in-spirit by being matched exhaustively in
//! exactly two pure functions ([`CanvasTool::element_kind`] and
//! [`CanvasTool::default_size`]), so a new variant is a compile error at both.
//!
//! # A click is a creation too
//!
//! Every creating tool answers a click as well as a drag: below
//! [`MIN_DRAG_PIXELS`] of travel the new element takes
//! [`CanvasTool::default_size`], **centred on the press**. That is one rule
//! rather than a special case for graph nodes, and it is what stops a twitch
//! during a rectangle drag from producing a two-pixel sliver the user then has
//! to find and delete.
//!
//! **This file names no UI framework.**

use crate::{
    geometry::{Rect, Vec2},
    models::{EdgeIndex, ElementKind, GraphNodeKind, LinearKind, NodeIndex, ShapeKind},
};

/// How far the pointer must travel, **in screen pixels**, before a press is a
/// drag rather than a click.
///
/// Screen rather than world on purpose: the question is "did the user's hand
/// move?", and a world-space threshold would answer it differently at every
/// zoom — a four-unit twitch is a whole node at 20× and invisible at 0.05×.
pub const MIN_DRAG_PIXELS: f32 = 4.0;

/// **What the next press means** (§45).
///
/// One tool at a time, held by the interaction machine. Two of the variants
/// change how an existing element is handled; the rest create a new one.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum CanvasTool {
    /// The resting tool: press to drag a node, drag a handle to connect, drag
    /// empty canvas to rubber-band. Everything Phases 2 to 7 built.
    #[default]
    Select,
    /// Pan with the left button, so a canvas can be navigated on a machine with
    /// one button and no space bar. The same gesture space-drag already gives.
    Hand,

    Rectangle,
    Diamond,
    Ellipse,
    /// A free linear element with a head — §8's `Linear`, not an edge. It is
    /// not bound to two nodes and does not reroute when anything moves.
    Arrow,
    /// A free linear element with no head.
    Line,
    /// A React-Flow-style node, born with a source and a target handle so it
    /// can be connected the moment it exists.
    GraphNode,
    /// §9's standalone text.
    ///
    /// **The one creating tool whose release does not add an element.** Every
    /// other tool commits on the mouse-up; this one opens a text editor over
    /// the rectangle it drew and the element is added when — and only when —
    /// non-empty text is committed. See
    /// [`InteractionEffect::BeginTextEdit`](super::InteractionEffect::BeginTextEdit).
    ///
    /// That is not a special case bolted on: an empty text element is
    /// *invisible*, because a text element is its glyphs. Creating one on
    /// release would mean a click with the Text tool leaving a selectable,
    /// undoable, unpaintable thing on the canvas — precisely the failure
    /// Phase 7.5 caught for Line and Arrow, arriving from the other direction.
    Text,
}

impl CanvasTool {
    /// Every tool, in palette order: the two that do not create, then the
    /// shapes, then the linear elements, then the graph node.
    ///
    /// The order is the palette's and the keyboard's both, so a row cannot be
    /// added to one and forgotten in the other.
    pub const ALL: &'static [CanvasTool] = &[
        CanvasTool::Select,
        CanvasTool::Hand,
        CanvasTool::Rectangle,
        CanvasTool::Diamond,
        CanvasTool::Ellipse,
        CanvasTool::Arrow,
        CanvasTool::Line,
        CanvasTool::GraphNode,
        CanvasTool::Text,
    ];

    /// A short stable name, for a test, a trace line or a widget id. **Not
    /// user-facing, and it must stay that way** — an element id has to survive
    /// a language change, which is exactly why `dodo-i18n-text`'s rule exempts
    /// ids and nothing else here. The tool's *label* is
    /// `dodo_i18n::flow::Text`, mapped in `views::palette` because this file
    /// sits below the UI-framework line and a catalogue is a view's business.
    pub fn name(self) -> &'static str {
        match self {
            CanvasTool::Select => "select",
            CanvasTool::Hand => "hand",
            CanvasTool::Rectangle => "rectangle",
            CanvasTool::Diamond => "diamond",
            CanvasTool::Ellipse => "ellipse",
            CanvasTool::Arrow => "arrow",
            CanvasTool::Line => "line",
            CanvasTool::GraphNode => "graph-node",
            CanvasTool::Text => "text",
        }
    }

    /// **What this tool creates**, or `None` for the two that create nothing.
    ///
    /// The one place a tool becomes a document kind, so a new tool is routed in
    /// exactly one `match` — the same discipline
    /// [`NodeShape::of`](crate::runtime::NodeShape::of) holds one layer down.
    pub fn element_kind(self) -> Option<ElementKind> {
        Some(match self {
            CanvasTool::Select | CanvasTool::Hand => return None,
            CanvasTool::Rectangle => ElementKind::Shape(ShapeKind::Rectangle),
            CanvasTool::Diamond => ElementKind::Shape(ShapeKind::Diamond),
            CanvasTool::Ellipse => ElementKind::Shape(ShapeKind::Ellipse),
            CanvasTool::Arrow => ElementKind::Linear(LinearKind::Arrow),
            CanvasTool::Line => ElementKind::Linear(LinearKind::Line),
            CanvasTool::GraphNode => ElementKind::GraphNode(GraphNodeKind::Default),
            CanvasTool::Text => ElementKind::Text,
        })
    }

    /// **Whether finishing this tool's gesture opens a text editor instead of
    /// adding an element.**
    ///
    /// A named question rather than `== CanvasTool::Text` in the transition
    /// function, because it is a property of the tool and the state machine
    /// should not be the place that knows which tools are textual.
    pub fn edits_text_on_release(self) -> bool {
        matches!(self, CanvasTool::Text)
    }

    /// Whether a press with this tool starts a creation rather than a
    /// selection, a drag or a pan.
    pub fn creates(self) -> bool {
        self.element_kind().is_some()
    }

    /// The size a **click** with this tool produces, in world units.
    ///
    /// Chosen to match what the engine's own demo document uses for each kind,
    /// so a clicked element and a hand-placed one look like the same thing.
    pub fn default_size(self) -> Vec2 {
        match self {
            // Never read — a non-creating tool has no element — but answered
            // rather than panicking, because a total function is one fewer
            // thing for a caller to get wrong.
            CanvasTool::Select | CanvasTool::Hand => Vec2::ZERO,
            CanvasTool::Rectangle | CanvasTool::Diamond | CanvasTool::Ellipse => {
                Vec2::new(120.0, 80.0)
            }
            CanvasTool::Arrow | CanvasTool::Line => Vec2::new(160.0, 0.0),
            CanvasTool::GraphNode => Vec2::new(160.0, 80.0),
            // Wide and one line high: text is read across, and a click that
            // placed a tall box would leave the caret floating in the middle
            // of empty space. The height is a `Medium` line plus its leading —
            // see `FontSize::Medium.world_size()`.
            CanvasTool::Text => Vec2::new(200.0, 22.0),
        }
    }

    /// Whether holding shift while dragging constrains this tool to a square
    /// bounding box — a square, a circle, a regular diamond, a 45° line.
    ///
    /// True for every creating tool **but text**. The constraint is a property
    /// of the bounding box, and for every drawn shape the bounding box *is* the
    /// shape; for text it is the column the words are laid into, and squaring
    /// it means dragging out a wide caption gives a tall narrow one instead.
    pub fn honours_square_constraint(self) -> bool {
        self.creates() && !self.edits_text_on_release()
    }
}

/// **What a text edit is about to change** (§9).
///
/// Three arms, because §9 asks for three things and the third is not a variant
/// of the first two: a node's text, an edge's label, and text that **does not
/// exist yet**.
///
/// [`New`](TextTarget::New) is what makes the Text tool honest. A tool that
/// created its element on release and then opened an editor would leave an
/// empty — and therefore invisible — element behind whenever the user changed
/// their mind, plus a second undo step to remove it. Carrying the *rectangle*
/// instead means the document hears about the text exactly once, when there is
/// text, which is the same rule §45 states for every other tool.
///
/// `Copy`, so it can live in [`InteractionState`](super::InteractionState)
/// beside the other gestures: a [`Rect`] is four floats and the runtime indices
/// are `u32`s. Nothing here carries a `String` — the text being typed belongs
/// to whatever widget is collecting it, and this file is below the UI-framework
/// line.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TextTarget {
    /// A node's own text — its label, laid out inside its body.
    Node(NodeIndex),
    /// An edge's label, positioned along its route.
    Edge(EdgeIndex),
    /// A standalone text element that has not been created yet, and the world
    /// rectangle it will occupy.
    New(Rect),
}

impl TextTarget {
    /// The world rectangle the editor should be drawn over.
    ///
    /// `None` for an existing element, because *this* type does not know where
    /// anything is — the world does, and the caller has it. Returning the
    /// rectangle only for the arm that carries one is what stops a caller
    /// silently placing an editor at the origin.
    pub fn pending_rect(self) -> Option<Rect> {
        match self {
            TextTarget::New(rect) => Some(rect),
            _ => None,
        }
    }

    /// A short stable name, for a test or an element id. **Not user-facing.**
    pub fn name(self) -> &'static str {
        match self {
            TextTarget::Node(_) => "node",
            TextTarget::Edge(_) => "edge",
            TextTarget::New(_) => "new",
        }
    }
}

/// **The rectangle a creation gesture produces**, from where it started, where
/// it is now, and whether the constraint was held.
///
/// Pure, total, and the only place the three rules meet:
///
/// 1. A drag below [`MIN_DRAG_PIXELS`] of *screen* travel is a click, and a
///    click places [`CanvasTool::default_size`] **centred on the press**. A
///    linear tool's default height is zero, so a clicked arrow is a horizontal
///    one rather than a dot.
/// 2. With the constraint held, the bounding box is squared to the longer of
///    its two sides, keeping the direction the pointer actually went — so the
///    shape grows towards the cursor instead of flipping across the anchor.
/// 3. Otherwise it is [`Rect::from_corners`], which normalises, so dragging
///    up-left and down-right give the same rectangle.
///
/// **Rule 3 has a consequence worth knowing about for the linear tools**: the
/// document stores a position and a size, never a pair of endpoints, so a
/// linear element's direction is its bounding box's diagonal and an arrow
/// always points from the box's top-left to its bottom-right. Dragging one out
/// leftwards produces an arrow pointing right. The preview is drawn from the
/// same rectangle by the same outline builder, so what is committed is exactly
/// what was shown — but a genuinely free arrow needs §7's point list, which is
/// a model change rather than a painter one.
pub fn creation_rect(tool: CanvasTool, gesture: CreationGesture) -> Rect {
    if gesture.screen_travel() < MIN_DRAG_PIXELS {
        let size = tool.default_size();
        return Rect::new(gesture.anchor_world - size * 0.5, size);
    }

    let delta = gesture.current_world - gesture.anchor_world;
    if gesture.constrain && tool.honours_square_constraint() {
        let side = delta.x.abs().max(delta.y.abs());
        let signed = Vec2::new(side.copysign(delta.x), side.copysign(delta.y));
        return Rect::from_corners(gesture.anchor_world, gesture.anchor_world + signed);
    }

    Rect::from_corners(gesture.anchor_world, gesture.current_world)
}

/// A creation drag in progress: both spaces, because the two questions need
/// different ones.
///
/// The world positions are the geometry — anchored to the document, so zooming
/// or scrolling mid-drag leaves the shape over the same content, exactly as
/// [`InteractionState::BoxSelecting`](super::InteractionState::BoxSelecting)
/// does. The screen positions answer only "has the hand moved?", which is a
/// question about the pointer and not about the document.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CreationGesture {
    pub anchor_world: Vec2,
    pub current_world: Vec2,
    pub anchor_screen: Vec2,
    pub current_screen: Vec2,
    /// Captured at press time rather than read at release, for the same reason
    /// [`InteractionState::BoxSelecting`](super::InteractionState::BoxSelecting)
    /// captures shift: a user who lets go before the mouse button still meant
    /// it.
    ///
    /// It also cannot be read later: [`InteractionEvent::PointerMove`](super::InteractionEvent::PointerMove)
    /// carries no modifiers, and adding them for one gesture would change the
    /// whole event vocabulary. The cost is that the constraint cannot be
    /// toggled mid-drag the way Excalidraw allows.
    pub constrain: bool,
}

impl CreationGesture {
    pub fn screen_travel(&self) -> f32 {
        (self.current_screen - self.anchor_screen).length()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::ElementKind;

    fn drag(from: Vec2, to: Vec2, constrain: bool) -> CreationGesture {
        CreationGesture {
            anchor_world: from,
            current_world: to,
            anchor_screen: from,
            current_screen: to,
            constrain,
        }
    }

    /// The palette and the enum must not drift: every tool is offered, once.
    #[test]
    fn every_tool_appears_exactly_once_in_the_palette_order() {
        for tool in CanvasTool::ALL {
            assert_eq!(
                CanvasTool::ALL.iter().filter(|it| *it == tool).count(),
                1,
                "{} appears more than once",
                tool.name()
            );
        }
        assert_eq!(CanvasTool::ALL.len(), 9);
    }

    /// **§45's rule, at the level this file can assert it**: the two navigating
    /// tools name no element, so there is nothing for a creation to insert.
    #[test]
    fn only_the_creating_tools_name_an_element() {
        assert_eq!(CanvasTool::Select.element_kind(), None);
        assert_eq!(CanvasTool::Hand.element_kind(), None);
        assert!(!CanvasTool::Select.creates());
        assert!(!CanvasTool::Hand.creates());

        for tool in CanvasTool::ALL.iter().filter(|tool| tool.creates()) {
            assert!(
                tool.element_kind().is_some(),
                "{} creates nothing",
                tool.name()
            );
        }
    }

    /// **The trap this phase's tool list was chosen around.** Every kind a tool
    /// can create must be one the renderer can draw — a tool producing a
    /// `NodeShape::Other` element would be a button that silently makes an
    /// invisible thing.
    #[test]
    fn every_tool_creates_something_the_renderer_can_draw() {
        use crate::runtime::NodeShape;

        for tool in CanvasTool::ALL {
            let Some(kind) = tool.element_kind() else {
                continue;
            };
            assert_ne!(
                NodeShape::of(&kind),
                NodeShape::Other,
                "the {} tool creates {kind:?}, which the canvas does not paint",
                tool.name()
            );
        }
    }

    /// The kinds deliberately left out, stated as a test so that adding a tool
    /// for one without adding its painter fails here rather than on screen.
    ///
    /// **[`ElementKind::Image`] left this list in Phase 12 and did not join
    /// [`CanvasTool`]**, which is the interesting case: it has a painter now,
    /// and it still has no tool, because inserting a picture is not a drawing
    /// gesture — there is no rectangle to drag out, the size comes from the
    /// file, and §45's rule is that a tool changes what the *next press*
    /// means. It is an action beside the tools instead. See
    /// [`views::palette`](crate::views::palette).
    #[test]
    fn the_kinds_without_painters_have_no_tool() {
        use crate::runtime::NodeShape;

        for kind in [ElementKind::Frame, ElementKind::FreeDraw] {
            assert_eq!(NodeShape::of(&kind), NodeShape::Other);
            assert!(
                !CanvasTool::ALL
                    .iter()
                    .any(|tool| tool.element_kind() == Some(kind.clone())),
                "{kind:?} has a tool but no painter"
            );
        }

        assert!(
            !CanvasTool::ALL
                .iter()
                .any(|tool| tool.element_kind() == Some(ElementKind::Image)),
            "an image is inserted by an action, not drawn by a tool"
        );
    }

    #[test]
    fn a_drag_becomes_the_rectangle_between_its_two_corners() {
        let rect = creation_rect(
            CanvasTool::Rectangle,
            drag(Vec2::new(10.0, 20.0), Vec2::new(110.0, 70.0), false),
        );
        assert_eq!(rect.min(), Vec2::new(10.0, 20.0));
        assert_eq!(rect.size, Vec2::new(100.0, 50.0));
    }

    /// Dragging up-left must give the same rectangle as dragging down-right;
    /// the document has no room for a negative size.
    #[test]
    fn a_backwards_drag_normalises() {
        let forwards = creation_rect(
            CanvasTool::Ellipse,
            drag(Vec2::new(0.0, 0.0), Vec2::new(100.0, 60.0), false),
        );
        let backwards = creation_rect(
            CanvasTool::Ellipse,
            drag(Vec2::new(100.0, 60.0), Vec2::new(0.0, 0.0), false),
        );
        assert_eq!(forwards, backwards);
    }

    /// Shift squares the bounding box **towards the cursor**: a shape that
    /// flipped across the anchor when the constraint was applied would jump out
    /// from under the hand.
    #[test]
    fn the_constraint_squares_towards_the_pointer() {
        let up_left = creation_rect(
            CanvasTool::Rectangle,
            drag(Vec2::new(0.0, 0.0), Vec2::new(-100.0, -20.0), true),
        );
        assert_eq!(up_left.size, Vec2::new(100.0, 100.0));
        assert_eq!(up_left.min(), Vec2::new(-100.0, -100.0));

        let down_right = creation_rect(
            CanvasTool::Rectangle,
            drag(Vec2::new(0.0, 0.0), Vec2::new(100.0, 20.0), true),
        );
        assert_eq!(down_right.size, Vec2::new(100.0, 100.0));
        assert_eq!(down_right.min(), Vec2::ZERO);
    }

    /// The constraint is ignored by a tool that does not honour it, rather than
    /// being an argument the caller has to remember not to pass.
    #[test]
    fn a_non_creating_tool_ignores_the_constraint() {
        assert!(!CanvasTool::Select.honours_square_constraint());
        assert!(!CanvasTool::Hand.honours_square_constraint());
    }

    /// **A text box is a column, not a shape**, so squaring it is wrong in a
    /// way squaring a rectangle is not — shift-dragging a wide caption would
    /// give a tall narrow one.
    #[test]
    fn the_text_tool_is_the_one_creating_tool_the_constraint_does_not_square() {
        assert!(CanvasTool::Text.creates());
        assert!(!CanvasTool::Text.honours_square_constraint());

        let wide = creation_rect(
            CanvasTool::Text,
            drag(Vec2::ZERO, Vec2::new(300.0, 20.0), true),
        );
        assert_eq!(wide.size, Vec2::new(300.0, 20.0));

        for tool in CanvasTool::ALL
            .iter()
            .filter(|tool| tool.creates() && **tool != CanvasTool::Text)
        {
            assert!(tool.honours_square_constraint(), "{}", tool.name());
        }
    }

    /// **Exactly one tool defers its element to a text commit**, and the
    /// property that matters is the negative half: every other creating tool
    /// still adds on release, so nothing else silently stopped drawing.
    #[test]
    fn only_the_text_tool_opens_an_editor_instead_of_committing() {
        assert!(CanvasTool::Text.edits_text_on_release());
        for tool in CanvasTool::ALL
            .iter()
            .filter(|tool| **tool != CanvasTool::Text)
        {
            assert!(
                !tool.edits_text_on_release(),
                "{} would stop creating anything",
                tool.name()
            );
        }
    }

    /// A pending text element is the only target that knows where it goes; the
    /// other two are looked up in the world. Returning a rectangle for all
    /// three would let a caller place an editor at the origin and never notice.
    #[test]
    fn only_a_pending_text_target_carries_its_own_rectangle() {
        let rect = Rect::new(Vec2::new(10.0, 20.0), Vec2::new(200.0, 22.0));

        assert_eq!(TextTarget::New(rect).pending_rect(), Some(rect));
        assert_eq!(TextTarget::Node(NodeIndex::new(0)).pending_rect(), None);
        assert_eq!(TextTarget::Edge(EdgeIndex::new(0)).pending_rect(), None);
    }

    /// **A click places a default-size element centred on the press**, for
    /// every creating tool and not only the graph node.
    #[test]
    fn a_click_places_the_default_size_centred_on_the_press() {
        for tool in CanvasTool::ALL.iter().filter(|tool| tool.creates()) {
            let at = Vec2::new(300.0, -50.0);
            let rect = creation_rect(*tool, drag(at, at, false));
            assert_eq!(rect.size, tool.default_size(), "{}", tool.name());
            assert_eq!(rect.center(), at, "{}", tool.name());
        }
    }

    /// A twitch is still a click. The threshold is screen travel, so this is
    /// the case that stops a two-pixel sliver being created and then having to
    /// be hunted down.
    #[test]
    fn a_twitch_below_the_threshold_is_still_a_click() {
        let gesture = CreationGesture {
            anchor_world: Vec2::ZERO,
            current_world: Vec2::new(2.0, 1.0),
            anchor_screen: Vec2::ZERO,
            current_screen: Vec2::new(2.0, 1.0),
            constrain: false,
        };
        assert!(gesture.screen_travel() < MIN_DRAG_PIXELS);
        assert_eq!(
            creation_rect(CanvasTool::Rectangle, gesture).size,
            CanvasTool::Rectangle.default_size()
        );
    }

    /// **The threshold is screen travel, not world travel**, and this is the
    /// case that shows the difference: zoomed far out, a hand that has not
    /// moved covers a large world distance.
    #[test]
    fn the_click_threshold_reads_screen_travel_not_world_travel() {
        let gesture = CreationGesture {
            anchor_world: Vec2::ZERO,
            // 2000 world units, at a zoom of 0.001 — one screen pixel.
            current_world: Vec2::new(2000.0, 0.0),
            anchor_screen: Vec2::ZERO,
            current_screen: Vec2::new(2.0, 0.0),
            constrain: false,
        };
        assert_eq!(
            creation_rect(CanvasTool::Rectangle, gesture).size,
            CanvasTool::Rectangle.default_size(),
            "a hand that did not move must not draw a 2000-unit shape"
        );
    }
}
