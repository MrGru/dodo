//! Running a script, behind a trait.
//!
//! This module is **the only place in the crate that may name `rquickjs`**, the
//! same containment rule `reqwest` and `bollard` live under. Everything else —
//! `state`, `views`, every model — talks to [`ScriptEngine`], so the engine
//! stays swappable.
//!
//! Be clear about what that buys, because it is easy to overclaim: the *crate*
//! is replaceable, the **`pm.*` language surface is not**. Once collections on
//! disk contain `pm.*` JavaScript, moving to a different language is a data
//! migration nobody can perform. The trait protects against `rquickjs` going
//! unmaintained (the fallback is `boa_engine`, pure Rust, measured at four
//! times the binary cost); it does not protect against choosing JavaScript.
//!
//! # Threading
//!
//! [`ScriptEngine::run`] is **blocking by contract**, exactly like
//! [`Transport::execute`](crate::services::Transport::execute),
//! and is only ever called from GPUI's background executor — `state::tab::send`
//! runs it inside the same job as `prepare` and the request itself. A fresh
//! runtime is built and dropped inside each call, so nothing engine-shaped
//! crosses a thread boundary and no state leaks between runs, between tabs or
//! between collections.
//!
//! # Synchronous, deliberately
//!
//! No `Promise` intrinsic is registered and nothing here is async. That is not
//! an omission to be filled in later: going async would change the shape of
//! every `pm.*` binding, the deadline mechanism and the outcome channel at
//! once. It is flagged as a one-way door in `report.md` §10.

mod quickjs;

use std::collections::BTreeMap;

use crate::models::script::{
    ScriptError, ScriptRequest, ScriptResponse, ScriptRun, ScriptSyntaxError,
};
use crate::models::test_result::ScriptPhase;
use crate::models::variables::{Variable, VariableSet};

pub use quickjs::QuickJsEngine;

/// Everything one run may see.
///
/// Owned plain data, built on the UI thread and moved into the background job
/// with the draft it belongs to — the same rule
/// [`VariableSet`] already follows.
pub struct ScriptContext {
    /// Which hook this is. Decides `pm.info.eventName`, and which phase the
    /// run's [`TestResult`]s are stamped with.
    ///
    /// [`TestResult`]: crate::models::test_result::TestResult
    pub phase: ScriptPhase,
    /// The request as the script first sees it. A pre-request script sees it
    /// **before** `{{name}}` substitution, which is what makes
    /// `pm.variables.set("timestamp", …)` followed by `{{timestamp}}` in the
    /// URL work the way the shipped template promises.
    pub request: ScriptRequest,
    /// What came back, for a post-response run. `None` in the pre-request
    /// phase, where `pm.response` is genuinely absent rather than an object
    /// full of zeroes — reaching for it must fail, not quietly read as a 0
    /// status.
    pub response: Option<ScriptResponse>,
    /// The layers `pm.variables.get` falls through to.
    pub variables: VariableSet,
    /// The environment scope as `pm.environment` sees it: enabled, named
    /// variables only.
    pub environment: BTreeMap<String, String>,
    /// The collection scope, likewise.
    pub collection: BTreeMap<String, String>,
    /// `pm.info.requestName`.
    pub request_name: String,
}

impl ScriptContext {
    /// The variables of one scope, reduced to what a script may see: switched
    /// on, with a name.
    pub fn scope_map(variables: &[Variable]) -> BTreeMap<String, String> {
        variables
            .iter()
            .filter(|variable| variable.is_effective())
            .map(|variable| (variable.key.trim().to_string(), variable.value.clone()))
            .collect()
    }

    /// The layer `pm.variables.set` values become, on top of everything the
    /// user configured. Run-scoped: never persisted, gone when the send ends.
    pub fn locals_layer(locals: &[(String, String)]) -> Vec<Variable> {
        locals
            .iter()
            .map(|(key, value)| Variable {
                key: key.clone(),
                value: value.clone(),
                enabled: true,
                secret: false,
            })
            .collect()
    }
}

/// A JavaScript engine dodo can hand a script to.
///
/// Blocking; see this module's threading note.
pub trait ScriptEngine: Send + Sync + 'static {
    fn run(&self, script: &str, context: ScriptContext) -> ScriptRun;

    /// Compiles `script` without running any of it, so the editor can underline
    /// a syntax error before the user presses Send.
    ///
    /// **The same engine answers both questions**, which is the point: a check
    /// that used a second parser could disagree with the thing that actually
    /// runs, and an editor that contradicts the Console is worse than an editor
    /// that says nothing. `None` means "nothing to report" — never "this
    /// script is correct".
    ///
    /// Cheap enough to run on every (debounced) keystroke: it builds a bare
    /// runtime with no bindings, compiles, and drops it.
    fn check(&self, script: &str) -> Option<ScriptSyntaxError>;
}

/// An engine that runs nothing, and says so.
///
/// For tests that want the send path without an engine, and for the shape of a
/// build compiled without one. It deliberately does **not** report success:
/// "the script ran and did nothing" is the lie `report.md` §3.4b is about.
///
/// `#[allow(dead_code)]` for the same reason `InMemoryVariableStore` carries
/// one: the app wires up the real engine, so this is never constructed in the
/// shipping path. It exists so that swapping the engine stays a one-line change
/// and so the trait has a second implementation keeping it honest.
#[allow(dead_code)]
pub struct NullEngine;

impl ScriptEngine for NullEngine {
    fn run(&self, _script: &str, _context: ScriptContext) -> ScriptRun {
        ScriptRun::failed(ScriptError::NoEngine)
    }

    /// Reports nothing rather than guessing. With no engine there is no parser,
    /// and inventing a second one here would be the contradiction
    /// [`ScriptEngine::check`] exists to avoid.
    fn check(&self, _script: &str) -> Option<ScriptSyntaxError> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{NullEngine, ScriptContext, ScriptEngine};
    use crate::models::script::{ScriptError, ScriptRequest};
    use crate::models::test_result::ScriptPhase;
    use crate::models::variables::{Variable, VariableSet};

    fn context() -> ScriptContext {
        ScriptContext {
            phase: ScriptPhase::PreRequest,
            request: ScriptRequest::default(),
            response: None,
            variables: VariableSet::default(),
            environment: Default::default(),
            collection: Default::default(),
            request_name: String::new(),
        }
    }

    #[test]
    fn the_null_engine_reports_that_it_ran_nothing() {
        let run = NullEngine.run("pm.environment.set('a', 1)", context());
        assert_eq!(run.error, Some(ScriptError::NoEngine));
        assert!(run.request.is_none());
        assert!(run.writes.is_empty());
    }

    #[test]
    fn a_scope_map_drops_switched_off_and_unnamed_rows() {
        let map = ScriptContext::scope_map(&[
            Variable::new(" host ", "example.com"),
            Variable {
                enabled: false,
                ..Variable::new("off", "x")
            },
            Variable::new("  ", "unnamed"),
        ]);
        assert_eq!(map.len(), 1);
        assert_eq!(map.get("host").map(String::as_str), Some("example.com"));
    }
}
