//! [`CommandHistory`] — §30's undo stack, and the two different mechanisms that
//! together make a drag one step.
//!
//! # What was evaluated first, and why it is not used
//!
//! `gpui_component::history::History` is a real grouped undo stack with a
//! coalescing interval, it ships in a dependency dodo already builds, and the
//! phase brief asks for it to be considered before anything is written. It was.
//! Three things decided against it, in descending order of importance:
//!
//! 1. **It lives above this crate's central line.** `History` is in
//!    `gpui_component`, and the undo stack has to sit beside the stores, inside
//!    [`FlowEditor`](super::FlowEditor), or the view can reach the world
//!    without it — which is the exact bypass this phase exists to make
//!    unexpressible. Putting it there would put a UI-framework type in a module
//!    that `lib.rs`'s `the_pure_layers_name_no_ui_framework` test forbids, and
//!    that test is worth more than this file. Keeping the world's `&mut` in the
//!    *view* instead, next to a `History`, gives up the whole design.
//! 2. **Its grouping is wall-clock.** `inc_version` compares
//!    `last_changed_at.elapsed()` against a `group_interval`, so whether two
//!    edits are one undo step depends on how fast the machine was. A drag
//!    slower than the interval becomes several undo steps; two deliberate
//!    clicks faster than it become one. A gesture already has exact
//!    boundaries — [`InteractionEffect::BeginNodeDrag`](crate::interaction::InteractionEffect)
//!    and its `EndNodeDrag` — so the interval is a worse answer to a question
//!    that is already answered, and it is one no test can assert without
//!    sleeping.
//! 3. **`undo` is quadratic in the stack.** It re-scans the whole undo `Vec`
//!    per popped item to find the rest of the group. At its own default of
//!    1,000 entries that is a million comparisons for one keystroke. Phase 5
//!    lost a day to a quadratic that only showed up as a slow test; this one
//!    would show up as a slow keystroke.
//!
//! **What was taken from it is the good idea**: a group identifier stamped on
//! each entry, so that one undo pops a whole group. Here the identifier is a
//! [`GestureId`] handed out by [`CommandHistory::begin_gesture`] rather than a
//! version number derived from a clock, which makes the same behaviour
//! deterministic and testable in microseconds.
//!
//! # Two mechanisms, deliberately not one
//!
//! ```text
//! merging   two entries become one    keeps the stack small
//! grouping  several entries pop as one   keeps the undo step whole
//! ```
//!
//! A node drag emits a `MoveNodes` per mouse move — sixty a second — and
//! **merging** is what stops the stack growing by sixty entries a second:
//! consecutive moves of the same nodes fold into one by summing their deltas
//! ([`EditCommand::merge`]). But not every gesture emits mergeable commands; a
//! future multi-element transform might emit a move *and* a resize. **Grouping**
//! covers that: every entry made between `begin_gesture` and `end_gesture`
//! carries the same [`GestureId`], and one undo pops all the contiguous entries
//! that share it.
//!
//! Merging alone would leave such a gesture as several undo steps. Grouping
//! alone would leave the stack holding one entry per mouse move. Both, and the
//! drag is one entry *and* one step.
//!
//! **This file names no UI framework.**

use std::collections::VecDeque;

use crate::commands::edit::EditCommand;

/// Identifies one continuous gesture — a drag, a nudge repeat — so its entries
/// undo together.
///
/// Allocated by [`CommandHistory::begin_gesture`] and never reused, which is
/// what stops two drags of the same node from being mistaken for one when they
/// happen to produce identical deltas.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct GestureId(u64);

impl GestureId {
    pub fn raw(self) -> u64 {
        self.0
    }
}

impl std::fmt::Display for GestureId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "gesture#{}", self.0)
    }
}

/// One reversible step: the delta that made it and the delta that unmakes it.
#[derive(Debug, Clone, PartialEq)]
pub struct HistoryEntry {
    /// The delta to apply to move *forward* over this step.
    pub redo: EditCommand,
    /// The delta to apply to move *back* over it.
    pub undo: EditCommand,
    /// The gesture this entry belongs to, if it was made inside one.
    pub gesture: Option<GestureId>,
}

