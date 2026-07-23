//! An expand/collapse tree over a JSON document.
//!
//! The response viewer's Tree mode renders large JSON without freezing by never
//! walking what is collapsed. Parsing is done once (here); rendering asks for a
//! **flat list of visible rows**, which only descends into expanded containers
//! and stops at a row budget, so a 200 000-node document costs a screenful of
//! work rather than 200 000 elements.
//!
//! Nodes are addressed by a path of child indices, which stays valid across
//! re-renders because the tree's shape never changes — only which nodes are
//! expanded. Deep nodes start collapsed, so opening a huge document shows only
//! its top level until the user drills in.

use serde_json::Value;

/// How deep the tree is expanded when first parsed. The root and its immediate
/// children are open; everything below starts collapsed.
const DEFAULT_EXPAND_DEPTH: usize = 1;

/// The most rows [`JsonTree::visible_rows`] will emit before it stops and
/// reports the rest as withheld — a rendering guard, not a data limit.
pub const ROW_BUDGET: usize = 2000;

/// What a scalar is, so the view can colour it by type.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ScalarKind {
    Null,
    Bool,
    Number,
    String,
}

/// A node's key: nothing for the root, an index in an array, a field in an
/// object.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Key {
    Root,
    Index(usize),
    Field(String),
}

#[derive(Clone, Debug)]
enum NodeValue {
    Scalar { text: String, kind: ScalarKind },
    Array(Vec<JsonNode>),
    Object(Vec<JsonNode>),
}

#[derive(Clone, Debug)]
struct JsonNode {
    key: Key,
    value: NodeValue,
    expanded: bool,
}

impl JsonNode {
    fn from_value(key: Key, value: &Value, depth: usize) -> Self {
        let expanded = depth < DEFAULT_EXPAND_DEPTH;
        let value = match value {
            Value::Null => NodeValue::Scalar {
                text: "null".into(),
                kind: ScalarKind::Null,
            },
            Value::Bool(b) => NodeValue::Scalar {
                text: b.to_string(),
                kind: ScalarKind::Bool,
            },
            Value::Number(n) => NodeValue::Scalar {
                text: n.to_string(),
                kind: ScalarKind::Number,
            },
            Value::String(s) => NodeValue::Scalar {
                text: s.clone(),
                kind: ScalarKind::String,
            },
            Value::Array(items) => NodeValue::Array(
                items
                    .iter()
                    .enumerate()
                    .map(|(index, item)| JsonNode::from_value(Key::Index(index), item, depth + 1))
                    .collect(),
            ),
            Value::Object(map) => NodeValue::Object(
                map.iter()
                    .map(|(name, item)| {
                        JsonNode::from_value(Key::Field(name.clone()), item, depth + 1)
                    })
                    .collect(),
            ),
        };
        Self {
            key,
            value,
            expanded,
        }
    }

    fn children(&self) -> Option<&[JsonNode]> {
        match &self.value {
            NodeValue::Array(children) | NodeValue::Object(children) => Some(children),
            NodeValue::Scalar { .. } => None,
        }
    }

    fn child_count(&self) -> usize {
        self.children().map_or(0, <[JsonNode]>::len)
    }
}

/// A parsed JSON document as a tree.
pub struct JsonTree {
    root: JsonNode,
}

/// The label a row carries: the array index or object field it sits at.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RowLabel {
    Root,
    Index(usize),
    Field(String),
}

/// What a visible row shows.
#[derive(Clone, Debug)]
pub enum RowContent {
    Scalar { text: String, kind: ScalarKind },
    /// A container: the brackets to draw, how many children it has, and whether
    /// it is open.
    Container {
        open: char,
        close: char,
        count: usize,
        expanded: bool,
    },
}

/// One line of the rendered tree.
#[derive(Clone, Debug)]
pub struct JsonRow {
    /// The path used to toggle this row, if it is a container.
    pub path: Vec<usize>,
    pub depth: usize,
    pub label: RowLabel,
    pub content: RowContent,
}

/// The visible rows of a tree, plus whether the row budget cut any off.
pub struct VisibleRows {
    pub rows: Vec<JsonRow>,
    pub truncated: bool,
}

impl JsonTree {
    /// Parses `text` as JSON. Returns `None` when it does not parse — the caller
    /// falls back to the plain text view rather than showing an error.
    pub fn parse(text: &str) -> Option<Self> {
        let value: Value = serde_json::from_str(text).ok()?;
        Some(Self {
            root: JsonNode::from_value(Key::Root, &value, 0),
        })
    }

    /// Whether the document is a container at all — a bare scalar has no tree
    /// worth showing.
    pub fn is_expandable(&self) -> bool {
        self.root.children().is_some()
    }

