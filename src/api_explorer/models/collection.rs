//! The Collections tree as plain data: collections, folders and saved requests.
//!
//! The tree is nested — a [`Node`] owns its children — and every edit
//! (create, rename, delete, duplicate) is a method on [`CollectionTree`], so
//! the view layer only ever describes an intent and this module keeps the tree
//! consistent (unique ids, a copy inserted beside its original). None of it
//! touches GPUI, so all of it is unit tested.
//!
//! Drag-and-drop reordering is deliberately absent: the data model supports it
//! trivially (children are an ordered `Vec`), but the gesture is future work.

use serde::{Deserialize, Serialize};

use crate::api_explorer::models::snapshot::RequestSnapshot;

/// A stable, never-reused identifier for a node, so a click on a row that has
/// since moved cannot land on a different node.
pub type NodeId = u64;

/// What a node is. Collections and folders hold children; a request holds a
/// saved [`RequestSnapshot`] and no children.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum NodeKind {
    Collection,
    Folder,
    /// Boxed so the enum is not sized by its largest variant on every node.
    Request(Box<RequestSnapshot>),
}

/// One node of the tree.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Node {
    pub id: NodeId,
    pub name: String,
    pub kind: NodeKind,
    /// Empty for a request; the children of a collection or folder otherwise.
    #[serde(default)]
    pub children: Vec<Node>,
    /// Whether a container is showing its children. Persisted so the tree opens
    /// the way it was left.
    #[serde(default = "yes")]
    pub expanded: bool,
}

fn yes() -> bool {
    true
}

impl Node {
    /// Whether this node can hold children — i.e. is a collection or a folder.
    pub fn is_container(&self) -> bool {
        matches!(self.kind, NodeKind::Collection | NodeKind::Folder)
    }

    /// The saved request behind this node, if it is a request.
    pub fn snapshot(&self) -> Option<&RequestSnapshot> {
        match &self.kind {
            NodeKind::Request(snapshot) => Some(snapshot),
            _ => None,
        }
    }

    /// The largest id in this subtree, so the tree can resume allocating ids
    /// above everything a loaded file already used.
    fn max_id(&self) -> NodeId {
        self.children
            .iter()
            .map(Node::max_id)
            .max()
            .unwrap_or(0)
            .max(self.id)
    }

    /// Deep-copies the subtree, giving every node a fresh id from `next`.
    fn deep_copy(&self, next: &mut NodeId) -> Node {
        let id = *next;
        *next += 1;
        Node {
            id,
            name: self.name.clone(),
            kind: self.kind.clone(),
            children: self
                .children
                .iter()
                .map(|child| child.deep_copy(next))
                .collect(),
            expanded: self.expanded,
        }
    }
}

/// The whole Collections tree, plus its id allocator.
#[derive(Default)]
pub struct CollectionTree {
    roots: Vec<Node>,
    next_id: NodeId,
}

impl CollectionTree {
    /// Rebuilds a tree from nodes loaded off disk, resuming id allocation above
    /// the highest id any of them already carries.
    pub fn from_roots(roots: Vec<Node>) -> Self {
        let next_id = roots
            .iter()
            .map(Node::max_id)
            .max()
            .map_or(0, |max| max + 1);
        Self { roots, next_id }
    }

    pub fn roots(&self) -> &[Node] {
        &self.roots
    }

    pub fn is_empty(&self) -> bool {
        self.roots.is_empty()
    }

