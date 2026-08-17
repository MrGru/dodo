//! [`EdgeGeometryStore`] — the derived half of every edge, and the count that
//! proves the architecture works.
//!
//! §23 asks for expensive derived geometry to be cached against a version, and
//! names edge routes first. This is that cache for routes: one
//! [`EdgeRoute`] per edge, marked stale by
//! [`EdgeDirty::GEOMETRY`](crate::runtime::EdgeDirty::GEOMETRY) and rebuilt on
//! demand. The tessellation cache §23 also asks for — the flattened, screen
//! space `Path` — is a different cache with a different key and a byte bound,
//! and it is Phase 4's; this one is world-space control points, which is what
//! makes it cheap enough to rebuild during a drag.
//!
//! # `rebuilds` is a test fixture, not telemetry
//!
//! §19's target is stated in units of *rebuilds*: moving one node with four
//! connected edges should cost four edge geometry rebuilds. A claim like that
//! is only worth having if it is asserted, so the store counts every rebuild it
//! performs and `runtime::world`'s property test moves one node in a
//! 100,000-node, 500,000-edge graph and fails if the count is anything but the
//! node's own degree. It costs one `u64` increment.
//!
//! # Invalid rather than absent
//!
//! A stale route is a route with `valid = false`, not a `None`. The difference
//! matters on the hot path: rebuilding into the existing [`EdgeRoute`] reuses
//! its segment buffer, so dragging a node through a hundred mouse moves
//! allocates once per incident edge instead of a hundred times.
//!
//! **This file names no UI framework.**

use crate::{
    geometry::{Attachment, EdgeRoute, RouteOptions, route::route_into},
    models::{EdgeIndex, EdgeRouting},
};

/// Every edge's derived route.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EdgeGeometryStore {
    routes: Vec<EdgeRoute>,
    valid: Vec<bool>,
    options: RouteOptions,
    rebuilds: u64,
}

impl EdgeGeometryStore {
    pub fn new() -> EdgeGeometryStore {
        EdgeGeometryStore::default()
    }

    pub fn len(&self) -> usize {
        self.routes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.routes.is_empty()
    }

    pub fn options(&self) -> &RouteOptions {
        &self.options
    }

    /// Changing the routing options invalidates **every** route, because every
    /// one of them was built with the old ones. This is the one operation here
    /// that is proportional to the graph, and it is a settings change rather
    /// than an interaction.
    pub fn set_options(&mut self, options: RouteOptions) {
        if self.options == options {
            return;
        }
        self.options = options;
        self.valid.iter_mut().for_each(|valid| *valid = false);
    }

    /// Adds a slot for a new edge, stale, so the first frame that wants it
    /// builds it.
    pub fn push_edge(&mut self) {
        self.routes.push(EdgeRoute::default());
        self.valid.push(false);
    }

    pub fn reserve(&mut self, additional: usize) {
        self.routes.reserve(additional);
        self.valid.reserve(additional);
    }

    /// The route, or `None` if it has never been built or is stale.
    ///
    /// A stale route is deliberately not returned. A caller that painted one
    /// would draw an edge hanging off a node that has already moved, which is
    /// the exact artefact the dirty tracking exists to prevent.
    pub fn route(&self, edge: EdgeIndex) -> Option<&EdgeRoute> {
        match self.valid.get(edge.index()) {
            Some(true) => self.routes.get(edge.index()),
            _ => None,
        }
    }

    /// The route whether or not it is current. For a caller that would rather
    /// paint a frame-old edge than none — an in-progress drag at overview LOD,
    /// say. Nothing does today; it exists so that "stale" and "absent" stay
    /// different questions.
    pub fn stale_route(&self, edge: EdgeIndex) -> Option<&EdgeRoute> {
        self.routes.get(edge.index())
    }

    pub fn is_valid(&self, edge: EdgeIndex) -> bool {
        self.valid.get(edge.index()).copied().unwrap_or(false)
    }

    pub fn invalidate(&mut self, edge: EdgeIndex) {
        if let Some(valid) = self.valid.get_mut(edge.index()) {
            *valid = false;
        }
    }

    /// **Rebuilds one route in place**, reusing its buffers, and counts it.
    pub fn rebuild(
        &mut self,
        edge: EdgeIndex,
        routing: EdgeRouting,
        source: Attachment,
        target: Attachment,
    ) {
        let Some(route) = self.routes.get_mut(edge.index()) else {
            return;
        };

        route_into(route, routing, source, target, &self.options);
        self.valid[edge.index()] = true;
        self.rebuilds += 1;
    }

