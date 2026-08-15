//! Which tools the sidebar shows, and in what order.
//!
//! The captain asked for this on 2026-08-06: *"for the feature flag, I want to
//! make a setting name Feature: for it we can on/off for each tab and reorder
//! them (drag ordering) so user can only show little items in tab and order
//! what feature they want to be above, this one should be persistance too."*
//!
//! Everything here is **pure**: `&'static str` codes in, a resolved list out,
//! no GPUI and no window. That is deliberate — every way this feature can ship
//! broken is a data question (a tool switched off while it is open, a stored
//! order from a build with more tools, an empty sidebar) and each one is a unit
//! test below rather than something to be found by looking at the app.
//!
//! [`Features`] is the resolved list. [`super::document::ToolRecord`] is what is
//! written to `session.json`; the two are not the same type on purpose, because
//! the file may say anything and this may not.
//!
//! # The four rules
//!
//! 1. **The stored order decides, but only about tools this build has.** An
//!    entry naming a tool that is not here is dropped rather than kept as a gap
//!    — see [`Features::resolve`]. A tool this build has that the stored order
//!    never mentions is **put back beside the tool it was declared after**, so
//!    one added in the middle of the list arrives in the middle even after the
//!    user has reordered everything around it. It arrives *enabled*: a feature
//!    nobody has ever seen must not turn up switched off and invisible.
//! 2. **At least one tool is always visible.** An empty sidebar leaves a user
//!    with an empty main pane and no way back to the Settings dialog's tool
//!    list, so [`Features::set_enabled`] refuses the last one — the caller shows
//!    the refusal — and [`Features::resolve`] repairs a hand-edited file that
//!    switched everything off.
//! 3. **The tool on screen is always a visible one.** [`Features::active`]
//!    answers that question once, for both the restored session (a remembered
//!    tool since switched off) and the live one (the open tool switched off just
//!    now). Both fall back to the **first enabled tool in sidebar order**, which
//!    is the one the user's own ordering says matters most.
//! 4. **This order is the sidebar's alone.** Quick navigation's detection order
//!    is a correctness ordering — most specific first — and is not this. See
//!    `quick_nav::models::detect`; dragging Base64 above JWT in the sidebar must
//!    not change what a pasted token does.

use crate::i18n::{Str, session};

use super::document::ToolRecord;

/// One tool in the sidebar, and whether it is shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Feature {
    /// A [`crate::tools::View::code`] — the same stable identifier
    /// `session.json` files the open tool under.
    pub code: &'static str,
    pub enabled: bool,
}

/// Why a change to the tool list was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FeatureError {
    /// Switching this one off would leave the sidebar empty. Rule 2.
    LastVisibleTool,
}

impl FeatureError {
    /// What to tell the user. A [`Str`] rather than rendered text, so a message
    /// already on screen re-translates when the language changes.
    pub fn message(self) -> Str {
        match self {
            FeatureError::LastVisibleTool => session::Text::FeatureLastVisibleTool.into(),
        }
    }
}

/// The sidebar's tools, in the user's order, resolved against this build.
///
/// Construct it with [`Features::resolve`] and nothing else: the invariants the
/// rest of dodo relies on — every code known, no duplicates, at least one
/// enabled — are established there.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Features {
    entries: Vec<Feature>,
}

