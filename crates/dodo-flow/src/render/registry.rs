//! §43's node renderer registry — **the extension point, and the six kinds
//! that prove it works.**
//!
//! # What §43 asks for, and the one place this deviates
//!
//! §43 sketches the trait as returning an `AnyElement`. That signature cannot
//! live here: `AnyElement` is GPUI's, and everything below `views/` names no UI
//! framework — the crate doc explains what that boundary is worth and why it is
//! very hard to recover once lost. A registry that returned elements would drag
//! GPUI into the one module a renderer author most wants to unit test.
//!
//! So the registry answers with a **[`NodeVisual`]**: a `Copy` description of
//! what the node looks like, in the vocabulary of roles and glyphs rather than
//! of colours and pixels. [`crate::views`] turns one into an element. The
//! extension point is unchanged — a caller registers a
//! [`NodeRenderer`] against a kind name and gets to decide the node's shape,
//! accent, glyph, badge and which text lines it shows — and the win is that a
//! renderer can be asserted in an ordinary test. If a future kind genuinely
//! needs an arbitrary element tree, the escape hatch belongs in `views/`, on
//! top of this, rather than replacing it.
//!
//! # Why a `Copy` descriptor and not a trait object per node
//!
//! §43 is explicit: *"Avoid trait-object overhead in hot geometry loops if an
//! enum/static dispatch is better. The registry is mainly for high-level rich
//! node customization."* Two things keep that true here:
//!
//! - **The built-in kinds never touch the map.** [`NodeRendererRegistry::visual`]
//!   matches [`ElementKind`] first and only reaches the `HashMap` for a
//!   [`CustomKind`] — so a 100,000-node document of ordinary graph nodes does
//!   no lookups at all.
//! - **It is called on the rich set, not on the visible set.** The rich set is
//!   bounded by [`RenderBudgets::max_rich_elements`](crate::budgets::RenderBudgets::max_rich_elements)
//!   and is empty below full zoom (§15). The vector-shape loop in
//!   [`crate::render::scene`] reads the hot [`NodeShape`] array and never comes
//!   here — which is §43's "rich graph nodes and vector canvas shapes are
//!   different rendering strategies sharing the same infrastructure", made
//!   structural.
//!
//! # Six kinds, deliberately, not sixty
//!
//! §5 lists roughly sixty node kinds across seven categories. **They are not
//! this phase's**, and the plan says why: every user-visible kind name costs an
//! English *and* a Vietnamese string in `dodo-i18n`, so the catalogue is ~120
//! translations for kinds nobody can place on a canvas yet. The registry is the
//! deliverable; the catalogue is later content, and it arrives by calling
//! [`NodeRendererRegistry::register`] rather than by editing this file.
//!
//! The six here are [`GenericKind`]'s, and three of them —
//! `Process`, `Decision`, `Note` — are registered through the **same public
//! path a third party would use**, keyed by a stable identifier rather than
//! added as enum variants. That is on purpose: a registry whose own contents
//! take a private shortcut is a registry nobody has tested.
//!
//! Kind ids are identifiers, never display text: `"dodo.flow.decision"` is not
//! a label and is not translated. Nothing in this module is user-visible, which
//! is why this phase adds no strings.
//!
//! **This file names no UI framework.**

use std::{collections::HashMap, sync::Arc};

use crate::{
    geometry::Vec2,
    models::{ElementKind, GraphNodeKind, NodeIndex, ShapeKind},
    runtime::NodeShape,
};

/// The theme role a node's accent is drawn in.
///
/// A role rather than a colour, for the same reason
/// [`ElementStyle`](crate::models::ElementStyle) stores `Option<Color>`: a
/// document — or a registered renderer — that baked in a palette would look
/// wrong in the other theme. [`crate::views`] resolves these against the active
/// theme once per frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum AccentRole {
    #[default]
    Neutral,
    Info,
    Success,
    Warning,
    Danger,
}

/// The mark a rich node carries in its header.
///
/// An enum rather than a glyph string or an asset path, so this module stays
/// free of both text and files. `views/` decides what each one is actually
/// drawn as, and a build with no icon assets still renders every node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum NodeGlyph {
    #[default]
    None,
    /// A generic node.
    Dot,
    /// A source: data enters here.
    Inbound,
    /// A sink: data leaves here.
    Outbound,
    /// A step that transforms.
    Process,
    /// A branch.
    Decision,
    /// An annotation with no graph semantics.
    Note,
}