/// §30's undo/redo stacks.
///
/// Holds deltas and nothing else — no document, no element, no clone of a
/// store. The memory a history costs is proportional to what was edited, which
/// is the whole point of §30 asking for commands rather than snapshots.
#[derive(Debug, Clone)]
pub struct CommandHistory {
    /// A deque rather than a `Vec` so that dropping the oldest entry at the
    /// limit is O(1) instead of a memmove of the whole stack.
    undos: VecDeque<HistoryEntry>,
    redos: Vec<HistoryEntry>,
    limit: usize,
    gesture: Option<GestureId>,
    next_gesture: u64,
}

impl Default for CommandHistory {
    fn default() -> CommandHistory {
        CommandHistory::new()
    }
}

impl CommandHistory {
    /// How many undo steps are kept before the oldest is dropped.
    ///
    /// The same number `gpui_component::history::History` defaults to, and for
    /// the same reason: it is far past what anyone undoes through, and an
    /// unbounded stack on a canvas is a slow memory leak — a drag that is one
    /// *entry* is still one entry per drag, and a long editing session has
    /// thousands.
    pub const DEFAULT_LIMIT: usize = 1_000;

    pub fn new() -> CommandHistory {
        CommandHistory::with_limit(CommandHistory::DEFAULT_LIMIT)
    }

    pub fn with_limit(limit: usize) -> CommandHistory {
        CommandHistory {
            undos: VecDeque::new(),
            redos: Vec::new(),
            limit: limit.max(1),
            gesture: None,
            next_gesture: 0,
        }
    }

    pub fn can_undo(&self) -> bool {
        !self.undos.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redos.is_empty()
    }

    /// How many entries are on the undo stack. **Not the number of undo
    /// steps** — a gesture can hold several entries and pops as one — which is
    /// exactly the distinction the coalescing tests measure.
    pub fn undo_depth(&self) -> usize {
        self.undos.len()
    }

    pub fn redo_depth(&self) -> usize {
        self.redos.len()
    }

    pub fn open_gesture(&self) -> Option<GestureId> {
        self.gesture
    }

    /// Forgets everything. For a wholesale replacement of the document, where
    /// every index a stored delta names has stopped meaning what it meant.
    pub fn clear(&mut self) {
        self.undos.clear();
        self.redos.clear();
        self.gesture = None;
    }

    /// Opens a gesture. Entries recorded until [`end_gesture`](CommandHistory::end_gesture)
    /// undo together.
    ///
    /// Re-entrant by design: a second `begin_gesture` while one is open returns
    /// the open one rather than nesting. A press that begins a drag while
    /// another drag is somehow live is an input bug, and answering it with a
    /// nested gesture would turn it into an undo bug as well.
    pub fn begin_gesture(&mut self) -> GestureId {
        if let Some(open) = self.gesture {
            return open;
        }

        let id = GestureId(self.next_gesture);
        self.next_gesture += 1;
        self.gesture = Some(id);
        id
    }

    pub fn end_gesture(&mut self) {
        self.gesture = None;
    }

    /// **Records one step**, merging it into the entry below when it can.
    ///
    /// Merging is only attempted inside a gesture. Two separate edits that
    /// happen to be mergeable — a nudge, then another nudge a minute later —
    /// are two steps, because a person who paused between them expects two.
    pub fn push(&mut self, redo: EditCommand, undo: EditCommand) {
        // Any new edit invalidates the redo branch: the deltas on it were
        // recorded against a state that no longer exists.
        self.redos.clear();

        if let Some(gesture) = self.gesture
            && let Some(top) = self.undos.back_mut()
            && top.gesture == Some(gesture)
            && top.redo.merge(&redo)
        {
            // The undo side folds the other way round — undoing A then B is
            // `undo B` then `undo A` — and `EditCommand::merge` sums, so
            // folding the new undo into the old one gives the same answer as
            // folding them in order. `edit.rs` has that as a test.
            let merged = top.undo.merge(&undo);
            debug_assert!(
                merged,
                "the redo sides merged but the undo sides did not, which would \
                 leave this entry undoing less than it redoes"
            );
            return;
        }

        if self.undos.len() >= self.limit {
            self.undos.pop_front();
        }
        self.undos.push_back(HistoryEntry {
            redo,
            undo,
            gesture: self.gesture,
        });
    }

