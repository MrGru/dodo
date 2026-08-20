//! **Layer ordering**: the four buttons on the property panel's Layers row,
//! and the arithmetic behind them.
//!
//! # Why the arithmetic is here rather than in the editor
//!
//! "Send backward" is one sentence and four decisions: what the selection moves
//! *relative to*, what happens to two selected elements' order between
//! themselves, what happens when nothing is above, and what happens at the ends
//! of an `i32`. Every one of them is observable and none of them needs a world,
//! a document or a window — so they are a pure function over two numbers and a
//! [`DepthSpan`], and [`FlowEditor`](crate::commands::FlowEditor) is left with
//! the part that genuinely needs the world: reading the depths and applying the
//! command.
//!
//! # The four decisions, stated
//!
//! 1. **The selection moves as a block.** Every selected element shifts by the
//!    same delta, so two shapes sent to the front keep the order they had
//!    between themselves. Assigning each a rank instead would silently
//!    reorder a selection every time it was raised.
//! 2. **A step passes whatever is next, not one unit — and "next" includes the
//!    selection's own depth.** "Bring forward" lands the selection's top
//!    element one above the nearest depth **at or above** it, so it passes
//!    everything sitting there. The *at* half is the one that is easy to get
//!    wrong and it is the common case: two shapes drawn one after the other
//!    both sit at depth 0, ordered only by which was created first, and a rule
//!    that looked for a depth *strictly* above would find none and refuse to
//!    move either of them. One press separates them; a second finds nothing
//!    above and correctly does nothing.
//! 3. **Nothing above means nothing happens.** The delta is zero, the command
//!    is empty, the applier records no change and no undo step is consumed. A
//!    front-most element pressed "bring to front" ten times is still one press
//!    of undo away from wherever it started.
//! 4. **The ends of an `i32` saturate.** A document would need two billion
//!    presses to reach one, and a wrap would put the front element behind
//!    everything — the one arithmetic failure here that is invisible until it
//!    is catastrophic.
//!
//! # And the decision that is *not* here
//!
//! Depths are left sparse. Nothing renumbers the document to close the gaps a
//! sequence of these opens, because renumbering is an edit to every element —
//! an undo entry proportional to the document for a press that moved one shape.
//! `i32` has room for a very long session, and the paint order only ever asks
//! for the *comparison*.
//!
//! **This file names no UI framework.**

/// One of the four Layers buttons.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LayerAction {
    /// Beneath everything.
    SendToBack,
    /// One step down.
    SendBackward,
    /// One step up.
    BringForward,
    /// Above everything.
    BringToFront,
}

impl LayerAction {
    /// The four, in the order the panel draws them — back, backward, forward,
    /// front, left to right, which is the order the captain's screenshots fix.
    pub const ALL: &'static [LayerAction] = &[
        LayerAction::SendToBack,
        LayerAction::SendBackward,
        LayerAction::BringForward,
        LayerAction::BringToFront,
    ];

    /// A short, stable name, for element ids and tests. **Not user-facing.**
    pub const fn name(self) -> &'static str {
        match self {
            LayerAction::SendToBack => "send-to-back",
            LayerAction::SendBackward => "send-backward",
            LayerAction::BringForward => "bring-forward",
            LayerAction::BringToFront => "bring-to-front",
        }
    }

    /// **What every selected element's depth shifts by.** Zero means the
    /// selection is already where this button would put it.
    ///
    /// See the module doc for the four decisions this encodes.
    pub fn shift(self, selection: DepthSpan, others: DepthSpan) -> i32 {
        let (Some(selection_min), Some(selection_max)) = (selection.min, selection.max) else {
            // Nothing is selected, so nothing moves.
            return 0;
        };

        let target = match self {
            LayerAction::BringToFront => others.max.map(|max| max.saturating_add(1)),
            LayerAction::SendToBack => others.min.map(|min| min.saturating_sub(1)),
            LayerAction::BringForward => others
                .nearest_at_or_above()
                .map(|above| above.saturating_add(1)),
            LayerAction::SendBackward => others
                .nearest_at_or_below()
                .map(|below| below.saturating_sub(1)),
        };

        let Some(target) = target else {
            // The selection is the whole document, or there is nothing between
            // it and the direction it was asked to move in.
            return 0;
        };

        let shift = match self {
            LayerAction::BringToFront | LayerAction::BringForward => {
                target.saturating_sub(selection_max)
            }
            LayerAction::SendToBack | LayerAction::SendBackward => {
                target.saturating_sub(selection_min)
            }
        };

        // A "bring" that would move the selection *down* is a selection that is
        // already in front; the same in reverse. Refusing rather than clamping
        // to zero would be the same answer, but this way the intent is written
        // down where the next reader is.
        match self {
            LayerAction::BringToFront | LayerAction::BringForward => shift.max(0),
            LayerAction::SendToBack | LayerAction::SendBackward => shift.min(0),
        }
    }
}