/// **What a node looks like**, in the vocabulary this side of the boundary can
/// speak. The registry's answer, and the rich element's input.
///
/// `Copy` and small: it is produced per rich node per frame, and a `String`
/// here would be a per-frame allocation on the hot path of the one loop §16
/// cares most about.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NodeVisual {
    /// The body the canvas paints under the element. **The registry may
    /// override the shape**, which is how a registered kind gets a diamond
    /// without a new [`ElementKind`] variant.
    pub body: NodeShape,
    pub accent: AccentRole,
    pub glyph: NodeGlyph,
    /// Whether the node's own label is drawn. A registered kind may suppress
    /// it — a pure-icon node is a legitimate design.
    pub shows_label: bool,
    /// Whether the node draws a coloured accent bar along its leading edge.
    pub shows_accent_bar: bool,
    /// Whether the node's body is a filled surface or an outline only. A note
    /// is a surface; a group is an outline.
    pub filled: bool,
}

impl NodeVisual {
    /// The fallback: a plain graph-node body with no decoration. What an
    /// unregistered kind renders as, so **a document naming a kind this build
    /// has never heard of still opens and still draws** — the same promise
    /// [`ElementKind::Custom`] makes in the serialized format.
    pub const FALLBACK: NodeVisual = NodeVisual {
        body: NodeShape::GraphNode,
        accent: AccentRole::Neutral,
        glyph: NodeGlyph::Dot,
        shows_label: true,
        shows_accent_bar: false,
        filled: true,
    };

    pub const fn with_accent(mut self, accent: AccentRole) -> NodeVisual {
        self.accent = accent;
        self
    }

    pub const fn with_glyph(mut self, glyph: NodeGlyph) -> NodeVisual {
        self.glyph = glyph;
        self
    }

    pub const fn with_body(mut self, body: NodeShape) -> NodeVisual {
        self.body = body;
        self
    }

    pub const fn with_accent_bar(mut self) -> NodeVisual {
        self.shows_accent_bar = true;
        self
    }
}

/// What a renderer is told about the node it is describing.
///
/// Borrowed, never cloned (§40 rule 10, and §24's "compact IDs/references
/// rather than cloning all node metadata"). A renderer that wants to keep
/// something has to copy it deliberately.
#[derive(Debug, Clone, Copy)]
pub struct NodeRef<'a> {
    pub index: NodeIndex,
    pub kind: &'a ElementKind,
    pub label: Option<&'a str>,
    /// World-space size, so a renderer can decide a small node shows less.
    pub size: Vec2,
    pub handle_count: u32,
    pub selected: bool,
}

/// **§43's extension point.**
///
/// Implemented by whatever wants to describe a kind of node — a later phase's
/// node catalogue, a plugin, a test. `Send + Sync` because §24 names this
/// boundary as the one that later allows background computation, and a registry
/// that cannot cross a thread would close that door quietly.
pub trait NodeRenderer: Send + Sync {
    fn visual(&self, node: NodeRef<'_>) -> NodeVisual;
}

/// A renderer that answers the same [`NodeVisual`] for every node of its kind.
///
/// Most kinds are this, and it exists so registering one is a value rather than
/// an `impl` block. A renderer that varies with the node — a badge counting
/// handles, an accent that follows a status field — implements the trait
/// directly.
#[derive(Debug, Clone, Copy)]
pub struct StaticRenderer(pub NodeVisual);

impl NodeRenderer for StaticRenderer {
    fn visual(&self, _node: NodeRef<'_>) -> NodeVisual {
        self.0
    }
}

/// The six generic kinds this phase ships. See the module doc for why six and
/// not sixty.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GenericKind {
    Generic,
    Input,
    Output,
    Process,
    Decision,
    Note,
}

impl GenericKind {
    pub const ALL: [GenericKind; 6] = [
        GenericKind::Generic,
        GenericKind::Input,
        GenericKind::Output,
        GenericKind::Process,
        GenericKind::Decision,
        GenericKind::Note,
    ];

