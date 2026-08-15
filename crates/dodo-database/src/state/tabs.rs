//! The open query tabs.
//!
//! Round 1 had one editor and one result. Round 2 has several, and the whole
//! reason this is a type rather than a `Vec` on the view is the index
//! arithmetic: closing a tab moves the active one, and getting that wrong is
//! the bug that shows a different query's rows under the tab you were reading.
//! [`active_after_close`] is that arithmetic, pure and tested.
//!
//! # What a tab owns, and what it does not
//!
//! It owns its **text**, its **run in flight** and its **result**. It does not
//! own the connection: the tree's selection says which database the Execute
//! button runs against, for every tab at once. Two tabs pointed at two servers
//! would need two selections on screen and there is one, so a tab that
//! remembered its own would disagree with the panel beside it.
//!
//! Each tab has its **own** [`Entity<InputState>`], the way the API Explorer's
//! request tabs do, because an editor carries a cursor, a scroll position and
//! an undo history that a shared one would throw away on every switch. The
//! result grid is shared — only one is ever on screen — and the view re-fills
//! it from the active tab's [`QueryState`] when the active tab changes.
//!
//! # Tabs do not survive a restart
//!
//! Deliberately, and it is the standing decision rather than an oversight: a
//! restored tab whose connection is gone, or whose statement was half-typed, is
//! a puzzle rather than a convenience. `state::history` keeps every statement
//! that actually *ran* for the session, which is the part worth getting back.

use gpui::{Entity, Task};
use gpui_component::input::InputState;

use crate::i18n::Str;
use crate::services::CancelHandle;
use crate::state::editor::EditorLanguage;
use crate::state::query::QueryState;

/// One open query tab.
pub struct QueryTab {
    /// Stable for the tab's life. Every background task carries one of these
    /// rather than an index, because the user may close a tab to the left of
    /// it while a query is running and indices then mean something else.
    pub id: u64,
    /// The number in the tab's default title. Counts up for the session, so
    /// closing "Query 2" does not make the next new tab claim its name.
    pub number: usize,
    pub editor: Entity<InputState>,
    /// The grammar this tab's editor is pointed at. Per tab, because the guard
    /// is against re-pointing an *editor* on every frame and each tab has its
    /// own — see [`EditorLanguage`]'s module doc for what that cost round 1.
    pub language: EditorLanguage,
    pub query: QueryState,
    /// The connection that produced `query`'s completed result. Export must
    /// re-run against this connection, not whichever root was selected later.
    pub result_connection: Option<u64>,
    /// How to stop the run in flight, **at the server**.
    ///
    /// Taken from the driver *before* the statement starts, because the
    /// connection is locked for as long as it runs — see
    /// [`CancelHandle`]'s notes in `services/mod.rs`. `None` between runs, and
    /// for a backend that reports no cancel capability.
    pub cancel: Option<CancelHandle>,
    /// Export re-runs the displayed statement into a file. It keeps the result
    /// on screen, but still counts as this tab's one run in flight.
    pub exporting: bool,
    /// Held so the tab keeps its own task: a run in one tab must not be
    /// cancelled by a run in another.
    pub run_task: Option<Task<()>>,
    /// Held so a cancel that is still travelling is not dropped on the way.
    pub cancel_task: Option<Task<()>>,
    /// A one-line message about this tab that is not its result — dodo could
    /// not deliver a cancel request, and later what an export did. Held as a
    /// [`Str`] rather than rendered text so it re-translates.
    pub notice: Option<Str>,
    /// The only successful notice today is a completed export; everything else
    /// means the requested operation did not complete.
    pub notice_success: bool,
}

impl QueryTab {
    pub fn new(id: u64, number: usize, editor: Entity<InputState>) -> Self {
        Self {
            id,
            number,
            editor,
            language: EditorLanguage::new(),
            query: QueryState::Idle,
            result_connection: None,
            cancel: None,
            exporting: false,
            run_task: None,
            cancel_task: None,
            notice: None,
            notice_success: false,
        }
    }

    /// Whether a statement is in flight in this tab.
    pub fn is_running(&self) -> bool {
        self.exporting || matches!(self.query, QueryState::Running)
    }

    /// Whether Cancel is worth offering: something is running and there is a
    /// handle that can stop it.
    pub fn can_cancel(&self) -> bool {
        self.is_running() && self.cancel.is_some()
    }
}

/// Every open tab, and which one is on screen.
///
/// There is **always at least one**: an empty strip with no editor under it is
/// a dead end, so closing the last tab opens a fresh one in its place.
#[derive(Default)]
pub struct QueryTabs {
    tabs: Vec<QueryTab>,
    active: usize,
    next_id: u64,
    next_number: usize,
}

impl QueryTabs {
    pub fn new() -> Self {
        Self::default()
    }

