//! One send, start to finish: script → `{{name}}` → `prepare` → the wire.
//!
//! Everything a send does after the editors have been read lives here, as one
//! blocking function over trait objects. That is what makes the *ordering*
//! testable: a fake [`Transport`] and a fake [`ScriptEngine`] are two small
//! structs, and no `Window` is involved.
//!
//! # The order, and the one place it departs from the plan
//!
//! ```text
//! pre-request script → resolve {{name}} → prepare → execute
//! ```
//!
//! `report.md` §4.1 drew this the other way round — substitute first, then run
//! the script. It is wrong, and the repository's own notes say so from both
//! ends:
//!
//! - `AGENTS.md` records that "a later scripting round only moves the [unresolved
//!   variable] check to after the pre-request hook", which is only true if the
//!   substitution moves with it.
//! - The shipped template `pm.variables.set("timestamp", Date.now())` is
//!   captioned *"Store the current time in a variable for this request."* With
//!   substitution first, `{{timestamp}}` in that same request resolves against
//!   a value the script has not written yet — the template would be a
//!   promise the engine breaks on arrival. Postman runs the script first for
//!   exactly this reason.
//!
//! What the report was actually protecting is unchanged and still holds:
//! **`prepare` is last**, so a header a script added still goes through header
//! validation and a URL a script rewrote still goes through scheme and host
//! validation, with dodo's own clear message rather than a failure deep inside
//! `reqwest`. A pre-request script therefore sees `pm.request.url` with its
//! `{{}}` still in it, which is also what Postman shows.
//!
//! # A failed pre-request script stops the send
//!
//! It does not "carry on without the script". A script that threw halfway
//! through has done some of its work — set one header of two, written one
//! variable of three — and sending that half-configured request would produce a
//! response nobody can reason about. The error names the script.

use std::collections::BTreeMap;

use crate::api_explorer::models::console::{ConsoleEntry, ConsoleLevel};
use crate::api_explorer::models::exchange::Exchange;
use crate::api_explorer::models::key_value::KeyValue;
use crate::api_explorer::models::method::HttpMethod;
use crate::api_explorer::models::request::RequestDraft;
use crate::api_explorer::models::script::{ScriptRequest, SkipReason, VariableWrite};
use crate::api_explorer::models::variables::{Variable, VariableScope, VariableSet};
use crate::api_explorer::services::Transport;
use crate::api_explorer::services::http::{prepare, resolve};
use crate::api_explorer::services::script::{ScriptContext, ScriptEngine};
use crate::i18n::Str;

/// What the send path should do about this request's pre-request script.
///
/// Decided on the UI thread, where the consent ledger and the setting live, so
/// the background job never has to ask a question.
pub enum ScriptJob {
    /// There is no script.
    None,
    /// There is one, and it must not run. Said out loud in the Console.
    Skipped(SkipReason),
    Run {
        source: String,
        environment: BTreeMap<String, String>,
        collection: BTreeMap<String, String>,
        request_name: String,
    },
}

/// Everything one send needs, owned so it can cross onto the background
/// executor.
pub struct SendJob {
    pub draft: RequestDraft,
    pub variables: VariableSet,
    pub script: ScriptJob,
}

/// Everything one send produced, owned so it can cross back. No `Entity`, no
/// `Rc`, no GPUI type — the same rule [`Exchange`] already crosses under.
pub struct SendOutcome {
    pub logs: Vec<ConsoleEntry>,
    /// Variable writes bound for the page's environment state. Emitted even
    /// when the request itself then failed: the script did write them, and
    /// pretending otherwise would make a retry behave differently from the
    /// first attempt for no visible reason.
    pub writes: Vec<VariableWrite>,
    pub result: Result<Exchange, Str>,
}

