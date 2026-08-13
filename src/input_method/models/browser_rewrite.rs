//! What a browser needs doing to a delete/insert plan before it is posted.
//!
//! A direct-output host rewrites the current syllable as *n* Backspaces
//! followed by one Unicode insert. That is correct wherever the caret is a
//! caret. It is wrong in a browser **address bar**, because inline autocomplete
//! keeps a selection alive after every keystroke: the first Backspace deletes
//! the selection rather than the character the engine meant, and every
//! character after it is off by one. Textareas and ordinary in-page inputs have
//! no such selection and are already correct — nothing here may change what
//! reaches them beyond the two documented strategies.
//!
//! # Two strategies, because Blink and WebKit do not agree
//!
//! Chromium-family browsers answer a `Shift`+`Left` before the Backspaces:
//! whether or not a suggestion is selected, the browser ends up with exactly
//! one selected character that the first Backspace — or the inserted string —
//! consumes. Safari and Firefox do not; WebKit's selection anchoring makes the
//! same trick land somewhere else. They are given an invisible character
//! instead, which forces the browser to *commit* and dismiss the suggestion,
//! and then one extra Backspace to remove it again.
//!
//! # What this module cannot see, and what that costs
//!
//! **Focus.** A bundle identifier says which application is frontmost; it
//! cannot say whether the caret is in the address bar or in a page input, and
//! macOS offers a direct-output host no cheap way to ask. So the workaround
//! runs for the whole application. For [`Strategy::ExtendSelection`] that is
//! free — the arithmetic is identical with and without a selection. For
//! [`Strategy::CommitSuggestion`] it means every tone mark typed into an
//! ordinary in-page input also costs one invisible insert and one extra
//! Backspace, which a page can observe as extra DOM `input` events. The final
//! text is unchanged. That trade is accepted for now, and the invisible
//! character is emitted from exactly one place
//! ([`BrowserRewrite::commit_character`]) so that a future focus test can
//! narrow it without touching anything else.
//!
//! **Start of field.** Nothing here can prove the caret is at the start of the
//! text field, and no CoreGraphics API offers it without an Accessibility query
//! per keystroke. The proxy is the plan itself: `delete_before > 0` means the
//! engine believes it rendered at least that many graphemes immediately before
//! the caret, and the composer forgets that belief on a mouse-down, an arrow
//! key, a focus change or a target-process change. The residual risk is a caret
//! moved by something the tap cannot observe — and in that case the *existing*
//! Backspace rewrite is already deleting the wrong characters, so neither
//! strategy is what broke it.

use crate::input_method::models::direct_output::OutputPlan;

/// The invisible character [`Strategy::CommitSuggestion`] types to make a
/// browser commit and dismiss its inline suggestion.
///
/// `U+200B ZERO WIDTH SPACE`. Deliberately a single named constant: it is the
/// one thing here most likely to need swapping during real-browser testing
/// (`U+2060 WORD JOINER` and `U+FEFF` are the obvious alternates), and swapping
/// it must not mean hunting through event-staging code.
pub const SELECTION_COMMIT_CHARACTER: &str = "\u{200b}";

/// How a browser's inline autocomplete selection is cleared, if at all.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Strategy {
    /// No workaround: the plan is posted exactly as the engine described it.
    #[default]
    None,
    /// Blink: one `Shift`+`Left` before the Backspaces, and a Backspace count
    /// adjusted for the character it selects.
    ExtendSelection,
    /// WebKit and Gecko: one invisible character before the Backspaces, and one
    /// extra Backspace to take it away again.
    CommitSuggestion,
}