impl Features {
    /// Places `stored` against the tools this build actually has.
    ///
    /// `known` is every tool's code **in default sidebar order**; it is the
    /// authority on what exists, and `stored` is only a preference about it.
    /// `stored` of `None` is "the user has never chosen", which is every tool
    /// in default order and every one of them shown.
    ///
    /// The three repairs, each of which has a stored file that produces it:
    ///
    /// * an entry naming a tool this build does not have is **dropped** — a
    ///   file written by a dodo with a seventh tool, or by one before a tool was
    ///   removed;
    /// * a tool `stored` never names is **inserted after its nearest default
    ///   neighbour**, enabled — a file written before this build's newest tool
    ///   existed. Beside the neighbour rather than at an absolute index,
    ///   because the list it is being inserted into is the *user's* order and
    ///   index 2 of that means nothing;
    /// * an entry naming the same tool twice keeps the **first** position.
    ///
    /// …and then rule 2: if nothing at all is left enabled, the first tool is
    /// switched back on. Only a hand-edited file can reach that state, because
    /// [`Features::set_enabled`] will not produce it.
    pub fn resolve(stored: Option<&[ToolRecord]>, known: &[&'static str]) -> Self {
        let mut entries: Vec<Feature> = Vec::with_capacity(known.len());

        for record in stored.unwrap_or_default() {
            let Some(code) = known
                .iter()
                .copied()
                .find(|candidate| *candidate == record.code)
            else {
                continue;
            };
            if entries.iter().any(|entry| entry.code == code) {
                continue;
            }
            entries.push(Feature {
                code,
                enabled: record.enabled,
            });
        }

        // Whatever the file never mentioned, put back beside the tool it was
        // declared after. Walking `known` forwards means the nearest preceding
        // default neighbour is already placed by the time it is looked for, so
        // a run of missing tools comes back in default order — and one whose
        // neighbour the user moved comes back beside it, rather than at an
        // absolute index that says nothing about a reordered list.
        for (default_ix, code) in known.iter().copied().enumerate() {
            if entries.iter().any(|entry| entry.code == code) {
                continue;
            }
            let at = known[..default_ix]
                .iter()
                .rev()
                .find_map(|earlier| entries.iter().position(|entry| entry.code == *earlier))
                .map_or(0, |after| after + 1);
            entries.insert(
                at,
                Feature {
                    code,
                    enabled: true,
                },
            );
        }

        if !entries.is_empty() && !entries.iter().any(|entry| entry.enabled) {
            entries[0].enabled = true;
        }

        Self { entries }
    }

    /// Every tool, in sidebar order, shown or not. What the Features settings
    /// page lists.
    pub fn all(&self) -> &[Feature] {
        &self.entries
    }

    /// The tools the sidebar shows, in order.
    pub fn visible(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.entries
            .iter()
            .filter(|entry| entry.enabled)
            .map(|entry| entry.code)
    }

    pub fn is_enabled(&self, code: &str) -> bool {
        self.entries
            .iter()
            .any(|entry| entry.code == code && entry.enabled)
    }

    /// Whether switching `code` off is allowed — false for the last visible
    /// tool, which is rule 2 stated as a question the *control* can ask before
    /// the user presses it.
    ///
    /// Also false for a tool that is **already** hidden, which has nothing to
    /// hide. That is why the control asks [`Features::can_toggle`] instead:
    /// reading this one directly would draw every hidden tool's switch dead and
    /// leave the user no way to bring a tool back.
    pub fn can_hide(&self, code: &str) -> bool {
        self.is_enabled(code) && self.visible().count() > 1
    }

    /// Whether this tool's switch may be pressed at all — in either direction.
    ///
    /// Switching a tool *on* is never refused, so this is false for exactly one
    /// tool in exactly one state: the last visible one.
    pub fn can_toggle(&self, code: &str) -> bool {
        !self.is_enabled(code) || self.can_hide(code)
    }

    /// The tool to show, given the one the caller would like.
    ///
    /// `wanted` is a `session.json` code at launch and the open tool's code
    /// afterwards. Anything this build does not have, or has but is not
    /// showing, falls back to the first visible tool — rule 3, and the answer to
    /// both "the remembered tool was switched off" and "the open tool was just
    /// switched off".
    ///
    /// `None` only when there are no tools at all, which [`Features::resolve`]
    /// cannot produce from a non-empty `known`.
    pub fn active(&self, wanted: Option<&str>) -> Option<&'static str> {
        wanted
            .and_then(|wanted| {
                self.entries
                    .iter()
                    .find(|entry| entry.code == wanted && entry.enabled)
                    .map(|entry| entry.code)
            })
            .or_else(|| self.visible().next())
    }

    /// Shows or hides one tool.
    ///
    /// Refuses to hide the last visible one — rule 2. An unrecognised code is a
    /// no-op rather than an error: every caller's codes come out of
    /// [`Features::all`], so it cannot happen, and inventing a second error for
    /// it would put a message on screen that no user could cause.
    pub fn set_enabled(&mut self, code: &str, enabled: bool) -> Result<(), FeatureError> {
        if !enabled && self.is_enabled(code) && !self.can_hide(code) {
            return Err(FeatureError::LastVisibleTool);
        }

        if let Some(entry) = self.entries.iter_mut().find(|entry| entry.code == code) {
            entry.enabled = enabled;
        }
        Ok(())
    }

    /// Moves one tool to `index`, which is clamped to the list.
    ///
    /// This is the drop half of the drag: the settings page hands over the row
    /// the pointer was released on. Returns whether the order actually changed,
    /// so a drop on a row's own position costs no write.
    pub fn move_to(&mut self, code: &str, index: usize) -> bool {
        let Some(from) = self.entries.iter().position(|entry| entry.code == code) else {
            return false;
        };
        let to = index.min(self.entries.len().saturating_sub(1));
        if from == to {
            return false;
        }

        let entry = self.entries.remove(from);
        self.entries.insert(to, entry);
        true
    }