/// The depths a set of elements occupies, accumulated in one pass.
///
/// Four numbers rather than a sorted list of every depth in the document: the
/// four questions the buttons ask are the extremes and the nearest neighbour on
/// each side of an interval, and all four fold. That is what lets one walk of
/// the live elements answer a press, rather than a walk plus an allocation plus
/// a sort — see [`FlowEditor::reorder_selection`](crate::commands::FlowEditor::reorder_selection).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DepthSpan {
    /// The lowest depth seen, or `None` if nothing was.
    pub min: Option<i32>,
    /// The highest depth seen.
    pub max: Option<i32>,
    /// The largest depth at or below the bottom of the reference interval.
    below: Option<i32>,
    /// The smallest depth at or above the top of it.
    above: Option<i32>,
}

impl DepthSpan {
    pub const EMPTY: DepthSpan = DepthSpan {
        min: None,
        max: None,
        below: None,
        above: None,
    };

    /// Folds one element's depth in, against the interval `low..=high` the
    /// neighbour questions are asked about.
    ///
    /// The interval is the *selection's*, and it is passed at every step rather
    /// than stored, so one `DepthSpan` can be accumulated for the selection
    /// (with an interval nobody reads) and one for everything else in the same
    /// walk.
    pub fn observe(&mut self, z: i32, low: i32, high: i32) {
        self.min = Some(self.min.map_or(z, |min| min.min(z)));
        self.max = Some(self.max.map_or(z, |max| max.max(z)));

        // `<=` and `>=` rather than `<` and `>`: see decision 2 in the module
        // doc. An element sharing the selection's depth *is* the thing a step
        // has to get past.
        if z <= low {
            self.below = Some(self.below.map_or(z, |below| below.max(z)));
        }
        if z >= high {
            self.above = Some(self.above.map_or(z, |above| above.min(z)));
        }
    }

    /// The nearest depth at or above the top of the interval
    /// [`observe`](DepthSpan::observe) was given — what "bring forward" has to
    /// get past.
    pub fn nearest_at_or_above(&self) -> Option<i32> {
        self.above
    }

    /// The nearest depth at or below the bottom of it.
    pub fn nearest_at_or_below(&self) -> Option<i32> {
        self.below
    }

