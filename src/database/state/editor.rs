//! Which grammar the query editor is pointed at.
//!
//! One field and one method, and both exist because of a defect that shipped in
//! round 1: **the SQL editor drew every character in the foreground colour**,
//! flashing coloured for a single frame after Format and then going black again.
//! What follows is the diagnosis, because the fix is three lines and the reason
//! is the part worth keeping.
//!
//! # What the widget does
//!
//! `InputState`'s `CodeEditor` mode holds `highlighter: Rc<RefCell<Option<..>>>`,
//! and the element paints uncoloured text whenever that is `None`
//! (`input/element.rs` bails with `highlighter.as_mut()?`). Exactly three things
//! put a highlighter back:
//!
//! - an **edit** — `replace_text_in_range` calls `mode.update_highlighter(force =
//!   true)`, which builds one and parses;
//! - a render with `_pending_update` set — `InputState::render` calls
//!   `update_highlighter(force = false)` and then clears the flag;
//! - and nothing else.
//!
//! `set_highlighter` sets the language, drops the highlighter **and cancels the
//! in-flight parse task**, and — this is the whole defect — does *not* set
//! `_pending_update`. Its `cx.notify()` guarantees another frame; that frame
//! finds `_pending_update` false and paints black.
//!
//! # Trigger, mask, symptom
//!
//! - **Initiating trigger.** `DatabaseView::sync_editor` ran from `render` and
//!   called `set_highlighter` unconditionally — so every frame threw the
//!   highlighter away. (It also re-notified the input from inside a render,
//!   which kept the window redrawing.)
//! - **Masking condition.** The two paths above still build a highlighter, so
//!   one correctly coloured frame is produced now and then; the next
//!   `DatabaseView::render` takes it away again. That is why the colour was
//!   *visible* after Format — `replace_all` sets `_pending_update`, that frame
//!   colours the text, and the frame after it is black — and why typing showed
//!   no colour at all: the edit builds the highlighter *before* the frame, and
//!   the frame's `sync_editor` runs first.
//! - **Visible symptom.** Black text, with a flash of colour on Format.
//!
//! # The proven path it was compared against
//!
//! The API Explorer's script editors highlight correctly and are built with the
//! same `code_editor(..)` builder — they simply never call `set_highlighter`, so
//! the highlighter an edit builds survives. Its response viewer *does* call
//! `set_highlighter` at runtime (`api_explorer::state::tab`), and gets away with
//! it because the very next line is `set_value`, which sets `_pending_update`
//! through `reset_lsp_state`. The earliest meaningful divergence is therefore
//! not *that* `set_highlighter` is called but *when*: on a change there, on
//! every frame here — and unpaired.
//!
//! # The two halves of the fix
//!
//! [`EditorLanguage`] makes re-pointing **idempotent**, so a render-time caller
//! stops wiping the highlighter it just built. And when the language genuinely
//! does change, `set_highlighter` must be followed by `InputState::refresh`,
//! whose own doc says it exists for exactly this: "so the next render re-runs
//! syntax highlighting … not just a redraw". `set_highlighter` alone leaves the
//! editor uncoloured until the user's next keystroke.
//!
//! # What is testable here and what is not
//!
//! The idempotence below is testable without a `Window`, and it is the invariant
//! the defect violated. Whether the glyphs come out coloured is not: it needs a
//! rendered frame, a grammar and a theme. The tests under this module prove the
//! mechanism, not the pixels.

/// Which grammar the query editor has been pointed at, if any.
///
/// The point of the type is [`adopt`](Self::adopt): a caller that runs on every
/// frame must not re-point the editor on every frame.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct EditorLanguage {
    applied: Option<&'static str>,
}

impl EditorLanguage {
    pub fn new() -> Self {
        Self::default()
    }

    /// Records `wanted` and reports whether the editor has to be re-pointed at
    /// it.
    ///
    /// `true` the first time and after every genuine change; `false` for every
    /// repeat. A caller that ignores the `false` is the round-1 defect.
    pub fn adopt(&mut self, wanted: &'static str) -> bool {
        if self.applied == Some(wanted) {
            return false;
        }
        self.applied = Some(wanted);
        true
    }

    /// The grammar currently applied. Nothing in the view needs it — the view
    /// already knows what it asked for — so it exists for the assertions below.
    #[cfg(test)]
    pub fn applied(&self) -> Option<&'static str> {
        self.applied
    }
}

#[cfg(test)]
mod tests {
    use super::EditorLanguage;

    #[test]
    fn the_first_language_has_to_be_applied() {
        let mut language = EditorLanguage::new();
        assert_eq!(language.applied(), None);
        assert!(language.adopt("sql"));
        assert_eq!(language.applied(), Some("sql"));
    }

    /// The regression this type exists to stop. `set_highlighter` throws the
    /// highlighter away and schedules nothing to rebuild it, so a caller that
    /// runs per frame and re-points per frame can never paint a coloured frame.
    #[test]
    fn re_pointing_at_the_language_already_applied_is_not_a_change() {
        let mut language = EditorLanguage::new();
        assert!(language.adopt("sql"));
        for _ in 0..100 {
            assert!(
                !language.adopt("sql"),
                "a render-time caller must be told to do nothing"
            );
        }
    }

    /// A driver whose console is not SQL — the `Capabilities::editor_language`
    /// case — still re-points once, and then settles.
    #[test]
    fn a_genuine_change_is_applied_once_and_then_settles() {
        let mut language = EditorLanguage::new();
        assert!(language.adopt("sql"));
        assert!(language.adopt("text"));
        assert_eq!(language.applied(), Some("text"));
        assert!(!language.adopt("text"));
        assert!(language.adopt("sql"), "switching back is a change too");
    }
}