    /// Moves one tool by `delta` places, stopping at either end.
    ///
    /// The keyboard half of the reorder, and the reason the move-up/move-down
    /// buttons need nothing of their own: at the top, moving up is a no-op
    /// rather than a wrap, because a list that wraps is one a user cannot walk
    /// an item down without watching it.
    pub fn move_by(&mut self, code: &str, delta: isize) -> bool {
        let Some(from) = self.entries.iter().position(|entry| entry.code == code) else {
            return false;
        };
        let last = self.entries.len().saturating_sub(1) as isize;
        let to = (from as isize + delta).clamp(0, last);
        self.move_to(code, to as usize)
    }

    /// What to write into `session.json`. The order is the list's order and the
    /// flag is each tool's own; nothing is inferred on the way back in.
    pub fn record(&self) -> Vec<ToolRecord> {
        self.entries
            .iter()
            .map(|entry| ToolRecord {
                code: entry.code.to_owned(),
                enabled: entry.enabled,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::{Feature, FeatureError, Features};
    use crate::i18n::Language;
    use crate::session::models::document::ToolRecord;

    /// Stand-in tool codes. Deliberately not dodo's real ones: these rules are
    /// about *a* list of tools, and testing them against `View::ALL` would tie
    /// every assertion to whichever six tools happen to ship.
    const KNOWN: [&str; 4] = ["alpha", "beta", "gamma", "delta"];

    fn record(code: &str, enabled: bool) -> ToolRecord {
        ToolRecord {
            code: code.to_owned(),
            enabled,
        }
    }

    fn codes(features: &Features) -> Vec<&'static str> {
        features.all().iter().map(|entry| entry.code).collect()
    }

    fn resolve(stored: &[ToolRecord]) -> Features {
        Features::resolve(Some(stored), &KNOWN)
    }

    #[test]
    fn nothing_stored_is_every_tool_in_default_order_and_all_of_them_shown() {
        let features = Features::resolve(None, &KNOWN);

        assert_eq!(codes(&features), KNOWN);
        assert_eq!(features.visible().collect::<Vec<_>>(), KNOWN);
        assert_eq!(features.record().len(), KNOWN.len());
    }

    #[test]
    fn a_stored_order_is_the_sidebar_order() {
        let features = resolve(&[
            record("delta", true),
            record("beta", false),
            record("alpha", true),
            record("gamma", true),
        ]);

        assert_eq!(codes(&features), ["delta", "beta", "alpha", "gamma"]);
        assert_eq!(
            features.visible().collect::<Vec<_>>(),
            ["delta", "alpha", "gamma"],
            "a hidden tool keeps its place in the list but not in the sidebar",
        );
    }

    /// Trap 6, first half: a file written by a dodo that had more tools than
    /// this one.
    #[test]
    fn a_stored_tool_this_build_does_not_have_is_dropped() {
        let features = resolve(&[
            record("epsilon", true),
            record("beta", true),
            record("graphql-explorer", false),
            record("alpha", true),
            record("gamma", true),
            record("delta", true),
        ]);

        assert_eq!(codes(&features), ["beta", "alpha", "gamma", "delta"]);
    }

    /// Trap 6, second half: a file written before this build's newest tool
    /// existed. It comes back beside the tool it was declared after, and it
    /// comes back **shown** — a feature nobody has seen must not turn up
    /// switched off.
    #[test]
    fn a_tool_the_file_never_mentions_lands_beside_its_default_neighbour() {
        // "gamma" is missing, and it is declared after "beta".
        let features = resolve(&[
            record("alpha", true),
            record("beta", true),
            record("delta", true),
        ]);

        assert_eq!(codes(&features), ["alpha", "beta", "gamma", "delta"]);
        assert!(features.is_enabled("gamma"));

        // …and it follows that neighbour into a *reordered* list, rather than
        // landing at an absolute index that means nothing there. "beta" and
        // "gamma" both follow "alpha", which the user moved second.
        let reordered = resolve(&[record("delta", true), record("alpha", true)]);
        assert_eq!(codes(&reordered), ["delta", "alpha", "beta", "gamma"]);
    }

    /// A run of missing tools comes back in default order, and a missing
    /// *first* tool — which has no neighbour to follow — leads.
    #[test]
    fn a_run_of_missing_tools_comes_back_in_default_order() {
        assert_eq!(codes(&resolve(&[record("alpha", true)])), KNOWN);
        assert_eq!(
            codes(&resolve(&[record("delta", true)])),
            ["alpha", "beta", "gamma", "delta"],
        );
    }

    #[test]
    fn a_tool_named_twice_keeps_its_first_position() {
        let features = resolve(&[
            record("delta", true),
            record("alpha", false),
            record("delta", false),
        ]);

        assert_eq!(codes(&features), ["delta", "alpha", "beta", "gamma"]);
        assert!(features.is_enabled("delta"), "the first entry is the one");
    }

    // ---- rule 2: the sidebar is never empty --------------------------------

    /// Trap 3. The last visible tool refuses to be switched off, and says so
    /// rather than doing nothing.
    #[test]
    fn the_last_visible_tool_cannot_be_switched_off() {
        let mut features = Features::resolve(None, &KNOWN);

        for code in ["beta", "gamma", "delta"] {
            assert_eq!(features.set_enabled(code, false), Ok(()));
        }
        assert_eq!(features.visible().collect::<Vec<_>>(), ["alpha"]);

        assert_eq!(
            features.set_enabled("alpha", false),
            Err(FeatureError::LastVisibleTool),
        );
        assert!(features.is_enabled("alpha"), "the refusal has to hold");
    }

    /// …and the refusal is something the user can read, in either language. A
    /// control that declines silently is the failure this rule exists to
    /// prevent.
    #[test]
    fn the_refusal_says_something_in_every_language() {
        for language in Language::ALL {
            assert!(
                !FeatureError::LastVisibleTool
                    .message()
                    .text(language)
                    .trim()
                    .is_empty()
            );
        }
    }

    /// …and the control can ask before the user presses it, which is what lets
    /// the switch render disabled with the reason beside it.
    #[test]
    fn can_hide_is_false_only_for_the_last_visible_tool() {
        let mut features = Features::resolve(None, &KNOWN);
        assert!(KNOWN.iter().all(|code| features.can_hide(code)));

        for code in ["beta", "gamma", "delta"] {
            features.set_enabled(code, false).expect("three may go");
        }

        assert!(!features.can_hide("alpha"));
        assert!(
            !features.can_hide("beta"),
            "an already-hidden tool has nothing to hide",
        );
    }

    /// The question the switch actually asks, and the reason it is not
    /// `can_hide`: a hidden tool must stay pressable, or switching one off
    /// would be a one-way door.
    #[test]
    fn only_the_last_visible_tools_switch_is_dead() {
        let mut features = Features::resolve(None, &KNOWN);
        assert!(KNOWN.iter().all(|code| features.can_toggle(code)));

        for code in ["beta", "gamma", "delta"] {
            features.set_enabled(code, false).expect("three may go");
        }

        assert!(!features.can_toggle("alpha"), "the last one may not go off");
        for hidden in ["beta", "gamma", "delta"] {
            assert!(
                features.can_toggle(hidden),
                "{hidden} is hidden, so its switch has to bring it back",
            );
        }
    }

    /// A hand-edited file that switched everything off is repaired rather than
    /// obeyed: an app with an empty sidebar and an empty pane has no way back.
    #[test]
    fn a_file_with_every_tool_switched_off_still_opens_on_one() {
        let features = resolve(&[
            record("alpha", false),
            record("beta", false),
            record("gamma", false),
            record("delta", false),
        ]);

        assert_eq!(features.visible().collect::<Vec<_>>(), ["alpha"]);
        assert_eq!(features.active(None), Some("alpha"));
    }

    /// Switching a tool back on is never refused, whatever the count.
    #[test]
    fn switching_a_tool_back_on_is_always_allowed() {
        let mut features = resolve(&[
            record("alpha", true),
            record("beta", false),
            record("gamma", false),
            record("delta", false),
        ]);

        assert_eq!(features.set_enabled("gamma", true), Ok(()));
        assert_eq!(features.visible().collect::<Vec<_>>(), ["alpha", "gamma"]);
    }

    #[test]
    fn an_unknown_code_changes_nothing() {
        let mut features = Features::resolve(None, &KNOWN);
        let before = features.clone();

        assert_eq!(features.set_enabled("epsilon", false), Ok(()));
        assert!(!features.move_to("epsilon", 0));
        assert!(!features.move_by("epsilon", 1));
        assert_eq!(features, before);
    }

    // ---- rule 3: the tool on screen is a visible one -----------------------

    /// Trap 2: `session.json` remembers a tool that has since been switched
    /// off. It must fall back rather than show a tool with no sidebar row.
    #[test]
    fn a_remembered_tool_that_is_switched_off_falls_back_to_the_first_visible() {
        let features = resolve(&[
            record("alpha", false),
            record("beta", false),
            record("gamma", true),
            record("delta", true),
        ]);

        assert_eq!(features.active(Some("beta")), Some("gamma"));
        assert_eq!(features.active(Some("gamma")), Some("gamma"));
    }

    /// Trap 1 is the same question asked at a different moment: the tool that
    /// is open right now has just been switched off.
    #[test]
    fn switching_off_the_open_tool_moves_to_the_first_visible_one() {
        let mut features = Features::resolve(None, &KNOWN);
        features.set_enabled("alpha", false).expect("not the last");

        assert_eq!(
            features.active(Some("alpha")),
            Some("beta"),
            "the pane cannot keep showing a tool the sidebar no longer lists",
        );
    }

    /// The fallback follows the **user's** order, not the default one — the
    /// first row of their sidebar is the one they said matters most.
    #[test]
    fn the_fallback_is_the_first_row_of_the_users_own_order() {
        let features = resolve(&[
            record("delta", true),
            record("gamma", true),
            record("beta", true),
            record("alpha", true),
        ]);

        assert_eq!(features.active(None), Some("delta"));
        assert_eq!(features.active(Some("epsilon")), Some("delta"));
    }

    #[test]
    fn an_unknown_or_absent_wanted_tool_falls_back() {
        let features = Features::resolve(None, &KNOWN);
        assert_eq!(features.active(None), Some("alpha"));
        assert_eq!(features.active(Some("")), Some("alpha"));
        assert_eq!(features.active(Some("graphql-explorer")), Some("alpha"));
    }

    /// Only reachable with no tools at all, which the app cannot produce — but
    /// the type says `Option` and the caller has to cope, so it is stated.
    #[test]
    fn a_build_with_no_tools_has_nothing_to_show() {
        let features = Features::resolve(None, &[]);
        assert!(features.all().is_empty());
        assert_eq!(features.active(Some("alpha")), None);
    }

    // ---- reordering --------------------------------------------------------

    #[test]
    fn a_drop_moves_the_tool_to_the_row_it_landed_on() {
        let mut features = Features::resolve(None, &KNOWN);

        assert!(features.move_to("delta", 0));
        assert_eq!(codes(&features), ["delta", "alpha", "beta", "gamma"]);

        assert!(features.move_to("delta", 2));
        assert_eq!(codes(&features), ["alpha", "beta", "delta", "gamma"]);
    }

    #[test]
    fn a_drop_on_the_rows_own_position_changes_nothing() {
        let mut features = Features::resolve(None, &KNOWN);
        assert!(!features.move_to("beta", 1));
        assert_eq!(codes(&features), KNOWN);
    }

    #[test]
    fn a_drop_past_the_end_lands_at_the_end() {
        let mut features = Features::resolve(None, &KNOWN);
        assert!(features.move_to("alpha", 99));
        assert_eq!(codes(&features), ["beta", "gamma", "delta", "alpha"]);
    }

    #[test]
    fn the_keyboard_moves_one_place_and_stops_at_either_end() {
        let mut features = Features::resolve(None, &KNOWN);

        assert!(features.move_by("gamma", -1));
        assert_eq!(codes(&features), ["alpha", "gamma", "beta", "delta"]);
        assert!(features.move_by("gamma", 1));
        assert_eq!(codes(&features), KNOWN);

        assert!(!features.move_by("alpha", -1), "the top does not wrap");
        assert!(!features.move_by("delta", 1), "and neither does the bottom");
        assert_eq!(codes(&features), KNOWN);
    }

    /// Hiding a tool must not move it: the Features page lists every tool, and
    /// a row that jumped when its switch was pressed would be unusable.
    #[test]
    fn hiding_a_tool_leaves_it_where_it_was() {
        let mut features = Features::resolve(None, &KNOWN);
        features.set_enabled("beta", false).expect("not the last");

        assert_eq!(codes(&features), KNOWN);
        assert_eq!(
            features.all()[1],
            Feature {
                code: "beta",
                enabled: false
            }
        );
    }

    // ---- the round trip ----------------------------------------------------

    /// The whole point of the file: order and enablement survive a restart.
    #[test]
    fn what_is_written_resolves_back_to_what_was_there() {
        let mut features = Features::resolve(None, &KNOWN);
        features.move_to("delta", 0);
        features.set_enabled("beta", false).expect("not the last");

        let written = features.record();
        assert_eq!(
            written.iter().map(|r| r.code.as_str()).collect::<Vec<_>>(),
            ["delta", "alpha", "beta", "gamma"],
        );

        assert_eq!(Features::resolve(Some(&written), &KNOWN), features);
    }
}
