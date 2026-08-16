//! [`ElementKind`] — the element taxonomy of requirements §3.
//!
//! The requirement is one sentence and it is the whole design constraint:
//! *"Design an extensible element taxonomy. Do not hard-code the engine around
//! only rectangular graph nodes."* Two things follow, and both cost something
//! that is worth naming.
//!
//! **Every arm that can grow has a `Custom` escape hatch**, carrying a
//! [`CustomKind`] name rather than a fixed variant. That is how §5's ~60-kind
//! catalogue and §43's renderer registry arrive later without this enum
//! changing: a registered renderer is looked up by name, and an unknown name
//! renders as the generic default rather than failing to load the document.
//! **A document that names a kind this build has never heard of must still
//! open** — that is the whole reason the escape hatch is in the *serialized*
//! type rather than only in the registry.
//!
//! **The variants present are the ones the first increment can draw**, not the
//! catalogue. The plan's decision is a registry plus about six generic kinds,
//! because every user-visible kind name costs an English *and* a Vietnamese
//! string in `dodo-i18n`, and ~120 translations for kinds nobody can yet place
//! on a canvas is not a deliverable. This slice adds no strings at all: nothing
//! here is user-visible yet.
//!
//! **Size matters here.** §41 warns about oversized enums, and this one is
//! stored per element. The `Custom` arms hold a `CustomKind` (one `String`, 24
//! bytes on a 64-bit target), which sets the enum's size; every other variant
//! is a discriminant. A future interning pass would shrink it to 8, and
//! `kind_is_not_oversized` in the tests below is the tripwire that notices if
//! something larger is added by accident.

use serde::{Deserialize, Serialize};

/// What an element *is*. One serialized field per element, matched on by the
/// renderer, the hit-tester and the inspector.
///
/// `#[serde(tag = "type")]`-free on purpose: the default externally-tagged
/// representation writes `{"Shape": {"Rectangle": null}}`-style JSON, which is
/// noisier than a tagged one but round-trips a `Custom(name)` payload without a
/// bespoke visitor. Legibility of the file can be revisited; correctness of
/// the round-trip is this phase's.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ElementKind {
    /// A React-Flow-style graph node: a body with handles, connected by edges.
    GraphNode(GraphNodeKind),
    /// A drawn shape with no graph semantics — §6's flowchart shapes and §7's
    /// Excalidraw element set.
    Shape(ShapeKind),
    /// A free linear element: a line or an arrow that is not an edge between
    /// two nodes. §8 covers both; the distinction is that an *edge* is bound to
    /// endpoints and reroutes when they move, and a `Linear` element does not.
    Linear(LinearKind),
    /// Standalone text (§9).
    Text,
    /// An embedded raster image (§10).
    Image,
    /// A frame: a named region that clips and moves its children (§11).
    Frame,
    /// A group: a selection that behaves as one element, without clipping (§11).
    Group,
    /// A freehand stroke (§12).
    FreeDraw,
    /// An embedded external view (§7's embed).
    Embed,
    /// Anything a later registry defines. See the module doc for why this is in
    /// the serialized type.
    Custom(CustomKind),
}

impl Default for ElementKind {
    fn default() -> ElementKind {
        ElementKind::GraphNode(GraphNodeKind::Default)
    }
}

impl ElementKind {
    /// True for kinds the graph engine routes edges between. The hit-tester and
    /// the connection tool ask this rather than matching every variant, so a
    /// new drawn kind does not have to be added to three `match` arms.
    pub fn is_graph_node(&self) -> bool {
        matches!(self, ElementKind::GraphNode(_))
    }

    /// True for kinds that contain other elements (§11's hierarchy).
    pub fn is_container(&self) -> bool {
        matches!(
            self,
            ElementKind::Frame | ElementKind::Group | ElementKind::GraphNode(GraphNodeKind::Group)
        )
    }

    /// The registered name a [`Custom`](ElementKind::Custom) kind carries, at
    /// any nesting depth — the key §43's renderer registry looks up.
    pub fn custom_name(&self) -> Option<&str> {
        match self {
            ElementKind::Custom(name)
            | ElementKind::GraphNode(GraphNodeKind::Custom(name))
            | ElementKind::Shape(ShapeKind::Custom(name)) => Some(name.as_str()),
            _ => None,
        }
    }
}

