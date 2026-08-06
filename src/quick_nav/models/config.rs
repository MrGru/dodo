//! `quick-nav.json` — the **seventh** thing dodo persists, and the second that
//! is a *setting* rather than data.
//!
//! Every other entry in the Settings dialog resets on launch by deliberate
//! design (`AGENTS.md`, and `settings.rs`'s own module doc). This one does not,
//! and the reason is the same as `updater.json`'s `skipped_version`: the setting
//! holds **text the user typed**. A detector pattern someone spent a minute
//! getting right, thrown away at the next launch, would be worse than not
//! offering the field at all.
//!
//! It follows the file discipline `AGENTS.md` names as the one to copy —
//! `script-consent.json`'s and `updater.json`'s, pointedly **not**
//! `collections.json`'s:
//!
//! - an explicit `"version"` written from the very first save;
//! - a parser that **refuses** a higher version rather than half-reading it
//!   (see [`super::super::services::config_store::parse_document`]);
//! - a missing file meaning *first run*, not an error;
//! - a temp-file-then-rename write.
//!
//! # Why the patterns are a map rather than five fields
//!
//! [`QuickNavDocument::patterns`] is keyed by
//! [`Detector::code`](super::detect::Detector::code). Five named fields would
//! read better in the file and would make adding a sixth detector a schema
//! change — a new field, a new default, a new `#[serde(default)]`. A map makes
//! it nothing at all, which is the extensibility the captain asked for, and it
//! means a pattern belonging to a detector this build does not know is carried
//! through a load/save cycle instead of being deleted.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::detect::Detector;

/// The schema version written into every `quick-nav.json`.
pub const SCHEMA_VERSION: u32 = 1;

/// Quick navigation is **on** unless the file says otherwise. The captain chose
/// the default; it is stated here because a `#[serde(default)]` on a `bool`
/// would silently mean `false` and turn a hand-edited file into a disabled
/// feature.
const DEFAULT_ENABLED: bool = true;

/// The persisted quick-navigation settings.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct QuickNavDocument {
    /// Written first and read first. See the module doc.
    pub version: u32,
    /// The master switch. Off, no keystroke is claimed and nothing is read from
    /// the clipboard.
    #[serde(default = "enabled_by_default")]
    pub enabled: bool,
    /// The user's pattern per detector, keyed by
    /// [`Detector::code`](super::detect::Detector::code). A missing or empty
    /// entry means "use the detector's default", which for the three
    /// parser-backed detectors means no gate at all.
    #[serde(default)]
    pub patterns: BTreeMap<String, String>,
}

fn enabled_by_default() -> bool {
    DEFAULT_ENABLED
}

impl Default for QuickNavDocument {
    fn default() -> Self {
        Self {
            version: SCHEMA_VERSION,
            enabled: DEFAULT_ENABLED,
            patterns: BTreeMap::new(),
        }
    }
}

impl QuickNavDocument {
    /// The user's pattern for `detector`, or `""` if they have set none.
    ///
    /// Returned raw — untrimmed and uncompiled — because this is what the
    /// settings field shows and what the file holds. Compiling it, and deciding
    /// what an empty one means, is [`super::pattern::compile`]'s job.
    pub fn pattern(&self, detector: Detector) -> &str {
        self.patterns
            .get(detector.code())
            .map(String::as_str)
            .unwrap_or_default()
    }

    /// Sets one detector's pattern. An empty one removes the key rather than
    /// storing a blank, so a file that has never been customized stays empty and
    /// a cleared field really does mean "back to the default".
    pub fn set_pattern(&mut self, detector: Detector, source: impl Into<String>) {
        let source = source.into();
        if source.trim().is_empty() {
            self.patterns.remove(detector.code());
        } else {
            self.patterns.insert(detector.code().to_owned(), source);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{QuickNavDocument, SCHEMA_VERSION};
    use crate::quick_nav::models::detect::Detector;

    #[test]
    fn quick_navigation_is_on_out_of_the_box() {
        let document = QuickNavDocument::default();
        assert!(document.enabled, "the captain's default is on");
        assert_eq!(document.version, SCHEMA_VERSION);
        assert!(document.patterns.is_empty());
    }

    #[test]
    fn an_unset_pattern_is_the_empty_string() {
        let document = QuickNavDocument::default();
        for detector in Detector::ORDER {
            assert_eq!(document.pattern(detector), "");
        }
    }

    #[test]
    fn setting_and_clearing_a_pattern_round_trips() {
        let mut document = QuickNavDocument::default();
        document.set_pattern(Detector::Base64, "^[A-Za-z0-9+/]+=*$");
        assert_eq!(document.pattern(Detector::Base64), "^[A-Za-z0-9+/]+=*$");
        assert_eq!(document.pattern(Detector::Jwt), "");

        document.set_pattern(Detector::Base64, "  ");
        assert_eq!(
            document.pattern(Detector::Base64),
            "",
            "a cleared field means the default, not a blank pattern",
        );
        assert!(
            document.patterns.is_empty(),
            "and it leaves no key behind in the file",
        );
    }

    /// A key this build does not recognise survives a load/save cycle, so a file
    /// written by a dodo with a sixth detector is not quietly pruned by one
    /// without it.
    #[test]
    fn a_pattern_for_an_unknown_detector_is_carried_through() {
        let json = r#"{"version":1,"enabled":true,"patterns":{"graphql":"^query "}}"#;
        let document: QuickNavDocument = serde_json::from_str(json).expect("parses");
        let round_tripped = serde_json::to_string(&document).expect("serializes");
        assert!(round_tripped.contains("graphql"));
        assert!(round_tripped.contains("^query "));
    }

    /// The default has to be spelled out: `#[serde(default)]` on a `bool` is
    /// `false`, which would turn a file missing the key into a disabled feature.
    #[test]
    fn a_file_that_omits_enabled_is_still_enabled() {
        let document: QuickNavDocument = serde_json::from_str(r#"{"version":1}"#).expect("parses");
        assert!(document.enabled);
        assert!(document.patterns.is_empty());
    }
}