    /// Flips a container node's expanded state. An out-of-range path is ignored
    /// rather than panicking — a stale click from a re-render is not an error.
    pub fn toggle(&mut self, path: &[usize]) {
        let mut node = &mut self.root;
        for &index in path {
            let next = match &mut node.value {
                NodeValue::Array(children) | NodeValue::Object(children) => children.get_mut(index),
                NodeValue::Scalar { .. } => None,
            };
            match next {
                Some(child) => node = child,
                None => return,
            }
        }
        if node.children().is_some() {
            node.expanded = !node.expanded;
        }
    }

    /// The rows to draw: the root, then every node reachable through an expanded
    /// container, up to [`ROW_BUDGET`].
    pub fn visible_rows(&self) -> VisibleRows {
        let mut rows = Vec::new();
        let mut path = Vec::new();
        let truncated = !self.push_rows(&self.root, 0, &mut path, &mut rows);
        VisibleRows { rows, truncated }
    }

    /// Appends `node` and, if expanded, its descendants. Returns `false` when
    /// the budget was hit and rows were withheld.
    fn push_rows(
        &self,
        node: &JsonNode,
        depth: usize,
        path: &mut Vec<usize>,
        rows: &mut Vec<JsonRow>,
    ) -> bool {
        if rows.len() >= ROW_BUDGET {
            return false;
        }

        let label = match &node.key {
            Key::Root => RowLabel::Root,
            Key::Index(index) => RowLabel::Index(*index),
            Key::Field(name) => RowLabel::Field(name.clone()),
        };

        let content = match &node.value {
            NodeValue::Scalar { text, kind } => RowContent::Scalar {
                text: text.clone(),
                kind: *kind,
            },
            NodeValue::Array(_) => RowContent::Container {
                open: '[',
                close: ']',
                count: node.child_count(),
                expanded: node.expanded,
            },
            NodeValue::Object(_) => RowContent::Container {
                open: '{',
                close: '}',
                count: node.child_count(),
                expanded: node.expanded,
            },
        };

        rows.push(JsonRow {
            path: path.clone(),
            depth,
            label,
            content,
        });

        if let Some(children) = node.children().filter(|_| node.expanded) {
            for (index, child) in children.iter().enumerate() {
                path.push(index);
                let ok = self.push_rows(child, depth + 1, path, rows);
                path.pop();
                if !ok {
                    return false;
                }
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::{JsonTree, RowContent, RowLabel, ScalarKind};

    #[test]
    fn invalid_json_does_not_parse() {
        assert!(JsonTree::parse("{not json").is_none());
    }

    #[test]
    fn the_root_object_shows_its_keys_but_nested_objects_start_collapsed() {
        let tree = JsonTree::parse(r#"{"a": 1, "b": {"c": 2}}"#).expect("parses");
        let rows = tree.visible_rows().rows;
        // root, a, b — but not c, because b is collapsed at depth 1.
        assert_eq!(rows.len(), 3);
        assert!(matches!(rows[1].label, RowLabel::Field(ref f) if f == "a"));
        assert!(matches!(
            rows[2].content,
            RowContent::Container { expanded: false, count: 1, .. }
        ));
    }

    #[test]
    fn toggling_a_collapsed_node_reveals_its_children() {
        let mut tree = JsonTree::parse(r#"{"a": 1, "b": {"c": 2}}"#).expect("parses");
        // Path to "b": it is the second field of the root object → index 1.
        tree.toggle(&[1]);
        let rows = tree.visible_rows().rows;
        assert_eq!(rows.len(), 4, "c is now visible");
        assert!(matches!(rows[3].label, RowLabel::Field(ref f) if f == "c"));
    }

    #[test]
    fn scalars_are_typed_for_colouring() {
        let tree = JsonTree::parse(r#"[null, true, 3, "x"]"#).expect("parses");
        let rows = tree.visible_rows().rows;
        let kinds: Vec<_> = rows
            .iter()
            .filter_map(|row| match &row.content {
                RowContent::Scalar { kind, .. } => Some(*kind),
                _ => None,
            })
            .collect();
        assert_eq!(
            kinds,
            vec![
                ScalarKind::Null,
                ScalarKind::Bool,
                ScalarKind::Number,
                ScalarKind::String
            ]
        );
    }

    #[test]
    fn a_huge_array_is_capped_by_the_row_budget() {
        let big = format!("[{}]", (0..10_000).map(|n| n.to_string()).collect::<Vec<_>>().join(","));
        // The root array is expanded by default, so its 10k scalar children are
        // what the budget has to cut off.
        let tree = JsonTree::parse(&big).expect("parses");
        let visible = tree.visible_rows();
        assert!(visible.truncated, "the budget should cut a 10k-element array off");
        assert!(visible.rows.len() <= super::ROW_BUDGET + 1);
    }

    #[test]
    fn a_bare_scalar_document_is_not_expandable() {
        let tree = JsonTree::parse("42").expect("parses");
        assert!(!tree.is_expandable());
    }
}