/// Runs one send. Blocking; always called from the background executor.
pub fn send(job: SendJob, engine: &dyn ScriptEngine, transport: &dyn Transport) -> SendOutcome {
    let SendJob {
        mut draft,
        mut variables,
        script,
    } = job;
    let mut logs = Vec::new();
    let mut writes = Vec::new();

    match script {
        ScriptJob::None => {}
        ScriptJob::Skipped(reason) => {
            logs.push(ConsoleEntry::runtime(ConsoleLevel::Warn, reason.message()));
        }
        ScriptJob::Run {
            source,
            environment,
            collection,
            request_name,
        } => {
            let context = ScriptContext {
                request: view(&draft),
                variables: variables.clone(),
                environment,
                collection,
                request_name,
            };

            let run = engine.run(&source, context);
            logs.extend(run.logs);
            if run.dropped_logs > 0 {
                logs.push(ConsoleEntry::runtime(
                    ConsoleLevel::Warn,
                    Str::ConsoleRunTruncated(run.dropped_logs),
                ));
            }

            if let Some(error) = run.error {
                logs.push(ConsoleEntry::runtime(ConsoleLevel::Error, error.message()));
                // The writes a failed run managed before it failed are still
                // real; they go back with the failure.
                return SendOutcome {
                    logs,
                    writes: run.writes,
                    result: Err(error.message()),
                };
            }

            logs.push(ConsoleEntry::runtime(
                ConsoleLevel::Debug,
                Str::ScriptFinished {
                    millis: run.duration.as_millis().min(u128::from(u64::MAX)) as u64,
                },
            ));
            if !run.writes.is_empty() {
                logs.push(ConsoleEntry::runtime(
                    ConsoleLevel::Debug,
                    Str::ScriptWroteVariables(run.writes.len()),
                ));
            }

            if let Some(request) = run.request {
                apply(&mut draft, request, &mut logs);
            }

            // Rebuilt from the scopes as the script left them, rather than
            // stacked on top of the originals: that keeps collection under
            // environment under script-local, which is the precedence
            // `models::variables` documents, even after a script has written to
            // the middle of it.
            variables = rebuilt(&run.environment, &run.collection, &run.locals);
            writes = run.writes;
        }
    }

    // `resolve` before `prepare`, so everything `prepare` validates is the text
    // that actually goes on the wire.
    let result = resolve::resolve(&draft, &variables)
        .and_then(|resolved| prepare::prepare(&resolved))
        .and_then(|prepared| transport.execute(prepared))
        .map_err(|error| error.message());

    SendOutcome {
        logs,
        writes,
        result,
    }
}

/// The request as a script first sees it.
///
/// Switched-off rows are not shown: a script asking `headers.has("X")` about a
/// row the user has unticked should hear "no", because that is what goes on the
/// wire.
fn view(draft: &RequestDraft) -> ScriptRequest {
    ScriptRequest {
        method: draft.method.as_str().to_string(),
        url: draft.url.clone(),
        headers: draft
            .headers
            .iter()
            .filter(|row| row.is_effective())
            .map(|row| (row.key.clone(), row.value.clone()))
            .collect(),
        body: draft.body.text.clone(),
    }
}

/// Writes a script's changes back onto the draft.
///
/// Only reached when the script actually changed something, so a request whose
/// script never touched `pm.request` keeps its rows exactly as the editor has
/// them — switched-off rows, field kinds and all.
fn apply(draft: &mut RequestDraft, request: ScriptRequest, logs: &mut Vec<ConsoleEntry>) {
    match HttpMethod::parse(&request.method) {
        Some(method) => draft.method = method,
        // A method dodo has no variant for is a warning, not a silent
        // downgrade to GET and not a failed send: the rest of the script's
        // work is still good.
        None => logs.push(ConsoleEntry::runtime(
            ConsoleLevel::Warn,
            Str::ScriptUnknownMethod(request.method.clone()),
        )),
    }
    draft.url = request.url;
    draft.body.text = request.body;
    draft.headers = request
        .headers
        .into_iter()
        .map(|(key, value)| KeyValue::text(key, value))
        .collect();
}

/// The layers the post-script `resolve` reads.
fn rebuilt(
    environment: &BTreeMap<String, String>,
    collection: &BTreeMap<String, String>,
    locals: &[(String, String)],
) -> VariableSet {
    let layer = |map: &BTreeMap<String, String>| -> Vec<Variable> {
        map.iter()
            .map(|(key, value)| Variable {
                key: key.clone(),
                value: value.clone(),
                enabled: true,
                secret: false,
            })
            .collect()
    };

    let mut set = VariableSet::default();
    set.push_layer(VariableScope::Collection, layer(collection));
    set.push_layer(VariableScope::Environment, layer(environment));
    if !locals.is_empty() {
        set.push_layer(VariableScope::Script, ScriptContext::locals_layer(locals));
    }
    set
}

