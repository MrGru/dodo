//! Bounded global catalog indexing over the existing `Driver::children` seam.
//!
//! Search performs one ordinary catalog call per expandable node, never one per
//! leaf. That means Redis keeps using its driver's `SCAN … TYPE` cursor pages:
//! key leaves are indexed and never queried, while the expandable “More…” node
//! advances the cursor. The whole walk is capped by calls and objects, runs on
//! the background executor, and checks a cancellation flag between calls.

use std::collections::{HashMap, HashSet};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use crate::database::models::catalog::{CatalogNode, NodeId, NodeKind, NodeLabel};
use crate::database::models::library::QueryScope;
use crate::database::services::Driver;
use crate::database::state::tree::CatalogTree;

pub const MAX_CALLS: usize = 1_000;
pub const MAX_NODES: usize = 10_000;

pub struct CatalogSource {
    pub scope: QueryScope,
    pub driver: Arc<dyn Driver>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogSearchEntry {
    pub scope: QueryScope,
    pub node: CatalogNode,
    /// Root through this node, including translated grouping nodes. Callers use
    /// [`path_names`](Self::path_names) when they need only server identifiers.
    pub path: Vec<CatalogNode>,
}

impl CatalogSearchEntry {
    pub fn path_names(&self) -> Vec<&str> {
        self.path
            .iter()
            .filter_map(|node| match &node.label {
                NodeLabel::Name(name) => Some(name.as_str()),
                NodeLabel::Group(_) => None,
            })
            .collect()
    }