    fn alloc(&mut self) -> NodeId {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// Adds a top-level collection and returns its id.
    pub fn add_collection(&mut self, name: String) -> NodeId {
        let id = self.alloc();
        self.roots.push(Node {
            id,
            name,
            kind: NodeKind::Collection,
            children: Vec::new(),
            expanded: true,
        });
        id
    }

    /// Adds a folder under `parent`, if `parent` is a container. Returns the new
    /// folder's id, or `None` if the parent is missing or is a request.
    pub fn add_folder(&mut self, parent: NodeId, name: String) -> Option<NodeId> {
        let id = self.alloc();
        let node = Node {
            id,
            name,
            kind: NodeKind::Folder,
            children: Vec::new(),
            expanded: true,
        };
        if self.insert_child(parent, node) {
            Some(id)
        } else {
            None
        }
    }

    /// Adds a saved request under `parent`, if `parent` is a container. Returns
    /// the new request's id, or `None` if the parent cannot hold it.
    pub fn add_request(
        &mut self,
        parent: NodeId,
        name: String,
        snapshot: RequestSnapshot,
    ) -> Option<NodeId> {
        let id = self.alloc();
        let node = Node {
            id,
            name,
            kind: NodeKind::Request(Box::new(snapshot)),
            children: Vec::new(),
            expanded: true,
        };
        if self.insert_child(parent, node) {
            Some(id)
        } else {
            None
        }
    }

    /// The first collection's id, creating one named `default_name` if the tree
    /// is empty. This is the target "Save request" uses when the user has not
    /// picked a collection yet.
    pub fn first_or_new_collection(&mut self, default_name: String) -> NodeId {
        if let Some(first) = self
            .roots
            .iter()
            .find(|node| matches!(node.kind, NodeKind::Collection))
        {
            first.id
        } else {
            self.add_collection(default_name)
        }
    }

    /// Renames a node. Returns whether one was found.
    pub fn rename(&mut self, id: NodeId, name: String) -> bool {
        match find_mut(&mut self.roots, id) {
            Some(node) => {
                node.name = name;
                true
            }
            None => false,
        }
    }

    /// Removes a node and its subtree. Returns the removed node, if any.
    pub fn remove(&mut self, id: NodeId) -> Option<Node> {
        remove_from(&mut self.roots, id)
    }

    /// Copies a node (and its subtree) with fresh ids, inserting the copy
    /// directly after the original among its siblings. Returns the copy's id.
    pub fn duplicate(&mut self, id: NodeId) -> Option<NodeId> {
        let (siblings, index) = locate_mut(&mut self.roots, id)?;
        let mut next = self.next_id;
        let copy = siblings[index].deep_copy(&mut next);
        let new_id = copy.id;
        self.next_id = next;
        siblings.insert(index + 1, copy);
        Some(new_id)
    }

    /// Merges imported collections in as new top-level nodes, re-numbering every
    /// imported node so its ids cannot collide with the existing tree.
    pub fn import(&mut self, roots: Vec<Node>) {
        let mut next = self.next_id;
        for root in &roots {
            self.roots.push(root.deep_copy(&mut next));
        }
        self.next_id = next;
    }

    /// Sets whether a container is expanded.
    pub fn set_expanded(&mut self, id: NodeId, expanded: bool) {
        if let Some(node) = find_mut(&mut self.roots, id) {
            node.expanded = expanded;
        }
    }

    /// The saved request behind a node, if it is a request.
    pub fn snapshot(&self, id: NodeId) -> Option<&RequestSnapshot> {
        find(&self.roots, id).and_then(Node::snapshot)
    }

    fn insert_child(&mut self, parent: NodeId, node: Node) -> bool {
        match find_mut(&mut self.roots, parent) {
            Some(parent) if parent.is_container() => {
                parent.expanded = true;
                parent.children.push(node);
                true
            }
            _ => false,
        }
    }
}

/// Finds a node anywhere in the forest.
fn find(nodes: &[Node], id: NodeId) -> Option<&Node> {
    for node in nodes {
        if node.id == id {
            return Some(node);
        }
        if let Some(found) = find(&node.children, id) {
            return Some(found);
        }
    }
    None
}

/// Finds a node mutably anywhere in the forest.
fn find_mut(nodes: &mut [Node], id: NodeId) -> Option<&mut Node> {
    for node in nodes.iter_mut() {
        if node.id == id {
            return Some(node);
        }
        if let Some(found) = find_mut(&mut node.children, id) {
            return Some(found);
        }
    }
    None
}

/// The sibling `Vec` a node lives in, and its index within it.
fn locate_mut(nodes: &mut Vec<Node>, id: NodeId) -> Option<(&mut Vec<Node>, usize)> {
    if let Some(index) = nodes.iter().position(|node| node.id == id) {
        return Some((nodes, index));
    }
    for node in nodes.iter_mut() {
        if let Some(found) = locate_mut(&mut node.children, id) {
            return Some(found);
        }
    }
    None
}

/// Removes a node from wherever it is in the forest.
fn remove_from(nodes: &mut Vec<Node>, id: NodeId) -> Option<Node> {
    if let Some(index) = nodes.iter().position(|node| node.id == id) {
        return Some(nodes.remove(index));
    }
    for node in nodes.iter_mut() {
        if let Some(removed) = remove_from(&mut node.children, id) {
            return Some(removed);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::CollectionTree;
    use crate::api_explorer::models::snapshot::RequestSnapshot;

    fn snapshot(url: &str) -> RequestSnapshot {
        RequestSnapshot {
            url: url.into(),
            ..RequestSnapshot::default()
        }
    }

    #[test]
    fn a_new_tree_is_empty() {
        let tree = CollectionTree::default();
        assert!(tree.is_empty());
    }

    #[test]
    fn requests_nest_under_a_collection_and_a_folder() {
        let mut tree = CollectionTree::default();
        let collection = tree.add_collection("APIs".into());
        let folder = tree
            .add_folder(collection, "Auth".into())
            .expect("folder added");
        let request = tree
            .add_request(folder, "Login".into(), snapshot("https://x/login"))
            .expect("request added");

        assert_eq!(
            tree.snapshot(request).map(|s| s.url.as_str()),
            Some("https://x/login")
        );
        // The saved node is a request, not a container.
        assert!(tree.snapshot(request).is_some());
        assert!(tree.snapshot(collection).is_none());
    }

    #[test]
    fn a_request_cannot_hold_children() {
        let mut tree = CollectionTree::default();
        let collection = tree.add_collection("APIs".into());
        let request = tree
            .add_request(collection, "One".into(), snapshot("https://x"))
            .expect("added");
        assert!(tree.add_folder(request, "Nope".into()).is_none());
    }

    #[test]
    fn ids_are_unique_across_creates() {
        let mut tree = CollectionTree::default();
        let a = tree.add_collection("A".into());
        let b = tree.add_collection("B".into());
        let c = tree.add_folder(a, "F".into()).expect("folder");
        assert_ne!(a, b);
        assert_ne!(b, c);
        assert_ne!(a, c);
    }

    #[test]
    fn rename_changes_the_name() {
        let mut tree = CollectionTree::default();
        let id = tree.add_collection("Old".into());
        assert!(tree.rename(id, "New".into()));
        assert_eq!(tree.roots()[0].name, "New");
        assert!(!tree.rename(9999, "x".into()));
    }

    #[test]
    fn remove_takes_the_whole_subtree() {
        let mut tree = CollectionTree::default();
        let collection = tree.add_collection("APIs".into());
        let folder = tree.add_folder(collection, "F".into()).expect("folder");
        tree.add_request(folder, "R".into(), snapshot("https://x"))
            .expect("request");

        let removed = tree.remove(folder).expect("folder removed");
        assert_eq!(removed.children.len(), 1);
        assert!(tree.roots()[0].children.is_empty());
    }

    #[test]
    fn duplicate_makes_a_deep_copy_with_new_ids_beside_the_original() {
        let mut tree = CollectionTree::default();
        let collection = tree.add_collection("APIs".into());
        let folder = tree.add_folder(collection, "F".into()).expect("folder");
        tree.add_request(folder, "R".into(), snapshot("https://x"))
            .expect("request");

        let copy = tree.duplicate(folder).expect("duplicated");
        assert_ne!(copy, folder);

        let root = &tree.roots()[0];
        assert_eq!(root.children.len(), 2, "the copy sits beside the original");
        // The copy's request is a distinct node with its own id.
        let original_request = root.children[0].children[0].id;
        let copied_request = root.children[1].children[0].id;
        assert_ne!(original_request, copied_request);
        assert_eq!(
            root.children[1].children[0]
                .snapshot()
                .map(|s| s.url.as_str()),
            Some("https://x")
        );
    }

    #[test]
    fn save_targets_an_existing_collection_or_makes_one() {
        let mut tree = CollectionTree::default();
        let made = tree.first_or_new_collection("My Collection".into());
        assert_eq!(tree.roots().len(), 1);
        // A second save reuses the same collection rather than making another.
        let again = tree.first_or_new_collection("My Collection".into());
        assert_eq!(made, again);
        assert_eq!(tree.roots().len(), 1);
    }

    #[test]
    fn from_roots_resumes_ids_above_the_highest_loaded_one() {
        let mut tree = CollectionTree::default();
        tree.add_collection("A".into());
        tree.add_collection("B".into());
        let roots: Vec<_> = tree.roots().to_vec();
        let max = roots.iter().map(|n| n.id).max().unwrap();

        let mut reloaded = CollectionTree::from_roots(roots);
        let fresh = reloaded.add_collection("C".into());
        assert!(fresh > max, "a reloaded tree must not reuse an id");
    }

    #[test]
    fn a_container_expands_when_something_is_added_to_it() {
        let mut tree = CollectionTree::default();
        let collection = tree.add_collection("APIs".into());
        tree.set_expanded(collection, false);
        tree.add_request(collection, "R".into(), snapshot("https://x"))
            .expect("request");
        assert!(tree.roots()[0].expanded, "adding a child reveals it");
    }
}