    pub fn is_empty(&self) -> bool {
        self.min.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::{DepthSpan, LayerAction};

    /// Builds the two spans a press is decided from: the selection's depths,
    /// and everything else's measured against the selection's interval.
    fn spans(selected: &[i32], others: &[i32]) -> (DepthSpan, DepthSpan) {
        let low = selected.iter().copied().min().unwrap_or(0);
        let high = selected.iter().copied().max().unwrap_or(0);

        let mut selection = DepthSpan::EMPTY;
        for &z in selected {
            selection.observe(z, low, high);
        }

        let mut rest = DepthSpan::EMPTY;
        for &z in others {
            rest.observe(z, low, high);
        }

        (selection, rest)
    }

    fn shift(action: LayerAction, selected: &[i32], others: &[i32]) -> i32 {
        let (selection, rest) = spans(selected, others);
        action.shift(selection, rest)
    }

    #[test]
    fn bringing_to_front_lands_one_above_the_highest() {
        assert_eq!(shift(LayerAction::BringToFront, &[0], &[0, 3, 7]), 8);
    }

    #[test]
    fn sending_to_back_lands_one_below_the_lowest() {
        assert_eq!(shift(LayerAction::SendToBack, &[5], &[0, 3, 7]), -6);
    }

    /// Decision 2: a step passes the whole depth it lands on, so two elements
    /// sharing a depth separate on the first press rather than swapping for
    /// ever.
    #[test]
    fn one_step_passes_everything_at_the_next_depth() {
        assert_eq!(shift(LayerAction::BringForward, &[0], &[0, 0, 0]), 1);
        assert_eq!(shift(LayerAction::SendBackward, &[0], &[0, 0, 0]), -1);
    }

    #[test]
    fn a_step_only_passes_the_nearest_neighbour() {
        // Above 0 sit 2 and 9. Forward means one above 2, not one above 9.
        assert_eq!(shift(LayerAction::BringForward, &[0], &[2, 9]), 3);
        // Below 5 sit 2 and -9. Backward lands at 1, just under 2.
        assert_eq!(shift(LayerAction::SendBackward, &[5], &[2, -9]), -4);
    }

    /// The second press of "forward" on a pair that has already separated finds
    /// nothing at or above it and stops, rather than climbing for ever.
    #[test]
    fn a_step_stops_once_there_is_nothing_left_to_pass() {
        assert_eq!(shift(LayerAction::BringForward, &[1], &[0]), 0);
        assert_eq!(shift(LayerAction::SendBackward, &[-1], &[0]), 0);
    }

    /// Decision 3, and the reason it matters: a press that changes nothing must
    /// produce an empty command, so it does not consume an undo.
    #[test]
    fn a_press_with_nowhere_to_go_shifts_nothing() {
        assert_eq!(shift(LayerAction::BringForward, &[9], &[0, 3]), 0);
        assert_eq!(shift(LayerAction::SendBackward, &[-9], &[0, 3]), 0);
        assert_eq!(shift(LayerAction::BringToFront, &[9], &[0, 3]), 0);
        assert_eq!(shift(LayerAction::SendToBack, &[-9], &[0, 3]), 0);
    }

    #[test]
    fn nothing_selected_and_nothing_else_both_shift_nothing() {
        assert_eq!(shift(LayerAction::BringToFront, &[], &[1, 2]), 0);
        assert_eq!(shift(LayerAction::BringToFront, &[1], &[]), 0);
    }

    /// Decision 1: the block keeps its own order. The shift is one number, so
    /// this is really an assertion that the *whole selection* is measured
    /// rather than each element.
    #[test]
    fn a_multiple_selection_moves_as_one_block() {
        // Two selected at 1 and 4, one other at 10. Front puts the top at 11,
        // so both shift by 7 and stay three apart.
        assert_eq!(shift(LayerAction::BringToFront, &[1, 4], &[10]), 7);
        // Back puts the bottom at -1, so both shift by -2.
        assert_eq!(shift(LayerAction::SendToBack, &[1, 4], &[0]), -2);
    }

    /// Decision 4. A wrap here would put the front element behind everything,
    /// which is the failure that is invisible until it is total.
    #[test]
    fn the_ends_of_an_i32_saturate_rather_than_wrap() {
        assert_eq!(
            shift(LayerAction::BringToFront, &[0], &[i32::MAX]),
            i32::MAX
        );
        assert_eq!(shift(LayerAction::SendToBack, &[0], &[i32::MIN]), i32::MIN);
    }

    /// The names are element ids and test labels, so two buttons sharing one is
    /// a GPUI state collision rather than a cosmetic clash.
    #[test]
    fn every_button_has_its_own_name() {
        let names: Vec<&str> = LayerAction::ALL.iter().map(|it| it.name()).collect();
        assert_eq!(names.len(), 4);
        for (index, name) in names.iter().enumerate() {
            assert!(!names[index + 1..].contains(name), "two buttons are {name}");
        }
    }
}