    /// The stable identifier a document stores and the registry keys on.
    ///
    /// **Not a display name.** It is never shown, never translated, and never
    /// changes once a document has been written with it — which is exactly what
    /// distinguishes a kind id from a label.
    pub const fn id(self) -> &'static str {
        match self {
            GenericKind::Generic => "dodo.flow.generic",
            GenericKind::Input => "dodo.flow.input",
            GenericKind::Output => "dodo.flow.output",
            GenericKind::Process => "dodo.flow.process",
            GenericKind::Decision => "dodo.flow.decision",
            GenericKind::Note => "dodo.flow.note",
        }
    }

    /// The [`ElementKind`] a document stores for this kind.
    ///
    /// Three of the six map onto existing [`GraphNodeKind`] variants, because
    /// they were already there; the other three are `Custom`, through the
    /// escape hatch that exists for precisely this. Both paths reach the same
    /// registry.
    pub fn element_kind(self) -> ElementKind {
        match self {
            GenericKind::Generic => ElementKind::GraphNode(GraphNodeKind::Default),
            GenericKind::Input => ElementKind::GraphNode(GraphNodeKind::Input),
            GenericKind::Output => ElementKind::GraphNode(GraphNodeKind::Output),
            other => ElementKind::GraphNode(GraphNodeKind::Custom(crate::models::CustomKind::new(
                other.id(),
            ))),
        }
    }

    pub const fn visual(self) -> NodeVisual {
        match self {
            GenericKind::Generic => NodeVisual::FALLBACK,
            GenericKind::Input => NodeVisual::FALLBACK
                .with_accent(AccentRole::Success)
                .with_glyph(NodeGlyph::Inbound)
                .with_accent_bar(),
            GenericKind::Output => NodeVisual::FALLBACK
                .with_accent(AccentRole::Info)
                .with_glyph(NodeGlyph::Outbound)
                .with_accent_bar(),
            GenericKind::Process => NodeVisual::FALLBACK
                .with_accent(AccentRole::Neutral)
                .with_glyph(NodeGlyph::Process)
                .with_accent_bar(),
            // The one that needs a body the enum cannot give it — and the
            // reason `NodeVisual` carries a shape at all.
            GenericKind::Decision => NodeVisual::FALLBACK
                .with_accent(AccentRole::Warning)
                .with_glyph(NodeGlyph::Decision)
                .with_body(NodeShape::Diamond),
            GenericKind::Note => NodeVisual::FALLBACK
                .with_accent(AccentRole::Warning)
                .with_glyph(NodeGlyph::Note)
                .with_body(NodeShape::RoundedRectangle),
        }
    }
}

/// **The registry** (§43): kind id → renderer.
///
/// Cloneable, because a snapshot extractor holds one and a view holds one and
/// neither should own the other. The `Arc`s make that cheap.
#[derive(Clone, Default)]
pub struct NodeRendererRegistry {
    custom: HashMap<String, Arc<dyn NodeRenderer>>,
}

impl std::fmt::Debug for NodeRendererRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `dyn NodeRenderer` is not `Debug` and should not have to be — a
        // registered renderer is somebody else's type. The keys are the useful
        // part anyway.
        let mut names: Vec<&str> = self.custom.keys().map(String::as_str).collect();
        names.sort_unstable();
        f.debug_struct("NodeRendererRegistry")
            .field("registered", &names)
            .finish()
    }
}

impl NodeRendererRegistry {
    /// An empty registry. Every kind falls back to [`NodeVisual::FALLBACK`].
    pub fn new() -> NodeRendererRegistry {
        NodeRendererRegistry::default()
    }

    /// The registry the canvas starts with: the six generic kinds, registered
    /// through [`register`](NodeRendererRegistry::register) like anything else.
    pub fn with_generic_kinds() -> NodeRendererRegistry {
        let mut registry = NodeRendererRegistry::new();
        for kind in GenericKind::ALL {
            registry.register(kind.id(), Arc::new(StaticRenderer(kind.visual())));
        }
        registry
    }