    /// **Takes one undo step**: the top entry, plus every contiguous entry
    /// below it belonging to the same gesture.
    ///
    /// The entries come back newest-first, which is the order they have to be
    /// applied in. The caller applies each `undo` and hands back the inverse it
    /// got, through [`record_undone`](CommandHistory::record_undone) — the
    /// history never touches a world.
    pub fn take_undo(&mut self) -> Vec<HistoryEntry> {
        let Some(top) = self.undos.pop_back() else {
            return Vec::new();
        };

        let gesture = top.gesture;
        let mut step = vec![top];
        if gesture.is_some() {
            // Contiguous only, and a bounded walk: it stops at the first entry
            // that is not part of this gesture rather than scanning the stack.
            while self
                .undos
                .back()
                .is_some_and(|entry| entry.gesture == gesture)
            {
                step.push(self.undos.pop_back().expect("just peeked"));
            }
        }
        step
    }

    /// The same, for redo. Entries come back in the order they must be applied.
    pub fn take_redo(&mut self) -> Vec<HistoryEntry> {
        let Some(top) = self.redos.pop() else {
            return Vec::new();
        };

        let gesture = top.gesture;
        let mut step = vec![top];
        if gesture.is_some() {
            while self
                .redos
                .last()
                .is_some_and(|entry| entry.gesture == gesture)
            {
                step.push(self.redos.pop().expect("just peeked"));
            }
        }
        step
    }

    /// Files an entry the caller has just undone onto the redo stack.
    ///
    /// Pushed in the order they were applied, so that
    /// [`take_redo`](CommandHistory::take_redo) pops them back out oldest-first
    /// and the world walks forward through the gesture the way it originally
    /// went.
    pub fn record_undone(&mut self, entry: HistoryEntry) {
        self.redos.push(entry);
    }

    /// Files an entry the caller has just redone back onto the undo stack.
    pub fn record_redone(&mut self, entry: HistoryEntry) {
        self.undos.push_back(entry);
    }
}

#[cfg(test)]
mod tests {
    use super::CommandHistory;
    use crate::{commands::edit::EditCommand, geometry::Vec2, models::NodeIndex};

    fn move_of(node: u32, dx: f32) -> (EditCommand, EditCommand) {
        let node = NodeIndex::new(node);
        (
            EditCommand::move_node(node, Vec2::new(dx, 0.0)),
            EditCommand::move_node(node, Vec2::new(-dx, 0.0)),
        )
    }

    /// **§30's coalescing.** A drag emits one command per mouse move; the stack
    /// must not grow by one entry per move, and undoing must put the node back
    /// where the drag started rather than one mouse-move back.
    #[test]
    fn a_whole_drag_is_one_entry_and_one_step() {
        let mut history = CommandHistory::new();
        history.begin_gesture();
        for _ in 0..60 {
            let (redo, undo) = move_of(0, 1.0);
            history.push(redo, undo);
        }
        history.end_gesture();

        assert_eq!(history.undo_depth(), 1, "the drag grew the stack per move");

        let step = history.take_undo();
        assert_eq!(step.len(), 1);
        assert_eq!(
            step[0].undo,
            EditCommand::move_node(NodeIndex::new(0), Vec2::new(-60.0, 0.0)),
            "the coalesced undo does not cover the whole drag"
        );
        assert_eq!(
            step[0].redo,
            EditCommand::move_node(NodeIndex::new(0), Vec2::new(60.0, 0.0))
        );
    }