    pub fn matches(&self, query: &str, localized_kind: &str) -> bool {
        let query = query.trim().to_lowercase();
        query.is_empty()
            || self.scope.matches(&query)
            || localized_kind.to_lowercase().contains(&query)
            || self.path.iter().any(|node| match &node.label {
                NodeLabel::Name(name) => name.to_lowercase().contains(&query),
                NodeLabel::Group(_) => false,
            })
            || self
                .node
                .detail
                .as_deref()
                .is_some_and(|detail| detail.to_lowercase().contains(&query))
    }
}

#[derive(Clone, Debug, Default)]
pub struct CatalogSnapshot {
    roots: Vec<CatalogNode>,
    children: HashMap<NodeId, Vec<CatalogNode>>,
}

impl CatalogSnapshot {
    /// Adopts only the levels needed to reveal one hit. The search index keeps
    /// the rest of the bounded cache; copying all 10,000 nodes into the visible
    /// tree would make every redraw walk objects under collapsed branches.
    pub fn reveal(&self, entry: &CatalogSearchEntry, tree: &mut CatalogTree) {
        tree.set_roots(Ok(self.roots.clone()));
        for ancestor in entry.path.iter().take(entry.path.len().saturating_sub(1)) {
            if let Some(children) = self.children.get(&ancestor.id) {
                tree.set_children(&ancestor.id, Ok(children.clone()));
                tree.expand(&ancestor.id);
            }
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct CatalogIndex {
    pub entries: Vec<CatalogSearchEntry>,
    pub snapshots: HashMap<u64, CatalogSnapshot>,
    pub truncated: bool,
    pub cancelled: bool,
    pub failures: usize,
    pub nodes: usize,
}

impl CatalogIndex {
    pub fn reveal(&self, entry: &CatalogSearchEntry, tree: &mut CatalogTree) {
        if let Some(snapshot) = self.snapshots.get(&entry.scope.connection_id) {
            snapshot.reveal(entry, tree);
        }
    }
}

pub fn crawl_catalogs(sources: Vec<CatalogSource>, cancel: Arc<AtomicBool>) -> CatalogIndex {
    crawl_with_limits(sources, cancel, MAX_CALLS, MAX_NODES)
}

fn crawl_with_limits(
    sources: Vec<CatalogSource>,
    cancel: Arc<AtomicBool>,
    max_calls: usize,
    max_nodes: usize,
) -> CatalogIndex {
    let mut index = CatalogIndex::default();
    let mut calls = 0usize;
    let source_count = sources.len();

    for (source_index, source) in sources.into_iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            index.cancelled = true;
            break;
        }
        if calls >= max_calls || index.nodes >= max_nodes {
            index.truncated = true;
            break;
        }

        calls += 1;
        let mut snapshot = CatalogSnapshot::default();
        let roots = match source.driver.children(None) {
            Ok(nodes) => bound_nodes(nodes, &mut index, max_nodes),
            Err(_) => {
                index.failures += 1;
                index.snapshots.insert(source.scope.connection_id, snapshot);
                continue;
            }
        };
        snapshot.roots = roots.clone();
        let mut stack = Vec::new();
        add_nodes(roots, &[], &source.scope, &mut index, &mut stack);
        let mut visited = HashSet::new();

        while let Some((parent, path)) = stack.pop() {
            if cancel.load(Ordering::Relaxed) {
                index.cancelled = true;
                break;
            }
            if calls >= max_calls || index.nodes >= max_nodes {
                index.truncated = true;
                break;
            }
            if !visited.insert(parent.id.clone()) {
                continue;
            }

            calls += 1;
            match source.driver.children(Some(&parent.id)) {
                Ok(nodes) => {
                    let nodes = bound_nodes(nodes, &mut index, max_nodes);
                    snapshot.children.insert(parent.id.clone(), nodes.clone());
                    add_nodes(nodes, &path, &source.scope, &mut index, &mut stack);
                }
                Err(_) => index.failures += 1,
            }
        }

        index.snapshots.insert(source.scope.connection_id, snapshot);
        if index.cancelled || index.truncated {
            break;
        }
        if source_index + 1 < source_count && (calls >= max_calls || index.nodes >= max_nodes) {
            index.truncated = true;
            break;
        }
    }

    index
}

fn bound_nodes(
    mut nodes: Vec<CatalogNode>,
    index: &mut CatalogIndex,
    max_nodes: usize,
) -> Vec<CatalogNode> {
    let remaining = max_nodes.saturating_sub(index.nodes);
    if nodes.len() > remaining {
        nodes.truncate(remaining);
        index.truncated = true;
    }
    index.nodes += nodes.len();
    nodes
}

fn add_nodes(
    nodes: Vec<CatalogNode>,
    parent_path: &[CatalogNode],
    scope: &QueryScope,
    index: &mut CatalogIndex,
    stack: &mut Vec<(CatalogNode, Vec<CatalogNode>)>,
) {
    let mut expandable = Vec::new();
    for node in nodes {
        let mut path = parent_path.to_vec();
        path.push(node.clone());
        if node.kind != NodeKind::Folder {
            index.entries.push(CatalogSearchEntry {
                scope: scope.clone(),
                node: node.clone(),
                path: path.clone(),
            });
        }
        if node.expandable {
            expandable.push((node, path));
        }
    }
    stack.extend(expandable.into_iter().rev());
}

#[cfg(test)]
mod tests {
    use super::{CatalogSource, crawl_catalogs, crawl_with_limits};
    use crate::database::models::catalog::{CatalogNode, GroupLabel, NodeId, NodeKind, NodeLabel};
    use crate::database::models::connection::ConnectionProfile;
    use crate::database::models::engine::Engine;
    use crate::database::models::error::DbError;
    use crate::database::models::library::QueryScope;
    use crate::database::models::page::RowSink;
    use crate::database::models::query::{Execution, QueryRequest};
    use crate::database::services::fake::FakeDriver;
    use crate::database::services::{Capabilities, Driver};
    use crate::database::state::tree::CatalogTree;
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    };

    fn scope() -> QueryScope {
        let mut profile = ConnectionProfile::new(7, Engine::PostgreSql);
        profile.name = "Local".into();
        QueryScope::from_profile(&profile)
    }

    fn source(driver: Arc<dyn Driver>) -> CatalogSource {
        CatalogSource {
            scope: scope(),
            driver,
        }
    }

