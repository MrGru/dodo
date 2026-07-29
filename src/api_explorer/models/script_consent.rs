//! Who is allowed to run a script, and what the user has already agreed to.
//!
//! The premise, from `report.md` §5: **a script in an imported collection is
//! untrusted code from the internet, executing on a developer's machine, with
//! access to that developer's API tokens.** The sandbox bounds what such a
//! script can reach; this module bounds whether it runs at all.
//!
//! # The key is the script text, not the collection
//!
//! An approval records a [`ConsentKey`] — the node the request lives under
//! *and* a hash of the script's text. Both halves earn their place:
//!
//! - The **hash** is why editing an approved script re-arms the gate. A
//!   collection approved in January can be re-imported in March with a
//!   different body under the same name; "I approved this collection" is not a
//!   durable statement, "I approved this text" is.
//! - The **node** keeps an approval from spreading. Approving a script in one
//!   request does not silently approve the identical text somewhere the user
//!   never looked.
//!
//! A request with no saved node — a history reopen, a pasted cURL command —
//! keys on `None`, so it is one bucket rather than none.
//!
//! # The policy is a setting; the approvals are data
//!
//! [`ConsentPolicy`] lives in the Settings dialog beside Language, and like
//! every other dodo setting it is not persisted: it starts at
//! [`ConsentPolicy::AskImported`] every launch, which is the safe end. The
//! **approvals** are persisted, because re-approving every script on every
//! launch would train the user to click through the prompt without reading it —
//! which is the only thing the prompt is for.

use serde::{Deserialize, Serialize};

use crate::api_explorer::models::collection::NodeId;
use crate::api_explorer::models::script::{ScriptOrigin, is_runnable, script_hash};
use crate::i18n::Str;

/// The schema version written into every consent file.
///
/// Present from the first write, and refused when it is *higher* than this —
/// the pattern `models::variables` argues at length and `AGENTS.md` names as
/// the one to copy. Half-reading a file that records what the user agreed to
/// run would be the worst possible place to guess.
pub const SCHEMA_VERSION: u32 = 1;

/// What the user has said about running scripts at all.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ConsentPolicy {
    /// Nothing runs. The Scripts tab still edits and saves.
    Never,
    /// Scripts written here run; scripts that arrived by import ask first.
    #[default]
    AskImported,
    /// Everything runs, including a collection downloaded five minutes ago.
    Always,
}

impl ConsentPolicy {
    pub const ALL: [ConsentPolicy; 3] = [
        ConsentPolicy::Never,
        ConsentPolicy::AskImported,
        ConsentPolicy::Always,
    ];

    /// The stable identifier the settings dropdown stores. Not translated —
    /// it is a value, like `Language::code`.
    pub fn code(self) -> &'static str {
        match self {
            ConsentPolicy::Never => "never",
            ConsentPolicy::AskImported => "ask-imported",
            ConsentPolicy::Always => "always",
        }
    }

    pub fn from_code(code: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|policy| policy.code() == code)
            .unwrap_or_default()
    }

    pub fn label(self) -> Str {
        match self {
            ConsentPolicy::Never => Str::RunScriptsNever,
            ConsentPolicy::AskImported => Str::RunScriptsAskImported,
            ConsentPolicy::Always => Str::RunScriptsAlways,
        }
    }
}

/// What a request has to say about itself before a decision can be made.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConsentKey {
    /// The collection node this request was opened from, when it came from
    /// one.
    pub node: Option<NodeId>,
    /// A hash of the script text — see [`script_hash`].
    pub hash: String,
}

impl ConsentKey {
    pub fn new(node: Option<NodeId>, script: &str) -> Self {
        Self {
            node,
            hash: script_hash(script),
        }
    }
}

/// What the send path should do about a script.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ConsentDecision {
    /// There is no script, so there is nothing to decide.
    NoScript,
    /// Run it.
    Run,
    /// Do not run it, and say why in the Console.
    Skip,
    /// Show the script and ask.
    Ask,
}

/// One approval, as it is stored.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsentRecord {
    #[serde(default)]
    pub node: Option<NodeId>,
    pub hash: String,
}

/// Everything the consent file holds. `version` first and mandatory; see
/// [`SCHEMA_VERSION`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConsentDocument {
    pub version: u32,
    #[serde(default)]
    pub approvals: Vec<ConsentRecord>,
}

impl Default for ConsentDocument {
    fn default() -> Self {
        Self {
            version: SCHEMA_VERSION,
            approvals: Vec::new(),
        }
    }
}

/// The approvals the user has given, and the rule that reads them.
#[derive(Debug, Default)]
pub struct ConsentLedger {
    document: ConsentDocument,
}

impl ConsentLedger {
    pub fn document(&self) -> &ConsentDocument {
        &self.document
    }

    pub fn set_document(&mut self, document: ConsentDocument) {
        self.document = document;
    }

    pub fn is_approved(&self, key: &ConsentKey) -> bool {
        self.document
            .approvals
            .iter()
            .any(|record| record.node == key.node && record.hash == key.hash)
    }