/// **The one place to extend.** Every browser dodo works around, beside the
/// strategy its engine needs.
///
/// A new browser is one row. Getting the row wrong is not dangerous — the worst
/// case is the strategy that does not help — but putting an application that is
/// not a browser here is, because [`Strategy::CommitSuggestion`] types a
/// character into it.
const BROWSERS: [(&str, Strategy); 13] = [
    // Blink.
    ("com.google.Chrome", Strategy::ExtendSelection),
    ("com.google.Chrome.canary", Strategy::ExtendSelection),
    ("org.chromium.Chromium", Strategy::ExtendSelection),
    ("com.brave.Browser", Strategy::ExtendSelection),
    ("com.microsoft.edgemac", Strategy::ExtendSelection),
    ("com.vivaldi.Vivaldi", Strategy::ExtendSelection),
    ("com.operasoftware.Opera", Strategy::ExtendSelection),
    // Arc.
    ("company.thebrowser.Browser", Strategy::ExtendSelection),
    ("com.coccoc.Coccoc", Strategy::ExtendSelection),
    // WebKit.
    ("com.apple.Safari", Strategy::CommitSuggestion),
    (
        "com.apple.SafariTechnologyPreview",
        Strategy::CommitSuggestion,
    ),
    // Gecko.
    ("org.mozilla.firefox", Strategy::CommitSuggestion),
    (
        "org.mozilla.firefoxdeveloperedition",
        Strategy::CommitSuggestion,
    ),
];

impl Strategy {
    /// The strategy for a frontmost application.
    ///
    /// An application that is not in [`BROWSERS`] gets [`Strategy::None`], and
    /// that is the deliberate reading of "everything else". The alternative —
    /// treating every unrecognised application as WebKit — would type an
    /// invisible character into every text field on the system to fix a problem
    /// only browsers have. Widening it is one line here if a browser turns up
    /// that nobody listed.
    pub fn for_bundle_id(bundle_id: &str) -> Strategy {
        BROWSERS
            .into_iter()
            .find_map(|(id, strategy)| (id == bundle_id).then_some(strategy))
            .unwrap_or(Strategy::None)
    }
}

/// One plan's delete/insert sequence, adjusted for the frontmost application.
///
/// This replaces [`OutputPlan::delete_before`] at staging time; the plan's
/// `insert` is untouched, because what the user typed must never depend on
/// which browser they typed it in.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BrowserRewrite {
    /// Post a full `Shift`+`Left` key-down/key-up pair before the Backspaces.
    pub extend_selection: bool,
    /// Type this before the Backspaces. Always [`SELECTION_COMMIT_CHARACTER`]
    /// when set; it is an `Option<&str>` rather than a `bool` so the staging
    /// code never names the character itself.
    pub commit_character: Option<&'static str>,
    /// How many Backspace pairs to post, in place of the plan's own count.
    pub delete_before: usize,
}

impl BrowserRewrite {
    /// The plan posted verbatim: no extra events, the engine's own count.
    pub fn verbatim(plan: &OutputPlan) -> BrowserRewrite {
        BrowserRewrite {
            extend_selection: false,
            commit_character: None,
            delete_before: plan.delete_before,
        }
    }