    /// The id and number the next tab should be built with.
    ///
    /// Handed out before the tab exists because building one needs a `Window`,
    /// which this type deliberately knows nothing about.
    pub fn allocate(&mut self) -> (u64, usize) {
        self.next_id += 1;
        self.next_number += 1;
        (self.next_id, self.next_number)
    }

    /// Appends `tab` and makes it the one on screen — a new tab the user asked
    /// for is a tab they want to type in.
    pub fn push(&mut self, tab: QueryTab) {
        self.tabs.push(tab);
        self.active = self.tabs.len() - 1;
    }

    pub fn tabs(&self) -> &[QueryTab] {
        &self.tabs
    }

    /// How many tabs are open. There is no `is_empty` beside it on purpose:
    /// the page always has at least one tab, so the answer would always be
    /// `false` and reading it would suggest otherwise.
    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> usize {
        self.tabs.len()
    }

    pub fn active_index(&self) -> usize {
        self.active
    }

    pub fn active(&self) -> Option<&QueryTab> {
        self.tabs.get(self.active)
    }

    pub fn active_mut(&mut self) -> Option<&mut QueryTab> {
        self.tabs.get_mut(self.active)
    }

    /// The tab with `id`, if it is still open. What a finished background run
    /// looks itself up with.
    pub fn find_mut(&mut self, id: u64) -> Option<&mut QueryTab> {
        self.tabs.iter_mut().find(|tab| tab.id == id)
    }

    pub fn tab_mut(&mut self, index: usize) -> Option<&mut QueryTab> {
        self.tabs.get_mut(index)
    }

    /// Makes `index` the tab on screen. An out-of-range index is ignored: a
    /// stale click is not an error.
    pub fn select(&mut self, index: usize) {
        if index < self.tabs.len() {
            self.active = index;
        }
    }

    /// Removes the tab at `index` and returns it, so the caller can stop
    /// whatever it was running before dropping it.
    ///
    /// Returns `None` for an index that is not open, and — deliberately — for
    /// the *last* tab: closing the only tab is refused here rather than leaving
    /// the page with no editor, and the caller replaces its contents instead.
    pub fn close(&mut self, index: usize) -> Option<QueryTab> {
        if index >= self.tabs.len() || self.tabs.len() == 1 {
            return None;
        }
        let tab = self.tabs.remove(index);
        self.active = active_after_close(self.active, index, self.tabs.len());
        Some(tab)
    }
}

/// Where the active tab lands after the tab at `closed` is removed, leaving
/// `remaining` tabs.
///
/// The three cases, and why each is what it is:
///
/// - **A tab before the active one closed** — everything shifted left, so the
///   same tab keeps its place on screen. Not doing this is what makes closing a
///   tab silently switch the user to its neighbour.
/// - **The active tab closed** — the one that took its index shows, which is
///   the tab to its right; at the end of the strip there is none, so the new
///   last tab shows.
/// - **A tab after the active one closed** — nothing moved.
pub fn active_after_close(active: usize, closed: usize, remaining: usize) -> usize {
    let moved = if closed < active { active - 1 } else { active };
    moved.min(remaining.saturating_sub(1))
}

#[cfg(test)]
mod tests {
    use super::active_after_close;

    #[test]
    fn closing_a_tab_before_the_active_one_keeps_the_same_tab_on_screen() {
        // [a b C d], close a -> [b C d]: the active tab was index 2, now 1.
        assert_eq!(active_after_close(2, 0, 3), 1);
        assert_eq!(active_after_close(1, 0, 2), 0);
    }

    #[test]
    fn closing_a_tab_after_the_active_one_moves_nothing() {
        // [a B c d], close d -> [a B c]: still index 1.
        assert_eq!(active_after_close(1, 3, 3), 1);
        assert_eq!(active_after_close(0, 1, 2), 0);
    }

    /// The tab that took the closed one's index is the one to its right, which
    /// is what every editor does.
    #[test]
    fn closing_the_active_tab_shows_the_one_that_took_its_place() {
        // [a B c], close B -> [a c]: index 1 is now what was `c`.
        assert_eq!(active_after_close(1, 1, 2), 1);
    }

    /// The case the `min` exists for: the active tab was last, so there is
    /// nothing to its right to fall onto.
    #[test]
    fn closing_the_last_tab_falls_back_to_the_new_last_one() {
        // [a b C], close C -> [a b]: index 2 no longer exists.
        assert_eq!(active_after_close(2, 2, 2), 1);
        assert_eq!(active_after_close(0, 0, 1), 0);
    }

    /// Not reachable through `QueryTabs::close`, which refuses to empty itself
    /// — but the arithmetic must not underflow if it ever is.
    #[test]
    fn an_empty_strip_does_not_underflow() {
        assert_eq!(active_after_close(0, 0, 0), 0);
    }
}
