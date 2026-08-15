//! What a script run is, as plain data: what it may see, what it may change,
//! and how it can fail.
//!
//! Nothing here knows about an engine. The types are what crosses between the
//! background executor and the UI thread — the same rule
//! [`Exchange`](crate::api_explorer::models::exchange::Exchange) follows — so a
//! run is testable, and swapping the engine behind
//! [`ScriptEngine`](crate::api_explorer::services::script::ScriptEngine)
//! changes nothing in this file.
//!
//! # The bounds are here, not in the engine
//!
//! An engine limit stops a script from exhausting *the engine*; it does nothing
//! about a script that hands the host a hundred megabytes of console output.
//! [`limits`] states what one run may produce, and the engine truncates against
//! it. Every truncation is counted and said out loud, never silently dropped —
//! the rule `api_scripts::Text::BodyTruncated` already follows in the response viewer.

use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::api_explorer::models::console::ConsoleEntry;
use crate::api_explorer::models::test_result::TestResult;
use crate::i18n::{Str, api_scripts};

/// What one run may produce before the engine starts truncating.
pub mod limits {
    /// Console entries kept from a single run.
    pub const CONSOLE_ENTRIES: usize = 200;
    /// Bytes of console text kept from a single run.
    pub const CONSOLE_BYTES: usize = 64 * 1024;
    /// Variable writes applied from a single run.
    pub const VARIABLE_WRITES: usize = 200;
    /// Bytes of a single variable value.
    pub const VARIABLE_VALUE_BYTES: usize = 256 * 1024;
    /// `pm.test` results kept from a single run — a script can define them in a
    /// loop just as easily as it can log in one.
    pub use crate::api_explorer::models::test_result::MAX_RESULTS as TEST_RESULTS;
}

/// Where a request's scripts came from.
///
/// This is a property of the **request**, fixed when it entered dodo, not of
/// the text currently in the editor. A request the user created here stays
/// [`Authored`] even after its script is edited; a request that arrived in an
/// imported collection stays [`Imported`] for its life, so editing an imported
/// script cannot launder it past the consent gate. What editing *does* change
/// is the content hash, which re-arms the gate — see
/// [`script_consent`](crate::api_explorer::models::script_consent).
///
/// `#[serde(default)]` on the snapshot field means a collection written before
/// this existed loads as [`Authored`], which is what it was: everything in it
/// was typed here or imported before scripts ran at all, and nothing ran.
///
/// [`Authored`]: ScriptOrigin::Authored
/// [`Imported`]: ScriptOrigin::Imported
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScriptOrigin {
    #[default]
    Authored,
    Imported,
}

/// The request as a pre-request script sees it, and may change it.
///
/// Headers are a `Vec` of pairs rather than a map for the reason
/// [`KeyValue`](crate::api_explorer::models::key_value::KeyValue) is: a request
/// may legitimately carry the same header name twice.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScriptRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

/// The response as a post-response script sees it, and may **not** change it.
///
/// A copy rather than a borrow of
/// [`Exchange`](crate::api_explorer::models::exchange::Exchange), for two
/// reasons that both matter. It keeps `Exchange` free of any scripting concept,
/// which is the one-way door `report.md` §7.2 flags; and it makes the response a
/// script sees genuinely read-only, so the Body and Headers tabs cannot end up
/// showing something other than what arrived.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScriptResponse {
    /// `pm.response.code`.
    pub code: u16,
    /// `pm.response.status` — the reason phrase (`"OK"`), which is why
    /// `Exchange` now captures it.
    pub status: String,
    /// In wire order, duplicates preserved.
    pub headers: Vec<(String, String)>,
    pub body: String,
    /// `pm.response.responseTime`, in milliseconds.
    pub elapsed_millis: u64,
    /// `pm.response.responseSize` — bytes received, not the decoded length.
    pub size_bytes: usize,
}

/// A syntax error the editor can point at, before anything is sent.
///
/// Line and column are **0-based**, ready for
/// `gpui_component::input::Position`; the engine reports them 1-based and the
/// conversion happens where the engine is named, so nothing downstream has to
/// remember which convention it is holding.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScriptSyntaxError {
    pub line: usize,
    pub column: usize,
    /// The engine's own wording, kept verbatim inside a translated frame — the
    /// convention `i18n.rs` documents for third-party parser text.
    pub detail: String,
}

/// Which persisted scope a script wrote to.
///
/// `pm.variables.set` is deliberately absent: it writes a value for *this run*
/// only, which is what Postman means by it, so it never reaches the store. It
/// comes back as [`ScriptRun::locals`] instead.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteScope {
    Environment,
    Collection,
}

/// One variable a script wrote, on its way back to the page's state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VariableWrite {
    pub scope: WriteScope,
    pub key: String,
    /// `None` is an unset.
    pub value: Option<String>,
}