    /// The adjusted sequence for one plan in one application.
    ///
    /// Every guard answers [`verbatim`](Self::verbatim), which is exactly what
    /// this host did before the workaround existed. That is the fail-safe
    /// direction: a skipped workaround leaves an address bar wrong, while a
    /// workaround applied where it does not belong destroys text the user
    /// typed.
    pub fn plan(enabled: bool, bundle_id: Option<&str>, plan: &OutputPlan) -> BrowserRewrite {
        let verbatim = BrowserRewrite::verbatim(plan);
        if !enabled {
            return verbatim;
        }
        // Nothing to delete: a pure insertion cannot be off by one, and
        // `Shift`+`Left` at a start-of-field caret would select a character
        // belonging to whatever precedes it.
        if plan.delete_before == 0 {
            return verbatim;
        }
        // The engine is also letting the original key through, so the synthetic
        // sequence is not the whole edit and the selection arithmetic cannot be
        // reasoned about. `OutputPlan` has no separate "do not touch preceding
        // text" flag; `pass_through` is the nearest and only signal it carries.
        if plan.pass_through {
            return verbatim;
        }
        // `ExtendSelection` collapses a single Backspace to none *because the
        // inserted string overwrites the selection*. With nothing to insert
        // there would be nothing to consume it, leaving a selected character
        // neither deleted nor replaced.
        if plan.insert.is_none() {
            return verbatim;
        }

        match bundle_id.map_or(Strategy::None, Strategy::for_bundle_id) {
            Strategy::None => verbatim,
            Strategy::ExtendSelection => BrowserRewrite {
                extend_selection: true,
                // One Backspace becomes none: the selection `Shift`+`Left` left
                // behind is what the inserted string overwrites. More than one
                // is unchanged: the first Backspace consumes that selection,
                // which is exactly the one real character it would have deleted
                // anyway.
                delete_before: if plan.delete_before == 1 {
                    0
                } else {
                    plan.delete_before
                },
                ..verbatim
            },
            Strategy::CommitSuggestion => BrowserRewrite {
                commit_character: Some(SELECTION_COMMIT_CHARACTER),
                // One more, to take the invisible character away again.
                delete_before: plan.delete_before.saturating_add(1),
                ..verbatim
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{BROWSERS, BrowserRewrite, SELECTION_COMMIT_CHARACTER, Strategy};
    use crate::input_method::models::direct_output::OutputPlan;

    /// A tone mark: replace the last *n* graphemes with a composed string.
    fn tone(delete_before: usize) -> OutputPlan {
        OutputPlan {
            delete_before,
            insert: Some("ế".into()),
            pass_through: false,
        }
    }

    const CHROME: &str = "com.google.Chrome";
    const SAFARI: &str = "com.apple.Safari";

    /// The boundary the whole fix turns on. One Backspace becomes none because
    /// the insert overwrites what `Shift`+`Left` selected; two or more are left
    /// alone because the first one consumes that selection itself.
    #[test]
    fn extending_the_selection_collapses_one_backspace_and_leaves_every_other_count_alone() {
        let one = BrowserRewrite::plan(true, Some(CHROME), &tone(1));
        assert!(one.extend_selection);
        assert_eq!(one.commit_character, None);
        assert_eq!(one.delete_before, 0);

        for count in [2, 3, 5, 32] {
            let many = BrowserRewrite::plan(true, Some(CHROME), &tone(count));
            assert!(many.extend_selection, "{count}");
            assert_eq!(many.commit_character, None, "{count}");
            assert_eq!(many.delete_before, count, "{count}");
        }
    }

    /// The other strategy is unconditional arithmetic: one invisible character
    /// in, one extra Backspace out, at every count.
    #[test]
    fn committing_the_suggestion_adds_one_character_and_one_backspace_at_every_count() {
        for count in [1, 2, 3, 5, 32] {
            let rewrite = BrowserRewrite::plan(true, Some(SAFARI), &tone(count));
            assert!(!rewrite.extend_selection, "{count}");
            assert_eq!(
                rewrite.commit_character,
                Some(SELECTION_COMMIT_CHARACTER),
                "{count}"
            );
            assert_eq!(rewrite.delete_before, count + 1, "{count}");
        }
    }

    /// The invisible character has to stay invisible and stay one scalar: a
    /// visible or multi-scalar replacement would be typed into every page
    /// input, and the extra Backspace only removes one grapheme.
    #[test]
    fn the_commit_character_is_a_single_invisible_scalar() {
        let mut characters = SELECTION_COMMIT_CHARACTER.chars();
        let character = characters.next().expect("one scalar");
        assert_eq!(characters.next(), None);
        assert!(!character.is_control());
        assert!(!character.is_alphanumeric());
        assert!(!character.is_ascii_graphic());
        assert_eq!(SELECTION_COMMIT_CHARACTER, "\u{200b}");
        assert_eq!(
            dodo_ime_core::core::grapheme_count(SELECTION_COMMIT_CHARACTER),
            1
        );
    }

    #[test]
    fn every_listed_bundle_id_routes_to_the_strategy_its_engine_needs() {
        for id in [
            "com.google.Chrome",
            "com.google.Chrome.canary",
            "org.chromium.Chromium",
            "com.brave.Browser",
            "com.microsoft.edgemac",
            "com.vivaldi.Vivaldi",
            "com.operasoftware.Opera",
            "company.thebrowser.Browser",
            "com.coccoc.Coccoc",
        ] {
            assert_eq!(
                Strategy::for_bundle_id(id),
                Strategy::ExtendSelection,
                "{id}"
            );
        }
        for id in [
            "com.apple.Safari",
            "com.apple.SafariTechnologyPreview",
            "org.mozilla.firefox",
            "org.mozilla.firefoxdeveloperedition",
        ] {
            assert_eq!(
                Strategy::for_bundle_id(id),
                Strategy::CommitSuggestion,
                "{id}"
            );
        }
    }

    /// The table is the whole routing rule, so it must not gain a duplicate or
    /// an empty row that silently shadows a later one.
    #[test]
    fn the_browser_table_has_no_duplicate_or_empty_identifier() {
        for (index, (id, _)) in BROWSERS.into_iter().enumerate() {
            assert!(!id.is_empty());
            assert!(
                !BROWSERS[..index].iter().any(|(earlier, _)| *earlier == id),
                "{id} is listed twice"
            );
        }
    }

    /// An application dodo has never heard of is left exactly as it is today.
    /// This is the conservative reading of "everything else" — see
    /// [`Strategy::for_bundle_id`].
    #[test]
    fn an_unknown_or_absent_bundle_id_changes_nothing() {
        for bundle_id in [
            Some("com.apple.TextEdit"),
            Some("com.apple.Terminal"),
            Some("com.googl.Chrome"),
            Some("com.google.Chrome "),
            Some(""),
            None,
        ] {
            let rewrite = BrowserRewrite::plan(true, bundle_id, &tone(3));
            assert_eq!(rewrite, BrowserRewrite::verbatim(&tone(3)), "{bundle_id:?}");
            assert!(!rewrite.extend_selection);
            assert_eq!(rewrite.commit_character, None);
            assert_eq!(rewrite.delete_before, 3);
        }
    }

    /// The setting is a real off switch, in both browser families and at the
    /// count where the arithmetic differs most.
    #[test]
    fn the_setting_switched_off_posts_every_plan_verbatim() {
        for bundle_id in [CHROME, SAFARI] {
            for count in [1, 2, 4] {
                let plan = tone(count);
                assert_eq!(
                    BrowserRewrite::plan(false, Some(bundle_id), &plan),
                    BrowserRewrite::verbatim(&plan),
                    "{bundle_id} {count}"
                );
            }
        }
    }

    /// The three guards, each proved in the browser where skipping it would be
    /// visible. Every one of them must answer the plan verbatim.
    #[test]
    fn every_guard_skips_both_strategies() {
        for bundle_id in [CHROME, SAFARI] {
            // Nothing to delete. This also covers the caret at the start of a
            // field, as far as this host can observe it at all.
            let insertion = OutputPlan {
                delete_before: 0,
                insert: Some("ư".into()),
                pass_through: false,
            };
            assert_eq!(
                BrowserRewrite::plan(true, Some(bundle_id), &insertion),
                BrowserRewrite::verbatim(&insertion),
                "{bundle_id} insertion"
            );

            // The engine is also letting the original key through.
            let committing = OutputPlan {
                delete_before: 2,
                insert: Some("ế".into()),
                pass_through: true,
            };
            assert_eq!(
                BrowserRewrite::plan(true, Some(bundle_id), &committing),
                BrowserRewrite::verbatim(&committing),
                "{bundle_id} pass-through"
            );

            // A deletion with nothing to insert.
            let deletion = OutputPlan {
                delete_before: 1,
                insert: None,
                pass_through: false,
            };
            assert_eq!(
                BrowserRewrite::plan(true, Some(bundle_id), &deletion),
                BrowserRewrite::verbatim(&deletion),
                "{bundle_id} deletion"
            );

            // A plan that does nothing at all.
            let empty = OutputPlan::default();
            assert_eq!(
                BrowserRewrite::plan(true, Some(bundle_id), &empty),
                BrowserRewrite::verbatim(&empty),
                "{bundle_id} empty"
            );
        }
    }

    /// Neither strategy may ever change what the user actually typed, and the
    /// two are mutually exclusive — a browser given both would delete one
    /// character too many.
    #[test]
    fn a_rewrite_never_touches_the_inserted_text_and_never_uses_both_strategies() {
        for bundle_id in [Some(CHROME), Some(SAFARI), Some("com.apple.Notes"), None] {
            for count in [0, 1, 2, 7] {
                let plan = tone(count);
                let rewrite = BrowserRewrite::plan(true, bundle_id, &plan);
                assert!(
                    !(rewrite.extend_selection && rewrite.commit_character.is_some()),
                    "{bundle_id:?} {count}"
                );
                // The plan itself is read-only; the insert is still the engine's.
                assert_eq!(plan.insert.as_deref(), Some("ế"));
            }
        }
    }
}