    #[test]
    fn fake_driver_composes_into_search_and_reveals_the_cached_path() {
        let index = crawl_catalogs(
            vec![source(Arc::new(FakeDriver::sql()))],
            Arc::new(AtomicBool::new(false)),
        );
        assert!(!index.entries.is_empty());
        let table = index
            .entries
            .iter()
            .find(|entry| {
                entry.node.kind == NodeKind::Table
                    && matches!(&entry.node.label, NodeLabel::Name(name) if name == "users")
            })
            .expect("users table indexed");
        assert!(table.matches("users", "Table"));
        assert_eq!(table.scope.connection_id, 7);

        let mut tree = CatalogTree::default();
        index.reveal(table, &mut tree);
        assert!(
            tree.outline()
                .iter()
                .any(|root| root.expanded || root.id == table.node.id.as_str())
        );
    }

    struct ChainDriver {
        calls: AtomicUsize,
    }

    impl Driver for ChainDriver {
        fn capabilities(&self) -> Capabilities {
            FakeDriver::sql().capabilities()
        }

        fn ping(&self) -> Result<(), DbError> {
            Ok(())
        }

        fn children(&self, parent: Option<&NodeId>) -> Result<Vec<CatalogNode>, DbError> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            let number = parent
                .and_then(|id| id.as_str().strip_prefix('n'))
                .and_then(|number| number.parse::<usize>().ok())
                .map_or(0, |number| number + 1);
            Ok(vec![CatalogNode::branch(
                format!("n{number}"),
                NodeKind::Namespace,
                format!("node-{number}"),
            )])
        }

        fn execute(&self, _: &QueryRequest, _: &mut dyn RowSink) -> Result<Execution, DbError> {
            Ok(Execution::default())
        }
    }

    #[test]
    fn remote_calls_nodes_and_cancellation_are_bounded() {
        let driver = Arc::new(ChainDriver {
            calls: AtomicUsize::new(0),
        });
        let index = crawl_with_limits(
            vec![source(driver.clone())],
            Arc::new(AtomicBool::new(false)),
            4,
            50,
        );
        assert!(index.truncated);
        assert_eq!(driver.calls.load(Ordering::Relaxed), 4);

        let cancelled_driver = Arc::new(ChainDriver {
            calls: AtomicUsize::new(0),
        });
        let index = crawl_with_limits(
            vec![source(cancelled_driver.clone())],
            Arc::new(AtomicBool::new(true)),
            50,
            50,
        );
        assert!(index.cancelled);
        assert_eq!(cancelled_driver.calls.load(Ordering::Relaxed), 0);
    }

    struct CursorDriver {
        calls: Mutex<Vec<String>>,
    }

    impl Driver for CursorDriver {
        fn capabilities(&self) -> Capabilities {
            FakeDriver::key_value().capabilities()
        }

        fn ping(&self) -> Result<(), DbError> {
            Ok(())
        }

        fn children(&self, parent: Option<&NodeId>) -> Result<Vec<CatalogNode>, DbError> {
            let id = parent.map_or("root", NodeId::as_str).to_string();
            self.calls.lock().expect("call log").push(id.clone());
            Ok(match id.as_str() {
                "root" => vec![CatalogNode::branch("db0", NodeKind::Namespace, "db0")],
                "db0" => vec![CatalogNode::branch(
                    "type:hash",
                    NodeKind::Namespace,
                    "hash",
                )],
                "type:hash" => vec![
                    CatalogNode::leaf("key:first", NodeKind::Key, "first"),
                    CatalogNode::group("page:17", GroupLabel::More),
                ],
                "page:17" => vec![CatalogNode::leaf("key:second", NodeKind::Key, "second")],
                _ => panic!("a key leaf was queried: {id}"),
            })
        }

        fn execute(&self, _: &QueryRequest, _: &mut dyn RowSink) -> Result<Execution, DbError> {
            Ok(Execution::default())
        }
    }

    #[test]
    fn redis_shaped_search_follows_cursor_pages_and_never_queries_a_key() {
        let driver = Arc::new(CursorDriver {
            calls: Mutex::new(Vec::new()),
        });
        let index = crawl_catalogs(
            vec![source(driver.clone())],
            Arc::new(AtomicBool::new(false)),
        );
        let calls = driver.calls.lock().expect("call log");
        assert_eq!(calls.as_slice(), ["root", "db0", "type:hash", "page:17"]);
        assert_eq!(
            index
                .entries
                .iter()
                .filter(|entry| entry.node.kind == NodeKind::Key)
                .count(),
            2
        );
    }
}