/// The generic graph-node kinds, mirroring React Flow's own set (§4).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GraphNodeKind {
    /// Handles on both sides; the kind a plain drag-out creates.
    Default,
    /// A source: outputs only.
    Input,
    /// A sink: inputs only.
    Output,
    /// A container node that holds other nodes and moves them with it.
    Group,
    /// A kind supplied by §43's renderer registry.
    Custom(CustomKind),
}

/// Drawn shapes. The four the first increment paints are the four the canvas
/// foundation builds through `PathBuilder`; the rest of §6's flowchart catalogue arrives
/// as registry content, not as variants here.
///
/// Measured rendering cost is why this is not a flat "polygon or not" split:
/// [`ShapeKind::Rectangle`] must be painted as a **quad**, not a filled path
/// (20,000 quads at 60 fps against 20,000 rect paths at 30), while
/// [`ShapeKind::Ellipse`] costs 337 vertices via two `arc_to` calls — as much
/// as a full-window Bézier — and so degrades to a quad *earlier* than a
/// rectangle does in the LOD ladder. The renderer needs the kind to make that
/// choice, so the kind is what the document stores.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ShapeKind {
    Rectangle,
    RoundedRectangle,
    Ellipse,
    Diamond,
    Triangle,
    Custom(CustomKind),
}

/// Free linear elements (§7, §8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum LinearKind {
    Line,
    Arrow,
    /// A multi-segment connector with orthogonal legs.
    Elbow,
}

/// The name of a kind this build may not know. See [`ElementKind::Custom`].
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CustomKind(String);

impl CustomKind {
    pub fn new(name: impl Into<String>) -> CustomKind {
        CustomKind(name.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::{CustomKind, ElementKind, GraphNodeKind, LinearKind, ShapeKind};

    #[test]
    fn graph_nodes_are_distinguished_from_drawn_elements() {
        assert!(ElementKind::default().is_graph_node());
        assert!(ElementKind::GraphNode(GraphNodeKind::Input).is_graph_node());
        assert!(!ElementKind::Shape(ShapeKind::Diamond).is_graph_node());
        assert!(!ElementKind::Linear(LinearKind::Arrow).is_graph_node());
        assert!(!ElementKind::Text.is_graph_node());
    }

    #[test]
    fn containers_are_the_three_that_hold_children() {
        assert!(ElementKind::Frame.is_container());
        assert!(ElementKind::Group.is_container());
        assert!(ElementKind::GraphNode(GraphNodeKind::Group).is_container());

        assert!(!ElementKind::GraphNode(GraphNodeKind::Default).is_container());
        assert!(!ElementKind::Shape(ShapeKind::Rectangle).is_container());
    }

    #[test]
    fn a_custom_name_is_reachable_at_every_nesting_depth() {
        let cases = [
            ElementKind::Custom(CustomKind::new("dodo.sticky")),
            ElementKind::GraphNode(GraphNodeKind::Custom(CustomKind::new("dodo.sticky"))),
            ElementKind::Shape(ShapeKind::Custom(CustomKind::new("dodo.sticky"))),
        ];

        for kind in cases {
            assert_eq!(kind.custom_name(), Some("dodo.sticky"));
        }

        assert_eq!(ElementKind::Text.custom_name(), None);
        assert_eq!(
            ElementKind::Shape(ShapeKind::Ellipse).custom_name(),
            None,
            "a known kind is not a custom one"
        );
    }

    #[test]
    fn an_unknown_custom_kind_survives_a_round_trip() {
        // The point of the escape hatch: a document written by a build that
        // knows "dodo.mermaid.actor" must still open here, unchanged, and be
        // written back with the name intact.
        let json = r#"{"Custom":"dodo.mermaid.actor"}"#;
        let kind: ElementKind = serde_json::from_str(json).expect("unknown kinds load");

        assert_eq!(kind.custom_name(), Some("dodo.mermaid.actor"));
        assert_eq!(serde_json::to_string(&kind).unwrap(), json);
    }

    #[test]
    fn kind_is_not_oversized() {
        // §41 warns about oversized enums, and this one is stored per element.
        // A `String` is the largest payload any variant may carry, and the
        // nested `Custom` arms pack into its niche; if this fails, something
        // bigger was added inline and should be boxed or interned instead.
        assert!(
            size_of::<ElementKind>() <= size_of::<String>() + size_of::<usize>(),
            "ElementKind is {} bytes, past a String plus a discriminant",
            size_of::<ElementKind>()
        );
    }
}