#[cfg(test)]
mod tests {
    use super::{ScriptJob, SendJob, SendOutcome, send};
    use crate::api_explorer::models::body::BodyDraft;
    use crate::api_explorer::models::console::ConsoleLevel;
    use crate::api_explorer::models::exchange::{BodyKind, Exchange};
    use crate::api_explorer::models::key_value::KeyValue;
    use crate::api_explorer::models::method::HttpMethod;
    use crate::api_explorer::models::request::RequestDraft;
    use crate::api_explorer::models::script::{
        ScriptError, ScriptRequest, ScriptRun, SkipReason, VariableWrite, WriteScope,
    };
    use crate::api_explorer::models::variables::{Variable, VariableScope, VariableSet};
    use crate::api_explorer::services::script::{QuickJsEngine, ScriptContext, ScriptEngine};
    use crate::api_explorer::services::{PreparedRequest, Protocol, Transport, TransportError};
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    /// A transport that records what it was asked to send and answers 200.
    #[derive(Default)]
    struct Recorder {
        seen: Mutex<Option<PreparedRequest>>,
    }

    impl Transport for Recorder {
        fn protocol(&self) -> Protocol {
            Protocol::Http
        }

        fn execute(&self, request: PreparedRequest) -> Result<Exchange, TransportError> {
            *self.seen.lock().expect("lock") = Some(request);
            Ok(Exchange {
                status: 200,
                headers: Vec::new(),
                body: String::new(),
                kind: BodyKind::Text,
                size_bytes: 0,
                truncated: false,
                elapsed: std::time::Duration::ZERO,
            })
        }
    }

    /// An engine that returns a canned run without evaluating anything, so the
    /// pipeline can be tested apart from the language.
    struct Canned(Mutex<Option<ScriptRun>>);

    impl ScriptEngine for Canned {
        fn run(&self, _script: &str, _context: ScriptContext) -> ScriptRun {
            self.0.lock().expect("lock").take().unwrap_or_default()
        }
    }

    fn draft() -> RequestDraft {
        RequestDraft {
            method: HttpMethod::Get,
            url: "https://example.com/things".into(),
            params: Vec::new(),
            headers: vec![KeyValue::text("Accept", "application/json")],
            body: BodyDraft::default(),
            auth: Default::default(),
        }
    }

    fn job(script: ScriptJob) -> SendJob {
        SendJob {
            draft: draft(),
            variables: VariableSet::default(),
            script,
        }
    }

    fn run_job(job: SendJob, run: ScriptRun) -> (SendOutcome, Recorder) {
        let transport = Recorder::default();
        let engine = Canned(Mutex::new(Some(run)));
        let outcome = send(job, &engine, &transport);
        (outcome, transport)
    }

    #[test]
    fn with_no_script_the_request_goes_out_unchanged() {
        let (outcome, transport) = run_job(job(ScriptJob::None), ScriptRun::default());
        assert!(outcome.result.is_ok());
        assert!(outcome.logs.is_empty());

        let seen = transport.seen.lock().expect("lock").take().expect("sent");
        assert_eq!(seen.url, "https://example.com/things");
        assert_eq!(
            seen.headers,
            vec![("Accept".into(), "application/json".into())]
        );
    }

    #[test]
    fn a_skipped_script_says_so_and_still_sends() {
        let (outcome, _) = run_job(
            job(ScriptJob::Skipped(SkipReason::ConsentDeclined)),
            ScriptRun::default(),
        );
        assert!(outcome.result.is_ok());
        assert_eq!(outcome.logs.len(), 1);
        assert_eq!(outcome.logs[0].level, ConsoleLevel::Warn);
    }

    fn running(source: &str) -> ScriptJob {
        ScriptJob::Run {
            source: source.into(),
            environment: BTreeMap::new(),
            collection: BTreeMap::new(),
            request_name: "Things".into(),
        }
    }

    #[test]
    fn a_scripts_request_changes_reach_the_wire() {
        let run = ScriptRun {
            request: Some(ScriptRequest {
                method: "POST".into(),
                url: "https://example.com/other".into(),
                headers: vec![("X-Token".into(), "abc".into())],
                body: "{\"a\":1}".into(),
            }),
            ..ScriptRun::default()
        };
        let (outcome, transport) = run_job(job(running("…")), run);
        assert!(outcome.result.is_ok());

        let seen = transport.seen.lock().expect("lock").take().expect("sent");
        assert_eq!(seen.method, HttpMethod::Post);
        assert_eq!(seen.url, "https://example.com/other");
        assert_eq!(seen.headers, vec![("X-Token".into(), "abc".into())]);
    }

    #[test]
    fn a_method_dodo_has_no_variant_for_warns_rather_than_failing_the_send() {
        let run = ScriptRun {
            request: Some(ScriptRequest {
                method: "PROPFIND".into(),
                url: "https://example.com/things".into(),
                headers: Vec::new(),
                body: String::new(),
            }),
            ..ScriptRun::default()
        };
        let (outcome, transport) = run_job(job(running("…")), run);
        assert!(outcome.result.is_ok());
        assert!(
            outcome
                .logs
                .iter()
                .any(|entry| entry.level == ConsoleLevel::Warn)
        );

        let seen = transport.seen.lock().expect("lock").take().expect("sent");
        assert_eq!(seen.method, HttpMethod::Get, "the editor's method stands");
    }