    /// Registers a renderer for a kind id, replacing any previous one.
    ///
    /// Replacing rather than refusing: a later-loaded plugin overriding an
    /// earlier one is the behaviour a registry is expected to have, and a
    /// silent refusal is the harder bug.
    pub fn register(&mut self, id: impl Into<String>, renderer: Arc<dyn NodeRenderer>) {
        self.custom.insert(id.into(), renderer);
    }

    pub fn is_registered(&self, id: &str) -> bool {
        self.custom.contains_key(id)
    }

    pub fn len(&self) -> usize {
        self.custom.len()
    }

    pub fn is_empty(&self) -> bool {
        self.custom.is_empty()
    }

    /// **Describes one node.**
    ///
    /// The built-in kinds are matched, not looked up — see the module doc.
    /// Only a [`CustomKind`](crate::models::CustomKind) reaches the map, and an
    /// unregistered one falls back rather than failing, because a document must
    /// open on a build that has never heard of its kinds.
    pub fn visual(&self, node: NodeRef<'_>) -> NodeVisual {
        if let Some(name) = node.kind.custom_name() {
            return match self.custom.get(name) {
                Some(renderer) => renderer.visual(node),
                None => NodeVisual::FALLBACK,
            };
        }

        match node.kind {
            ElementKind::GraphNode(GraphNodeKind::Default) => GenericKind::Generic.visual(),
            ElementKind::GraphNode(GraphNodeKind::Input) => GenericKind::Input.visual(),
            ElementKind::GraphNode(GraphNodeKind::Output) => GenericKind::Output.visual(),
            // A group is an outline that holds other nodes: a filled surface
            // would hide them.
            ElementKind::GraphNode(GraphNodeKind::Group) => NodeVisual {
                filled: false,
                ..NodeVisual::FALLBACK
            },
            // A drawn shape is not a rich node. It has a body and nothing else,
            // and the vector loop is what paints it — this arm exists so the
            // shape override is still answerable for one.
            ElementKind::Shape(shape) => NodeVisual {
                body: shape_body(shape),
                glyph: NodeGlyph::None,
                shows_label: false,
                ..NodeVisual::FALLBACK
            },
            _ => NodeVisual::FALLBACK,
        }
    }
}

