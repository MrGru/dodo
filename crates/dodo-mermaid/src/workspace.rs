//! The workspace's own shape: which panes a mode shows, and what closing a tab
//! leaves behind.
//!
//! **No GPUI here**, the same rule as [`crate::render`] and for a reason this
//! module feels harder than most: a `MermaidTab` owns an `Entity<InputState>`,
//! so a test that closed a *real* tab would need a window — and
//! [`crate::view`]'s module doc records that a `#[gpui::test]` cannot be added
//! to this crate at all at the pinned `gpui` revision. Keeping the *decision*
//! here and leaving [`crate::view`] only the moving is therefore the sole
//! shape in which "closing the last tab leaves a blank one" can be asserted by
//! anything but a human with the app open.
//!
//! [`WorkspaceMode`] is here for a related reason. Three different surfaces
//! now depend on what a mode means — the editor pane, the preview pane, and
//! the floating controls that live inside each — and a control stranded in a
//! pane that is no longer drawn is invisible rather than loud. One predicate
//! per pane, asserted below, is what stops the three drifting apart.

use crate::i18n::mermaid;

/// The three ways to lay out the editor and the preview.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WorkspaceMode {
    Editor,
    Split,
    Preview,
}

impl WorkspaceMode {
    /// Left to right, which is also the order the mode toggle draws them and
    /// therefore the order its click indices are in.
    pub(crate) const ALL: [WorkspaceMode; 3] = [
        WorkspaceMode::Editor,
        WorkspaceMode::Split,
        WorkspaceMode::Preview,
    ];

    /// The mode's name — the toggle's tooltip since it went to icons, and its
    /// accessible label.
    pub(crate) fn label(self) -> mermaid::Text {
        match self {
            WorkspaceMode::Editor => mermaid::Text::ModeEditor,
            WorkspaceMode::Split => mermaid::Text::ModeSplit,
            WorkspaceMode::Preview => mermaid::Text::ModePreview,
        }
    }

    /// Whether the editor pane is drawn — and therefore whether the template
    /// button floating inside it exists this frame.
    pub(crate) fn shows_editor(self) -> bool {
        matches!(self, WorkspaceMode::Editor | WorkspaceMode::Split)
    }

    /// Whether the preview pane is drawn — and therefore whether the zoom
    /// cluster and the render status floating inside it exist this frame.
    pub(crate) fn shows_preview(self) -> bool {
        matches!(self, WorkspaceMode::Split | WorkspaceMode::Preview)
    }
}

/// What [`crate::view::MermaidView`] should do about the tab the user just
/// closed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CloseOutcome {
    /// The workspace had exactly one tab. Remove it and open a fresh blank one
    /// in its place.
    ///
    /// Not politeness — necessity. `MermaidView::render` returns early when
    /// `tabs[active]` is missing, so a workspace with no tabs has no editor to
    /// type into **and** no tab bar to add one from: the tool is dead until
    /// dodo restarts. That is the state this variant exists to make
    /// unreachable.
    ReplaceWithBlank,
    /// Remove it. The index carried here is the tab to make active
    /// *afterwards*, numbered against the already-shortened list.
    RemoveThenActivate(usize),
}