    #[test]
    fn a_failed_script_stops_the_send_and_keeps_what_it_logged() {
        let run = ScriptRun {
            error: Some(ScriptError::Deadline { seconds: 2 }),
            logs: vec![crate::api_explorer::models::console::ConsoleEntry::script(
                ConsoleLevel::Log,
                "got this far",
            )],
            ..ScriptRun::default()
        };
        let (outcome, transport) = run_job(job(running("while (true) {}")), run);

        assert!(outcome.result.is_err());
        assert!(
            transport.seen.lock().expect("lock").is_none(),
            "nothing was sent"
        );
        assert!(
            outcome
                .logs
                .iter()
                .any(|entry| entry.message == "got this far"),
            "the run's own output was thrown away with the failure"
        );
    }

    #[test]
    fn a_variable_the_script_wrote_resolves_in_the_url_of_this_very_request() {
        // The ordering claim this module's doc argues: script first, then
        // `{{name}}`.
        let mut job = job(running("…"));
        job.draft.url = "https://example.com/{{path}}".into();

        let run = ScriptRun {
            environment: BTreeMap::from([("path".into(), "chosen".into())]),
            writes: vec![VariableWrite {
                scope: WriteScope::Environment,
                key: "path".into(),
                value: Some("chosen".into()),
            }],
            ..ScriptRun::default()
        };
        let (outcome, transport) = run_job(job, run);
        assert!(
            outcome.result.is_ok(),
            "{:?}",
            outcome.result.err().is_some()
        );

        let seen = transport.seen.lock().expect("lock").take().expect("sent");
        assert_eq!(seen.url, "https://example.com/chosen");
        assert_eq!(outcome.writes.len(), 1);
    }

    #[test]
    fn a_run_local_variable_outranks_the_environment_for_this_send_only() {
        let mut job = job(running("…"));
        job.draft.url = "https://example.com/{{who}}".into();
        job.variables.push_layer(
            VariableScope::Environment,
            vec![Variable::new("who", "configured")],
        );

        let run = ScriptRun {
            environment: BTreeMap::from([("who".into(), "configured".into())]),
            locals: vec![("who".into(), "local".into())],
            ..ScriptRun::default()
        };
        let (_, transport) = run_job(job, run);
        let seen = transport.seen.lock().expect("lock").take().expect("sent");
        assert_eq!(seen.url, "https://example.com/local");
    }

    #[test]
    fn an_unresolved_variable_still_fails_the_request_after_the_hook() {
        let mut job = job(running("…"));
        job.draft.url = "https://example.com/{{missing}}".into();
        let (outcome, transport) = run_job(job, ScriptRun::default());
        assert!(outcome.result.is_err());
        assert!(transport.seen.lock().expect("lock").is_none());
    }

    #[test]
    fn prepare_still_validates_what_a_script_produced() {
        // The half of `report.md` §4.1 that is right: `prepare` is last.
        let run = ScriptRun {
            request: Some(ScriptRequest {
                method: "GET".into(),
                url: "ftp://example.com/things".into(),
                headers: Vec::new(),
                body: String::new(),
            }),
            ..ScriptRun::default()
        };
        let (outcome, transport) = run_job(job(running("…")), run);
        assert!(
            outcome.result.is_err(),
            "an ftp:// URL from a script must fail"
        );
        assert!(transport.seen.lock().expect("lock").is_none());
    }

    /// End to end with the real engine: the shipped template's promise.
    #[test]
    fn the_shipped_timestamp_template_resolves_in_the_same_request() {
        let mut job = job(running("pm.variables.set(\"timestamp\", Date.now());"));
        job.draft.url = "https://example.com/things?t={{timestamp}}".into();

        let transport = Recorder::default();
        let outcome = send(job, &QuickJsEngine, &transport);
        assert!(
            outcome.result.is_ok(),
            "the template failed: {:?}",
            outcome.result.as_ref().err()
        );

        let seen = transport.seen.lock().expect("lock").take().expect("sent");
        let stamped = seen.url.rsplit_once("t=").expect("the query survived").1;
        assert!(
            stamped.parse::<u64>().is_ok(),
            "the template left {{timestamp}} unresolved: {}",
            seen.url
        );
    }
}