fn shape_body(shape: &ShapeKind) -> NodeShape {
    match shape {
        ShapeKind::Rectangle => NodeShape::Rectangle,
        ShapeKind::RoundedRectangle => NodeShape::RoundedRectangle,
        ShapeKind::Ellipse => NodeShape::Ellipse,
        ShapeKind::Diamond => NodeShape::Diamond,
        ShapeKind::Triangle => NodeShape::Triangle,
        ShapeKind::Custom(_) => NodeShape::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::CustomKind;

    fn node_ref(kind: &ElementKind) -> NodeRef<'_> {
        NodeRef {
            index: NodeIndex::new(0),
            kind,
            label: None,
            size: Vec2::new(160.0, 60.0),
            handle_count: 4,
            selected: false,
        }
    }

    #[test]
    fn the_registry_ships_exactly_the_six_generic_kinds() {
        let registry = NodeRendererRegistry::with_generic_kinds();

        assert_eq!(registry.len(), 6, "six, not sixty — see the module doc");
        for kind in GenericKind::ALL {
            assert!(registry.is_registered(kind.id()), "{kind:?}");
        }
    }

    /// The three kinds with no enum variant go through the public registration
    /// path, so the registry is exercised by its own contents rather than only
    /// by a hypothetical caller.
    #[test]
    fn the_kinds_without_an_enum_variant_resolve_through_the_map() {
        let registry = NodeRendererRegistry::with_generic_kinds();

        for kind in [
            GenericKind::Process,
            GenericKind::Decision,
            GenericKind::Note,
        ] {
            let element = kind.element_kind();
            assert_eq!(
                element.custom_name(),
                Some(kind.id()),
                "{kind:?} must reach the registry by name"
            );
            assert_eq!(registry.visual(node_ref(&element)), kind.visual());
        }
    }

    /// **The shape override, which is the reason `NodeVisual` carries a body.**
    /// A decision node is a diamond, and no `ElementKind` variant says so.
    #[test]
    fn a_registered_kind_can_choose_a_body_the_taxonomy_does_not_have() {
        let registry = NodeRendererRegistry::with_generic_kinds();
        let decision = GenericKind::Decision.element_kind();

        assert_eq!(
            NodeShape::of(&decision),
            NodeShape::Other,
            "the hot array cannot know this — that is the point"
        );
        assert_eq!(
            registry.visual(node_ref(&decision)).body,
            NodeShape::Diamond
        );
    }

    /// The promise `ElementKind::Custom` makes in the file format, kept in the
    /// renderer: an unknown kind draws as the generic node rather than
    /// vanishing.
    #[test]
    fn an_unregistered_kind_falls_back_rather_than_disappearing() {
        let registry = NodeRendererRegistry::with_generic_kinds();
        let unknown = ElementKind::Custom(CustomKind::new("dodo.mermaid.actor"));

        assert!(!registry.is_registered("dodo.mermaid.actor"));
        assert_eq!(registry.visual(node_ref(&unknown)), NodeVisual::FALLBACK);
    }

    #[test]
    fn a_later_registration_replaces_an_earlier_one() {
        let mut registry = NodeRendererRegistry::with_generic_kinds();
        let replacement = NodeVisual::FALLBACK.with_accent(AccentRole::Danger);
        registry.register(
            GenericKind::Process.id(),
            Arc::new(StaticRenderer(replacement)),
        );

        let kind = GenericKind::Process.element_kind();
        assert_eq!(registry.visual(node_ref(&kind)), replacement);
        assert_eq!(registry.len(), 6, "a replacement is not a second entry");
    }

    /// §43's performance clause, as a test: the built-in kinds must not consult
    /// the map, so an empty registry answers them identically to a full one.
    #[test]
    fn the_builtin_kinds_never_consult_the_map() {
        let empty = NodeRendererRegistry::new();
        let full = NodeRendererRegistry::with_generic_kinds();

        for kind in [
            ElementKind::GraphNode(GraphNodeKind::Default),
            ElementKind::GraphNode(GraphNodeKind::Input),
            ElementKind::GraphNode(GraphNodeKind::Output),
            ElementKind::GraphNode(GraphNodeKind::Group),
            ElementKind::Shape(ShapeKind::Ellipse),
        ] {
            assert_eq!(
                empty.visual(node_ref(&kind)),
                full.visual(node_ref(&kind)),
                "{kind:?} took a lookup it did not need"
            );
        }
    }

    /// A renderer that varies with the node is the reason this is a trait and
    /// not a table.
    #[test]
    fn a_renderer_may_answer_from_the_node_rather_than_from_the_kind() {
        struct ByHandles;
        impl NodeRenderer for ByHandles {
            fn visual(&self, node: NodeRef<'_>) -> NodeVisual {
                if node.handle_count > 2 {
                    NodeVisual::FALLBACK.with_accent(AccentRole::Danger)
                } else {
                    NodeVisual::FALLBACK
                }
            }
        }

        let mut registry = NodeRendererRegistry::new();
        registry.register("dodo.test.busy", Arc::new(ByHandles));
        let kind = ElementKind::Custom(CustomKind::new("dodo.test.busy"));

        let mut quiet = node_ref(&kind);
        quiet.handle_count = 1;
        assert_eq!(registry.visual(quiet).accent, AccentRole::Neutral);

        let mut busy = node_ref(&kind);
        busy.handle_count = 8;
        assert_eq!(registry.visual(busy).accent, AccentRole::Danger);
    }

    /// §40 rule 10 and §24: the descriptor is produced per rich node per frame,
    /// so it must not be able to grow a heap allocation without somebody
    /// noticing.
    #[test]
    fn a_visual_is_small_and_copy() {
        assert!(
            size_of::<NodeVisual>() <= 8,
            "NodeVisual is {} bytes; something with a payload was added",
            size_of::<NodeVisual>()
        );
    }

    #[test]
    fn kind_ids_are_stable_identifiers_and_not_display_text() {
        for kind in GenericKind::ALL {
            let id = kind.id();
            assert!(id.starts_with("dodo.flow."), "{id} is not a namespaced id");
            assert!(
                id.chars()
                    .all(|c| c.is_ascii_lowercase() || c == '.' || c == '_'),
                "{id} looks like prose; a kind id is never translated and never shown"
            );
        }
    }
}
