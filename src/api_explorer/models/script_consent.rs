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
//! # One key covers **both** hooks
//!
//! [`ConsentKey::new`] hashes the pre-request and post-response scripts
//! together. That is not a convenience: an approval given when only the
//! pre-request hook existed said nothing about a post-response script that never
//! ran, and honouring it now would let a request execute code the user was never
//! shown. Hashing the pair re-arms exactly those approvals, which is the correct
//! answer. The prompt shows every script that will run, for the same reason.
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
    /// A hash of both scripts — see [`script_hash`] and this module's doc.
    pub hash: String,
}

impl ConsentKey {
    pub fn new(node: Option<NodeId>, pre: &str, post: &str) -> Self {
        Self {
            node,
            hash: script_hash(&joined(pre, post)),
        }
    }
}

/// The two hooks as one string to hash.
///
/// The `\u{1e}` (record separator) cannot appear in JavaScript source outside a
/// string literal, and even inside one it would have to be written as an escape
/// — so no pair of scripts can be rearranged into another pair with the same
/// digest.
fn joined(pre: &str, post: &str) -> String {
    format!("{pre}\u{1e}{post}")
}

/// What the send path should do about a script.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConsentDecision {
    /// There is no script, so there is nothing to decide.
    NoScript,
    /// Run it.
    Run,
    /// Do not run it, and say why in the Console.
    Skip,
    /// Show the scripts and ask.
    Ask {
        /// Whether this request had an approval that an edit has just
        /// invalidated. It changes what the prompt can truthfully say: "this
        /// has not run before" is false once an earlier version has.
        re_armed: bool,
    },
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

    /// Whether this node carries an approval for *some other* text — i.e. the
    /// user approved a script here and then it changed.
    ///
    /// Only claimed for a request that came from a saved node. Everything with
    /// no node shares one bucket (a history reopen, a pasted cURL command), and
    /// "you approved something else that also had no node" is not a statement
    /// about this request.
    fn re_armed(&self, key: &ConsentKey) -> bool {
        key.node.is_some()
            && self
                .document
                .approvals
                .iter()
                .any(|record| record.node == key.node && record.hash != key.hash)
    }

    /// What to do about this request's scripts, given where the request came
    /// from and what the user has already agreed to.
    ///
    /// Both hooks are decided together: they run in the same send, from the same
    /// request, and asking twice for one Send would be theatre.
    pub fn decide(
        &self,
        policy: ConsentPolicy,
        origin: ScriptOrigin,
        node: Option<NodeId>,
        pre: &str,
        post: &str,
    ) -> ConsentDecision {
        if !is_runnable(pre) && !is_runnable(post) {
            return ConsentDecision::NoScript;
        }
        match policy {
            ConsentPolicy::Never => ConsentDecision::Skip,
            ConsentPolicy::Always => ConsentDecision::Run,
            ConsentPolicy::AskImported => match origin {
                ScriptOrigin::Authored => ConsentDecision::Run,
                ScriptOrigin::Imported => {
                    let key = ConsentKey::new(node, pre, post);
                    if self.is_approved(&key) {
                        ConsentDecision::Run
                    } else {
                        ConsentDecision::Ask {
                            re_armed: self.re_armed(&key),
                        }
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
    const TEST: &str = "pm.test(\"ok\", function () {});";

    /// The first-visit prompt: nothing here has ever been approved.
    const FIRST: ConsentDecision = ConsentDecision::Ask { re_armed: false };

    #[test]
    fn a_script_typed_in_dodo_runs_without_a_prompt() {
        let ledger = ConsentLedger::default();
        assert_eq!(
            ledger.decide(
                ConsentPolicy::AskImported,
                ScriptOrigin::Authored,
                Some(3),
                SCRIPT,
                "",
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
                "",
            )
        };

        assert_eq!(decide(&ledger, SCRIPT), FIRST);
        ledger.approve(&ConsentKey::new(Some(3), SCRIPT, ""));
        assert_eq!(decide(&ledger, SCRIPT), ConsentDecision::Run);
    }

    #[test]
    fn editing_an_approved_script_re_arms_the_gate_and_says_so() {
        // The whole point of hashing the text rather than trusting the node —
        // and the prompt must not then claim the script has never run.
        let mut ledger = ConsentLedger::default();
        ledger.approve(&ConsentKey::new(Some(3), SCRIPT, ""));

        let edited = "pm.environment.set(\"token\", \"stolen\");";
        assert_eq!(
            ledger.decide(
                ConsentPolicy::AskImported,
                ScriptOrigin::Imported,
                Some(3),
                edited,
                "",
            ),
            ConsentDecision::Ask { re_armed: true }
        );
    }

    #[test]
    fn adding_a_post_response_script_re_arms_an_approval_given_for_the_other_hook() {
        // An approval given when only the pre-request hook ran said nothing
        // about a post-response script, so it must not carry over to one.
        let mut ledger = ConsentLedger::default();
        ledger.approve(&ConsentKey::new(Some(3), SCRIPT, ""));
        assert_eq!(
            ledger.decide(
                ConsentPolicy::AskImported,
                ScriptOrigin::Imported,
                Some(3),
                SCRIPT,
                TEST,
            ),
            ConsentDecision::Ask { re_armed: true }
        );
    }

    #[test]
    fn the_two_hooks_cannot_be_rearranged_into_the_same_approval() {
        let swapped = ConsentKey::new(Some(3), SCRIPT, TEST);
        assert_ne!(ConsentKey::new(Some(3), TEST, SCRIPT).hash, swapped.hash);
        // …and neither collides with the concatenation of the pair.
        assert_ne!(
            ConsentKey::new(Some(3), &format!("{SCRIPT}{TEST}"), "").hash,
            swapped.hash
        );
    }

    #[test]
    fn a_post_response_script_alone_still_asks() {
        let ledger = ConsentLedger::default();
        assert_eq!(
            ledger.decide(
                ConsentPolicy::AskImported,
                ScriptOrigin::Imported,
                Some(3),
                "",
                TEST,
            ),
            FIRST
        );
    }

    #[test]
    fn an_approval_does_not_spread_to_another_node() {
        let mut ledger = ConsentLedger::default();
        ledger.approve(&ConsentKey::new(Some(3), SCRIPT, ""));
        assert_eq!(
            ledger.decide(
                ConsentPolicy::AskImported,
                ScriptOrigin::Imported,
                Some(4),
                SCRIPT,
                "",
            ),
            FIRST,
            "another node has approved nothing, so nothing was re-armed either"
        );
    }

    #[test]
    fn a_request_with_no_node_never_claims_a_previous_approval() {
        // Everything without a node shares one bucket, so "something else here
        // was approved" says nothing about this request.
        let mut ledger = ConsentLedger::default();
        ledger.approve(&ConsentKey::new(None, SCRIPT, ""));
        assert_eq!(
            ledger.decide(
                ConsentPolicy::AskImported,
                ScriptOrigin::Imported,
                None,
                TEST,
                "",
            ),
            FIRST
        );
    }

    #[test]
    fn never_skips_and_always_runs_whatever_the_origin() {
        let ledger = ConsentLedger::default();
        for origin in [ScriptOrigin::Authored, ScriptOrigin::Imported] {
            assert_eq!(
                ledger.decide(ConsentPolicy::Never, origin, None, SCRIPT, TEST),
                ConsentDecision::Skip
            );
            assert_eq!(
                ledger.decide(ConsentPolicy::Always, origin, None, SCRIPT, TEST),
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
                "   \n ",
                "",
            ),
            ConsentDecision::NoScript
        );
    }

    #[test]
    fn approving_twice_does_not_grow_the_file() {
        let mut ledger = ConsentLedger::default();
        let key = ConsentKey::new(Some(1), SCRIPT, "");
        ledger.approve(&key);
        ledger.approve(&key);
        assert_eq!(ledger.document().approvals.len(), 1);
    }

    #[test]
    fn a_document_round_trips_with_its_version() {
        let mut ledger = ConsentLedger::default();
        ledger.approve(&ConsentKey::new(Some(7), SCRIPT, ""));

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
