//! The Runtimes page store: the last detected row per [`RuntimeKind`], which
//! kind (if any) currently has a Start/Stop in flight, and a transient action
//! error banner. Plain data, no GPUI.
//!
//! Deliberately not built on [`containers::LoadStatus`](super::containers::LoadStatus):
//! that type's `Failed(Str)` arm models an engine the page cannot reach at
//! all, and [`RuntimeService::detect_all`](crate::docker::services::runtime::RuntimeService::detect_all)
//! has no equivalent failure — a kind whose own command could not run simply
//! becomes an `Unknown`/`NotInstalled` *row*, never a failure of the whole
//! list. [`RuntimeLoadStatus`] only ever distinguishes the first load (show
//! the skeleton) from everything after.

use crate::docker::models::runtime::{RuntimeInfo, RuntimeKind};
use crate::i18n::Str;

/// Where the Runtimes page's own load is.
#[derive(Default, PartialEq, Eq)]
pub enum RuntimeLoadStatus {
    /// The first detect is in flight and no rows have arrived yet.
    #[default]
    Loading,
    Ready,
}

#[derive(Default)]
pub struct RuntimeListState {
    status: RuntimeLoadStatus,
    /// One row per [`RuntimeKind::ALL`], in that order, from the last
    /// successful detect.
    rows: Vec<RuntimeInfo>,
    /// The kind a Start/Stop is currently running for. Its row disables its
    /// button and shows a pending label instead of a background poll's stale
    /// result flickering back on top of it before the action's own refresh
    /// lands.
    pending: Option<RuntimeKind>,
    action_error: Option<Str>,
}

impl RuntimeListState {
    /// True only for the very first load — a background poll re-detecting
    /// must not blank rows already on screen.
    pub fn is_loading(&self) -> bool {
        matches!(self.status, RuntimeLoadStatus::Loading) && self.rows.is_empty()
    }

    pub fn rows(&self) -> &[RuntimeInfo] {
        &self.rows
    }

    pub fn pending(&self) -> Option<RuntimeKind> {
        self.pending
    }

    pub fn is_pending(&self, kind: RuntimeKind) -> bool {
        self.pending == Some(kind)
    }

    pub fn action_error(&self) -> Option<&Str> {
        self.action_error.as_ref()
    }

    pub fn begin_load(&mut self) {
        self.status = RuntimeLoadStatus::Loading;
    }

    pub fn set_rows(&mut self, rows: Vec<RuntimeInfo>) {
        self.rows = rows;
        self.status = RuntimeLoadStatus::Ready;
    }

    /// Marks `kind`'s Start/Stop as in flight and clears any stale action
    /// error, the same "next action clears the banner" rule the other Docker
    /// pages' `action_error` follows.
    pub fn begin_action(&mut self, kind: RuntimeKind) {
        self.pending = Some(kind);
        self.action_error = None;
    }

    pub fn finish_action(&mut self) {
        self.pending = None;
    }

    pub fn set_action_error(&mut self, message: Str) {
        self.pending = None;
        self.action_error = Some(message);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::docker::models::runtime::RuntimeStatus;
    use crate::paths::HostOs;

    fn row(kind: RuntimeKind, status: RuntimeStatus) -> RuntimeInfo {
        RuntimeInfo::new(kind, status, None, HostOs::MacOs)
    }

    #[test]
    fn loading_is_only_true_before_the_first_rows_arrive() {
        let mut state = RuntimeListState::default();
        assert!(state.is_loading());
        state.set_rows(vec![row(RuntimeKind::Docker, RuntimeStatus::Running)]);
        assert!(!state.is_loading());
        // A later `begin_load` (a poll tick) does not blank the page: rows
        // already on screen mean this is not the first load any more.
        state.begin_load();
        assert!(!state.is_loading());
    }

    #[test]
    fn begin_action_clears_a_stale_error_and_finish_clears_pending() {
        let mut state = RuntimeListState::default();
        state.set_action_error(Str::RuntimeBinaryNotFound);
        assert!(state.action_error().is_some());

        state.begin_action(RuntimeKind::Docker);
        assert!(state.action_error().is_none());
        assert!(state.is_pending(RuntimeKind::Docker));
        assert!(!state.is_pending(RuntimeKind::Kubernetes));

        state.finish_action();
        assert_eq!(state.pending(), None);
    }

    #[test]
    fn set_action_error_also_clears_pending() {
        let mut state = RuntimeListState::default();
        state.begin_action(RuntimeKind::Containerd);
        state.set_action_error(Str::RuntimeBinaryNotFound);
        assert_eq!(state.pending(), None);
        assert!(state.action_error().is_some());
    }

}