/// Why a run produced no result.
///
/// Engine text (a `TypeError` and its line number) is kept verbatim inside a
/// translated frame, the convention `i18n.rs` documents for serde_json and
/// base64: there is nothing to translate it with.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScriptError {
    /// The script threw, or did not parse.
    Threw { detail: String },
    /// Stopped by the deadline. Almost always an unbounded loop.
    Deadline { seconds: u64 },
    /// Asked the engine for more memory than one run may have.
    OutOfMemory,
    /// Reached for something dodo deliberately does not provide.
    ///
    /// The binding genuinely is not registered — this is the *reporting* of
    /// that, so the failure names the API instead of reading as
    /// `undefined is not a function`. See
    /// [`unsupported`](crate::api_explorer::models::script::unsupported).
    Unsupported { name: String },
    /// There is no engine in this build — what
    /// [`NullEngine`](crate::api_explorer::services::script::NullEngine)
    /// reports. Never silently "ran and did nothing".
    ///
    /// `#[allow]`ed for the same reason `NullEngine` is: the shipping path
    /// wires up the real engine, so nothing constructs this outside tests.
    #[allow(dead_code)]
    NoEngine,
}

impl ScriptError {
    pub fn message(&self) -> Str {
        match self {
            ScriptError::Threw { detail } => api_scripts::Text::Threw(detail.clone()).into(),
            ScriptError::Deadline { seconds } => api_scripts::Text::Deadline(*seconds).into(),
            ScriptError::OutOfMemory => api_scripts::Text::OutOfMemory.into(),
            ScriptError::Unsupported { name } => {
                api_scripts::Text::Unsupported(name.clone()).into()
            }
            ScriptError::NoEngine => api_scripts::Text::NoEngine.into(),
        }
    }
}

/// Why a script that exists did not run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkipReason {
    /// The "Run scripts" setting is `Never`.
    PolicyDisabled,
    /// An imported script the user declined at the consent prompt.
    ConsentDeclined,
}

impl SkipReason {
    pub fn message(&self) -> Str {
        match self {
            SkipReason::PolicyDisabled => api_scripts::Text::SkippedByPolicy.into(),
            SkipReason::ConsentDeclined => api_scripts::Text::SkippedByConsent.into(),
        }
    }
}

/// One completed run of one script.
///
/// A run that failed still carries whatever it logged before it failed — that
/// is usually the only clue to *where* it failed, and throwing it away because
/// the run ended badly is exactly backwards.
#[derive(Clone, Debug, Default)]
pub struct ScriptRun {
    pub error: Option<ScriptError>,
    pub logs: Vec<ConsoleEntry>,
    /// Writes bound for the environment or collection scope, in the order the
    /// script made them.
    pub writes: Vec<VariableWrite>,
    /// `pm.variables.set` values: highest precedence for this send, never
    /// persisted.
    pub locals: Vec<(String, String)>,
    /// The environment scope as the script left it. The send path rebuilds the
    /// resolution layers from this rather than stacking writes on top of the
    /// originals, so precedence between the scopes survives a script writing
    /// into the middle of it.
    pub environment: std::collections::BTreeMap<String, String>,
    /// The collection scope, likewise.
    pub collection: std::collections::BTreeMap<String, String>,
    /// The request after the script, when the script actually changed it.
    ///
    /// `None` means "left alone", which is not the same as "equal to what went
    /// in": it is what lets the send path skip the write-back entirely and
    /// leave disabled header rows and field kinds untouched.
    pub request: Option<ScriptRequest>,
    /// What `pm.test` produced, in the order the script defined them.
    ///
    /// On [`ScriptRun`] rather than only on the post-response side because
    /// `pm.test` works in a pre-request script too, exactly as it does in
    /// Postman; each result carries the phase it came from.
    pub tests: Vec<TestResult>,
    pub duration: Duration,
    /// Console output the run's own caps dropped.
    pub dropped_logs: usize,
    /// Test results the run's own cap dropped.
    pub dropped_tests: usize,
}

impl ScriptRun {
    /// A run that reports one failure and nothing else — how a caller that
    /// never reached the engine (no engine, a refused consent) reports it.
    pub fn failed(error: ScriptError) -> Self {
        Self {
            error: Some(error),
            ..Self::default()
        }
    }
}

/// The `pm.*` and global names dodo deliberately does not provide, in the form
/// a thrown `TypeError` names them.
///
/// Used only to turn the engine's own "is not a function" into a message that
/// names the API and says dodo does not run it. Registering a throwing stub
/// instead was rejected in `decision-pm-sendrequest-scope`: a stub is a binding,
/// and the point is that there is none.
pub const UNSUPPORTED: [&str; 9] = [
    "pm.sendRequest",
    "pm.cookies",
    "pm.iterationData",
    "pm.visualizer",
    "pm.execution",
    "postman.setNextRequest",
    "require",
    "setTimeout",
    "setInterval",
];