    /// The mechanism merging cannot cover: a gesture whose commands are not
    /// mergeable stays several entries, and still undoes as one step.
    #[test]
    fn a_gesture_of_unmergeable_edits_is_several_entries_and_still_one_step() {
        let mut history = CommandHistory::new();
        history.begin_gesture();
        let (redo, undo) = move_of(0, 1.0);
        history.push(redo, undo);
        history.push(
            EditCommand::resize_node(NodeIndex::new(0), Vec2::new(5.0, 5.0)),
            EditCommand::resize_node(NodeIndex::new(0), Vec2::new(4.0, 4.0)),
        );
        history.end_gesture();

        assert_eq!(history.undo_depth(), 2);
        assert_eq!(
            history.take_undo().len(),
            2,
            "the gesture split into two steps"
        );
        assert_eq!(history.undo_depth(), 0);
    }

    /// Two drags of the same node produce identical deltas; only the gesture id
    /// keeps them apart, and it must.
    #[test]
    fn two_gestures_never_merge_into_each_other() {
        let mut history = CommandHistory::new();
        for _ in 0..2 {
            history.begin_gesture();
            let (redo, undo) = move_of(0, 1.0);
            history.push(redo, undo);
            history.end_gesture();
        }

        assert_eq!(history.undo_depth(), 2);
        assert_eq!(history.take_undo().len(), 1);
        assert_eq!(history.take_undo().len(), 1);
    }

    /// Outside a gesture nothing merges, however mergeable it looks — two
    /// keyboard nudges a minute apart are two undo steps.
    #[test]
    fn edits_outside_a_gesture_never_merge() {
        let mut history = CommandHistory::new();
        for _ in 0..3 {
            let (redo, undo) = move_of(0, 1.0);
            history.push(redo, undo);
        }

        assert_eq!(history.undo_depth(), 3);
    }

    /// A second `begin_gesture` while one is open must not nest: the second
    /// `end_gesture` would otherwise leave a gesture open forever and every
    /// later edit would join it.
    #[test]
    fn beginning_a_gesture_twice_returns_the_open_one() {
        let mut history = CommandHistory::new();
        let first = history.begin_gesture();
        assert_eq!(history.begin_gesture(), first);
        history.end_gesture();
        assert_eq!(history.open_gesture(), None);
        assert_ne!(history.begin_gesture(), first, "a gesture id was reused");
    }

    /// A new edit after an undo must drop the redo branch: its deltas were
    /// recorded against a state that no longer exists, and applying one would
    /// move an element by an amount nobody asked for.
    #[test]
    fn a_new_edit_discards_the_redo_branch() {
        let mut history = CommandHistory::new();
        let (redo, undo) = move_of(0, 1.0);
        history.push(redo, undo);

        for entry in history.take_undo() {
            history.record_undone(entry);
        }
        assert!(history.can_redo());

        let (redo, undo) = move_of(1, 5.0);
        history.push(redo, undo);
        assert!(!history.can_redo());
    }

    /// The limit is a memory bound rather than a suggestion, and it drops the
    /// *oldest* entry — undoing the last thing you did must always work.
    #[test]
    fn the_stack_is_bounded_and_forgets_from_the_far_end() {
        let mut history = CommandHistory::with_limit(4);
        for step in 0..10 {
            let (redo, undo) = move_of(0, step as f32 + 1.0);
            history.push(redo, undo);
        }

        assert_eq!(history.undo_depth(), 4);
        assert_eq!(
            history.take_undo()[0].undo,
            EditCommand::move_node(NodeIndex::new(0), Vec2::new(-10.0, 0.0)),
            "the newest entry was the one dropped"
        );
    }

    #[test]
    fn an_empty_history_undoes_and_redoes_nothing() {
        let mut history = CommandHistory::new();
        assert!(!history.can_undo() && !history.can_redo());
        assert!(history.take_undo().is_empty());
        assert!(history.take_redo().is_empty());
    }
}