    /// Records an approval. Idempotent, so approving the same text twice does
    /// not grow the file.
    pub fn approve(&mut self, key: &ConsentKey) {
        if self.is_approved(key) {
            return;
        }
        self.document.approvals.push(ConsentRecord {
            node: key.node,
            hash: key.hash.clone(),
        });
    }

    /// What to do about `script`, given where the request came from and what
    /// the user has already agreed to.
    pub fn decide(
        &self,
        policy: ConsentPolicy,
        origin: ScriptOrigin,
        node: Option<NodeId>,
        script: &str,
    ) -> ConsentDecision {
        if !is_runnable(script) {
            return ConsentDecision::NoScript;
        }
        match policy {
            ConsentPolicy::Never => ConsentDecision::Skip,
            ConsentPolicy::Always => ConsentDecision::Run,
            ConsentPolicy::AskImported => match origin {
                ScriptOrigin::Authored => ConsentDecision::Run,
                ScriptOrigin::Imported => {
                    if self.is_approved(&ConsentKey::new(node, script)) {
                        ConsentDecision::Run
                    } else {
                        ConsentDecision::Ask
                    }
                }
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        ConsentDecision, ConsentDocument, ConsentKey, ConsentLedger, ConsentPolicy, SCHEMA_VERSION,
    };
    use crate::api_explorer::models::script::ScriptOrigin;

    const SCRIPT: &str = "pm.environment.set(\"token\", \"abc\");";

    #[test]
    fn a_script_typed_in_dodo_runs_without_a_prompt() {
        let ledger = ConsentLedger::default();
        assert_eq!(
            ledger.decide(
                ConsentPolicy::AskImported,
                ScriptOrigin::Authored,
                Some(3),
                SCRIPT
            ),
            ConsentDecision::Run
        );
    }

    #[test]
    fn an_imported_script_asks_the_first_time_and_runs_after_approval() {
        let mut ledger = ConsentLedger::default();
        let decide = |ledger: &ConsentLedger, script: &str| {
            ledger.decide(
                ConsentPolicy::AskImported,
                ScriptOrigin::Imported,
                Some(3),
                script,
            )
        };

        assert_eq!(decide(&ledger, SCRIPT), ConsentDecision::Ask);
        ledger.approve(&ConsentKey::new(Some(3), SCRIPT));
        assert_eq!(decide(&ledger, SCRIPT), ConsentDecision::Run);
    }

    #[test]
    fn editing_an_approved_script_re_arms_the_gate() {
        // The whole point of hashing the text rather than trusting the node.
        let mut ledger = ConsentLedger::default();
        ledger.approve(&ConsentKey::new(Some(3), SCRIPT));

        let edited = "pm.environment.set(\"token\", \"stolen\");";
        assert_eq!(
            ledger.decide(
                ConsentPolicy::AskImported,
                ScriptOrigin::Imported,
                Some(3),
                edited
            ),
            ConsentDecision::Ask
        );
    }

    #[test]
    fn an_approval_does_not_spread_to_another_node() {
        let mut ledger = ConsentLedger::default();
        ledger.approve(&ConsentKey::new(Some(3), SCRIPT));
        assert_eq!(
            ledger.decide(
                ConsentPolicy::AskImported,
                ScriptOrigin::Imported,
                Some(4),
                SCRIPT
            ),
            ConsentDecision::Ask
        );
    }

    #[test]
    fn never_skips_and_always_runs_whatever_the_origin() {
        let ledger = ConsentLedger::default();
        for origin in [ScriptOrigin::Authored, ScriptOrigin::Imported] {
            assert_eq!(
                ledger.decide(ConsentPolicy::Never, origin, None, SCRIPT),
                ConsentDecision::Skip
            );
            assert_eq!(
                ledger.decide(ConsentPolicy::Always, origin, None, SCRIPT),
                ConsentDecision::Run
            );
        }
    }

    #[test]
    fn an_empty_script_never_prompts() {
        let ledger = ConsentLedger::default();
        assert_eq!(
            ledger.decide(
                ConsentPolicy::AskImported,
                ScriptOrigin::Imported,
                Some(1),
                "   \n "
            ),
            ConsentDecision::NoScript
        );
    }

    #[test]
    fn approving_twice_does_not_grow_the_file() {
        let mut ledger = ConsentLedger::default();
        let key = ConsentKey::new(Some(1), SCRIPT);
        ledger.approve(&key);
        ledger.approve(&key);
        assert_eq!(ledger.document().approvals.len(), 1);
    }

    #[test]
    fn a_document_round_trips_with_its_version() {
        let mut ledger = ConsentLedger::default();
        ledger.approve(&ConsentKey::new(Some(7), SCRIPT));

        let json = serde_json::to_string(ledger.document()).expect("serializes");
        assert!(json.contains(&format!("\"version\":{SCHEMA_VERSION}")));

        let back: ConsentDocument = serde_json::from_str(&json).expect("deserializes");
        assert_eq!(&back, ledger.document());
    }

    #[test]
    fn the_policy_round_trips_through_its_dropdown_value() {
        for policy in ConsentPolicy::ALL {
            assert_eq!(ConsentPolicy::from_code(policy.code()), policy);
        }
        assert_eq!(
            ConsentPolicy::from_code("nonsense"),
            ConsentPolicy::AskImported
        );
    }
}