/// `closing` is an index into the `tab_count` tabs; the caller finds it by id,
/// so it is always in range.
pub(crate) fn close_outcome(tab_count: usize, active: usize, closing: usize) -> CloseOutcome {
    if tab_count <= 1 {
        return CloseOutcome::ReplaceWithBlank;
    }

    let last_remaining = tab_count - 2;
    let next = match active.cmp(&closing) {
        // A tab *after* the active one closed: the active tab has not moved.
        std::cmp::Ordering::Less => active,
        // A tab *before* it closed: everything from `closing` on shifts one
        // place left, the active tab included.
        std::cmp::Ordering::Greater => active - 1,
        // The active tab itself closed. Staying at the same index selects the
        // tab that followed it — the behaviour every tabbed editor has — and
        // the clamp covers the one case where there was no such tab.
        std::cmp::Ordering::Equal => active.min(last_remaining),
    };
    CloseOutcome::RemoveThenActivate(next)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_mode_shows_at_least_one_pane() {
        for mode in WorkspaceMode::ALL {
            assert!(
                mode.shows_editor() || mode.shows_preview(),
                "{mode:?} draws neither pane"
            );
        }
    }

    /// The three mode-dependent surfaces have to agree: the template button
    /// rides with the editor, the zoom cluster and the render status ride with
    /// the preview, and Split is the only mode carrying both.
    #[test]
    fn the_modes_mean_what_the_panes_mean() {
        assert!(WorkspaceMode::Editor.shows_editor());
        assert!(!WorkspaceMode::Editor.shows_preview());

        assert!(WorkspaceMode::Split.shows_editor());
        assert!(WorkspaceMode::Split.shows_preview());

        assert!(!WorkspaceMode::Preview.shows_editor());
        assert!(WorkspaceMode::Preview.shows_preview());
    }

    /// The toggle reports the index of the segment that was clicked, so the
    /// order here is load-bearing rather than cosmetic.
    #[test]
    fn the_modes_are_listed_in_the_order_the_toggle_draws_them() {
        assert_eq!(
            WorkspaceMode::ALL,
            [
                WorkspaceMode::Editor,
                WorkspaceMode::Split,
                WorkspaceMode::Preview
            ]
        );
    }

    #[test]
    fn closing_the_only_tab_asks_for_a_blank_one() {
        assert_eq!(close_outcome(1, 0, 0), CloseOutcome::ReplaceWithBlank);
    }

    #[test]
    fn closing_the_active_tab_activates_the_one_after_it() {
        // Three tabs, the middle one active and closed: index 1 now holds
        // what was index 2.
        assert_eq!(
            close_outcome(3, 1, 1),
            CloseOutcome::RemoveThenActivate(1),
            "the tab that followed the closed one should take over"
        );
    }

    #[test]
    fn closing_the_active_last_tab_activates_the_new_last_tab() {
        assert_eq!(close_outcome(3, 2, 2), CloseOutcome::RemoveThenActivate(1));
    }

    #[test]
    fn closing_a_tab_before_the_active_one_shifts_it_left() {
        assert_eq!(close_outcome(3, 2, 0), CloseOutcome::RemoveThenActivate(1));
        assert_eq!(close_outcome(3, 1, 0), CloseOutcome::RemoveThenActivate(0));
    }

    #[test]
    fn closing_a_tab_after_the_active_one_leaves_it_alone() {
        assert_eq!(close_outcome(3, 0, 1), CloseOutcome::RemoveThenActivate(0));
        assert_eq!(close_outcome(3, 0, 2), CloseOutcome::RemoveThenActivate(0));
    }

    /// The whole class of index mistake in one sweep: whatever was closed, the
    /// index handed back must address a tab that still exists, and it must
    /// still be *the same tab* unless the user closed the active one.
    #[test]
    fn the_same_tab_stays_active_unless_it_was_the_one_closed() {
        for tab_count in 2..=6usize {
            for active in 0..tab_count {
                for closing in 0..tab_count {
                    let CloseOutcome::RemoveThenActivate(next) =
                        close_outcome(tab_count, active, closing)
                    else {
                        panic!("{tab_count} tabs is more than one; expected a removal");
                    };

                    // Stand-in identities, so "the same tab" is a fact about
                    // contents rather than about arithmetic restated.
                    let mut ids: Vec<usize> = (0..tab_count).collect();
                    let was_active = ids[active];
                    ids.remove(closing);

                    assert!(
                        next < ids.len(),
                        "{tab_count} tabs, active {active}, closed {closing}: \
                         index {next} is out of a list of {}",
                        ids.len()
                    );
                    if closing != active {
                        assert_eq!(
                            ids[next], was_active,
                            "{tab_count} tabs, active {active}, closed {closing}: \
                             the active tab changed identity"
                        );
                    }
                }
            }
        }
    }
}
