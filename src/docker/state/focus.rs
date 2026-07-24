//! Keyboard row-focus movement for the list pages (round 4).
//!
//! Arrow up/down move a highlighted row on every Docker list page. The rule is
//! plain data — "given the highlighted key and the ordered visible keys, what is
//! highlighted after this keypress" — so it lives here, keyed by the row's stable
//! id (a container id, an image id, a volume name…) and unit-tested without GPUI.
//! The view keeps the resulting key and paints that row highlighted; when the key
//! no longer appears (a filter hid it, a refresh removed it) movement simply
//! restarts from an end.

/// Which way an arrow key moves the highlight.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FocusMove {
    Up,
    Down,
}

/// The key that should be highlighted after a `dir` press, given the currently
/// highlighted `current` (if any) and the ordered `keys` visible on screen.
///
/// - Nothing highlighted yet: Down lands on the first row, Up on the last, so the
///   very first arrow press always selects a row.
/// - A highlighted row that is still visible: move one step, clamped at the ends
///   (no wrap — holding Down rests on the last row rather than jumping to the top).
/// - A highlighted row that has scrolled out of the visible set: treat it as
///   nothing highlighted and restart from the matching end.
/// - No rows at all: nothing to highlight.
pub fn next_focus(keys: &[String], current: Option<&str>, dir: FocusMove) -> Option<String> {
    if keys.is_empty() {
        return None;
    }
    let index = current.and_then(|key| keys.iter().position(|candidate| candidate == key));
    let next = match (index, dir) {
        (None, FocusMove::Down) => 0,
        (None, FocusMove::Up) => keys.len() - 1,
        (Some(i), FocusMove::Down) => (i + 1).min(keys.len() - 1),
        (Some(i), FocusMove::Up) => i.saturating_sub(1),
    };
    Some(keys[next].clone())
}

#[cfg(test)]
mod tests {
    use super::{FocusMove, next_focus};

    fn keys() -> Vec<String> {
        vec!["a".into(), "b".into(), "c".into()]
    }

    #[test]
    fn first_press_lands_on_an_end() {
        assert_eq!(next_focus(&keys(), None, FocusMove::Down).as_deref(), Some("a"));
        assert_eq!(next_focus(&keys(), None, FocusMove::Up).as_deref(), Some("c"));
    }

    #[test]
    fn movement_steps_and_clamps() {
        assert_eq!(
            next_focus(&keys(), Some("a"), FocusMove::Down).as_deref(),
            Some("b")
        );
        assert_eq!(
            next_focus(&keys(), Some("b"), FocusMove::Up).as_deref(),
            Some("a")
        );
        // Clamp at the bottom.
        assert_eq!(
            next_focus(&keys(), Some("c"), FocusMove::Down).as_deref(),
            Some("c")
        );
        // Clamp at the top.
        assert_eq!(
            next_focus(&keys(), Some("a"), FocusMove::Up).as_deref(),
            Some("a")
        );
    }

    #[test]
    fn a_stale_highlight_restarts_from_the_end() {
        // "z" is no longer visible: Down restarts at the first row.
        assert_eq!(
            next_focus(&keys(), Some("z"), FocusMove::Down).as_deref(),
            Some("a")
        );
        assert_eq!(
            next_focus(&keys(), Some("z"), FocusMove::Up).as_deref(),
            Some("c")
        );
    }

    #[test]
    fn no_rows_means_no_focus() {
        assert_eq!(next_focus(&[], Some("a"), FocusMove::Down), None);
        assert_eq!(next_focus(&[], None, FocusMove::Up), None);
    }
}