    /// **How many routes have ever been rebuilt.** See the module doc: this is
    /// what §19's property test measures, by taking the difference across one
    /// node move.
    pub fn rebuild_count(&self) -> u64 {
        self.rebuilds
    }

    /// How many routes are stale — zero after a frame's rebuild pass, and a
    /// quick way for a test to say "nothing else was invalidated".
    pub fn stale_count(&self) -> usize {
        self.valid.iter().filter(|valid| !**valid).count()
    }
}

#[cfg(test)]
mod tests {
    use super::EdgeGeometryStore;
    use crate::{
        geometry::{Attachment, RouteOptions, Side, Vec2},
        models::{EdgeIndex, EdgeRouting},
    };

    fn store(edges: usize) -> EdgeGeometryStore {
        let mut store = EdgeGeometryStore::new();
        for _ in 0..edges {
            store.push_edge();
        }
        store
    }

    fn build(store: &mut EdgeGeometryStore, edge: EdgeIndex, to: f32) {
        store.rebuild(
            edge,
            EdgeRouting::Straight,
            Attachment::new(Vec2::ZERO, Side::Right),
            Attachment::new(Vec2::new(to, 0.0), Side::Left),
        );
    }

    #[test]
    fn a_new_edge_starts_stale_and_reports_no_route() {
        let store = store(2);

        assert!(!store.is_valid(EdgeIndex::new(0)));
        assert_eq!(store.route(EdgeIndex::new(0)), None);
        assert_eq!(store.stale_count(), 2);
        assert_eq!(store.rebuild_count(), 0);
    }

    #[test]
    fn a_rebuilt_route_becomes_valid_and_readable() {
        let mut store = store(1);
        let edge = EdgeIndex::new(0);

        build(&mut store, edge, 100.0);

        assert!(store.is_valid(edge));
        assert_eq!(
            store.route(edge).map(|r| r.end()),
            Some(Vec2::new(100.0, 0.0))
        );
        assert_eq!(store.rebuild_count(), 1);
        assert_eq!(store.stale_count(), 0);
    }

    /// A stale route is not handed out, because painting one draws an edge
    /// hanging off a node that has already moved.
    #[test]
    fn an_invalidated_route_is_withheld_but_not_forgotten() {
        let mut store = store(1);
        let edge = EdgeIndex::new(0);
        build(&mut store, edge, 100.0);

        store.invalidate(edge);

        assert_eq!(store.route(edge), None);
        assert!(store.stale_route(edge).is_some());
        assert_eq!(store.rebuild_count(), 1, "invalidating rebuilds nothing");
    }

    /// The count §19's property test reads. It must move by exactly the number
    /// of rebuilds performed and by nothing else.
    #[test]
    fn the_rebuild_count_tracks_rebuilds_and_only_rebuilds() {
        let mut store = store(3);
        let before = store.rebuild_count();

        build(&mut store, EdgeIndex::new(0), 10.0);
        build(&mut store, EdgeIndex::new(2), 30.0);
        store.invalidate(EdgeIndex::new(1));
        store.is_valid(EdgeIndex::new(0));

        assert_eq!(store.rebuild_count() - before, 2);
    }

    #[test]
    fn changing_the_options_invalidates_everything_that_was_built_with_the_old_ones() {
        let mut store = store(3);
        for edge in 0..3u32 {
            build(&mut store, EdgeIndex::new(edge), 10.0);
        }
        assert_eq!(store.stale_count(), 0);

        let mut options = RouteOptions::DEFAULT;
        options.step_offset = 40.0;
        store.set_options(options);

        assert_eq!(store.stale_count(), 3);
    }

    #[test]
    fn setting_the_same_options_again_invalidates_nothing() {
        let mut store = store(1);
        build(&mut store, EdgeIndex::new(0), 10.0);

        store.set_options(RouteOptions::DEFAULT);

        assert_eq!(store.stale_count(), 0);
    }

    #[test]
    fn an_index_past_the_end_is_ignored_rather_than_a_panic() {
        let mut store = store(1);
        let past = EdgeIndex::new(9);

        build(&mut store, past, 10.0);
        store.invalidate(past);

        assert_eq!(store.route(past), None);
        assert_eq!(store.rebuild_count(), 0);
    }
}