/// The unsupported API a thrown message is about, if it is about one.
///
/// Two passes, in decreasing confidence:
///
/// 1. **The engine named it.** A bare identifier gets
///    `ReferenceError: setTimeout is not defined`, which says everything.
/// 2. **The engine did not.** A *property* call gets only
///    `TypeError: not a function` — QuickJS does not name the callee for
///    `pm.sendRequest(…)`. Falling back to the script source is the only way to
///    turn that into something a user can act on, and it is what `report.md`
///    §3.4c proposes. It is applied **only** when the failure is a
///    missing-callable error and **only** when exactly one unsupported name
///    appears in the source, so a script that merely mentions two of them in
///    comments gets the honest generic message rather than a guess.
pub fn unsupported(detail: &str, source: &str) -> Option<&'static str> {
    if !looks_like_a_missing_callable(detail) {
        return None;
    }

    if let Some(named) = UNSUPPORTED.into_iter().find(|name| detail.contains(name)) {
        return Some(named);
    }

    let mut mentioned = UNSUPPORTED.into_iter().filter(|name| source.contains(name));
    match (mentioned.next(), mentioned.next()) {
        (Some(only), None) => Some(only),
        _ => None,
    }
}

fn looks_like_a_missing_callable(detail: &str) -> bool {
    detail.contains("not a function")
        || detail.contains("not defined")
        || detail.contains("is undefined")
        || detail.contains("is not a constructor")
}

/// A stable hash of a script's text, used as half of a consent key.
///
/// FNV-1a rather than `DefaultHasher`: `std`'s is explicitly not stable across
/// releases, and this value is **written to disk**, so a toolchain upgrade must
/// not silently re-arm every approval the user has given.
pub fn script_hash(script: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in script.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{hash:016x}")
}

/// Whether a script has anything to run. Whitespace and nothing else is not a
/// script, and must not trip the consent prompt.
pub fn is_runnable(script: &str) -> bool {
    !script.trim().is_empty()
}

/// How long one script may run before the engine interrupts it.
pub const DEADLINE: Duration = Duration::from_secs(2);

/// How much memory one run may allocate.
pub const MEMORY_LIMIT: usize = 16 * 1024 * 1024;

/// How deep one run's JavaScript stack may go.
pub const STACK_LIMIT: usize = 256 * 1024;

#[cfg(test)]
mod tests {
    use super::{ScriptOrigin, is_runnable, script_hash, unsupported};

    #[test]
    fn the_hash_changes_when_one_character_does() {
        let before = script_hash("pm.environment.set(\"a\", 1);");
        let after = script_hash("pm.environment.set(\"a\", 2);");
        assert_ne!(before, after);
        // …and is stable for the same text, which is what an approval keys on.
        assert_eq!(before, script_hash("pm.environment.set(\"a\", 1);"));
    }

    #[test]
    fn the_hash_is_pinned_so_an_upgrade_cannot_re_arm_every_approval() {
        // FNV-1a of the empty string and of one known input. If either of these
        // changes, every consent record on every user's disk stops matching.
        assert_eq!(script_hash(""), "cbf29ce484222325");
        assert_eq!(script_hash("a"), "af63dc4c8601ec8c");
    }

    #[test]
    fn whitespace_is_not_a_script() {
        assert!(!is_runnable("   \n\t "));
        assert!(!is_runnable(""));
        assert!(is_runnable("// a comment is"));
    }

    #[test]
    fn an_unsupported_api_is_named_from_the_engines_own_message() {
        assert_eq!(
            unsupported(
                "ReferenceError: setTimeout is not defined",
                "setTimeout(f, 1)"
            ),
            Some("setTimeout")
        );
    }

    #[test]
    fn an_unnamed_missing_callable_falls_back_to_the_one_name_in_the_source() {
        // What `pm.sendRequest(…)` actually produces: QuickJS does not name the
        // callee for a property call.
        assert_eq!(
            unsupported("TypeError: not a function", "pm.sendRequest({}, cb);"),
            Some("pm.sendRequest")
        );
    }

    #[test]
    fn an_ambiguous_source_gets_the_honest_generic_message() {
        assert_eq!(
            unsupported(
                "TypeError: not a function",
                "// pm.sendRequest and require are both mentioned",
            ),
            None
        );
    }

    #[test]
    fn an_ordinary_failure_is_not_reported_as_an_unsupported_api() {
        assert_eq!(
            unsupported("TypeError: cannot read property 'x'", "pm.sendRequest"),
            None
        );
        // Naming one without the engine complaining about a callable is not a
        // claim.
        assert_eq!(
            unsupported("SyntaxError: unexpected token", "pm.sendRequest"),
            None
        );
    }

    #[test]
    fn a_request_that_predates_script_origins_loads_as_authored() {
        assert_eq!(ScriptOrigin::default(), ScriptOrigin::Authored);
    }
}
