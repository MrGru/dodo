//! One send, start to finish: script → `{{name}}` → `prepare` → the wire →
//! script.
//!
//! Everything a send does after the editors have been read lives here, as one
//! blocking function over trait objects. That is what makes the *ordering*
//! testable: a fake [`Transport`] and a fake [`ScriptEngine`] are two small
//! structs, and no `Window` is involved.
//!
//! # The order, and the one place it departs from the plan
//!
//! ```text
//! pre-request script → resolve {{name}} → prepare → execute → post-response script
//! ```
//!
//! `report.md` §4.1 drew this the other way round — substitute first, then run
//! the script. It is wrong, and the repository's own notes say so from both
//! ends:
//!
//! - `dodo-api-explorer-internals` records that "a later scripting round only moves the [unresolved
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
//!
//! # A failed post-response script does **not** lose the response
//!
//! The mirror image, and the asymmetry is the point. By the time the
//! post-response hook runs the request has already happened: the response is a
//! fact, and throwing it away because a script had a typo would destroy the one
//! piece of evidence the user needs to fix that typo. So a post-response failure
//! becomes a Console line and a message on the Tests tab, and
//! `SendOutcome::result` stays `Ok`.
//!
//! The same reasoning is why the hook cannot change the response: `Exchange` is
//! what arrived, and a mutated one would make the Body and Headers tabs lie.
//! It may still write variables, which is how `pm.environment.set("id",
//! pm.response.json().id)` — the whole point of the hook — works.

use std::collections::BTreeMap;

use crate::i18n::{Str, api_explorer};
use crate::models::console::{ConsoleEntry, ConsoleLevel};
use crate::models::exchange::Exchange;
use crate::models::key_value::KeyValue;
use crate::models::method::HttpMethod;
use crate::models::request::RequestDraft;
use crate::models::script::{ScriptRequest, ScriptResponse, SkipReason, VariableWrite};
use crate::models::test_result::{ScriptPhase, TestReport};
use crate::models::variables::{Variable, VariableScope, VariableSet};
use crate::services::Transport;
use crate::services::http::{prepare, resolve};
use crate::services::script::{ScriptContext, ScriptEngine};

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
    /// The hook that runs before the request.
    pub pre: ScriptJob,
    /// The hook that runs after it. Decided on the UI thread at the same moment
    /// as `pre`, under the same consent key, so the background job never has to
    /// ask a second question halfway through.
    pub post: ScriptJob,
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
    /// What `pm.test` produced, from both hooks. Attached to `ResponseState`,
    /// never to [`Exchange`] — see
    /// [`test_result`](crate::models::test_result).
    pub tests: TestReport,
}

/// Runs one send. Blocking; always called from the background executor.
pub fn send(job: SendJob, engine: &dyn ScriptEngine, transport: &dyn Transport) -> SendOutcome {
    let SendJob {
        mut draft,
        mut variables,
        pre,
        post,
    } = job;
    let mut logs = Vec::new();
    let mut writes = Vec::new();
    let mut tests = TestReport::default();

    match pre {
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
                phase: ScriptPhase::PreRequest,
                request: view(&draft),
                response: None,
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
                    api_explorer::Text::ConsoleRunTruncated(run.dropped_logs).into(),
                ));
            }
            // Kept even when the run then failed: a test that ran is a fact.
            tests.results.extend(run.tests);
            tests.dropped += run.dropped_tests;

            if let Some(error) = run.error {
                logs.push(ConsoleEntry::runtime(ConsoleLevel::Error, error.message()));
                // The writes a failed run managed before it failed are still
                // real; they go back with the failure.
                return SendOutcome {
                    logs,
                    writes: run.writes,
                    result: Err(error.message()),
                    tests,
                };
            }

            logs.push(ConsoleEntry::runtime(
                ConsoleLevel::Debug,
                api_explorer::Text::ScriptFinished {
                    millis: millis(run.duration),
                }
                .into(),
            ));
            if !run.writes.is_empty() {
                logs.push(ConsoleEntry::runtime(
                    ConsoleLevel::Debug,
                    api_explorer::Text::ScriptWroteVariables(run.writes.len()).into(),
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

    // Nothing to run the post-response hook against when nothing came back —
    // and no `pm.response` to hand it if we tried.
    if let Ok(exchange) = &result {
        run_post(
            post,
            exchange,
            &draft,
            &variables,
            engine,
            &mut logs,
            &mut writes,
            &mut tests,
        );
    }

    SendOutcome {
        logs,
        writes,
        result,
        tests,
    }
}

/// The post-response hook. Everything it can change is a `&mut` argument, and
/// the response is not among them.
#[allow(clippy::too_many_arguments)]
fn run_post(
    job: ScriptJob,
    exchange: &Exchange,
    draft: &RequestDraft,
    variables: &VariableSet,
    engine: &dyn ScriptEngine,
    logs: &mut Vec<ConsoleEntry>,
    writes: &mut Vec<VariableWrite>,
    tests: &mut TestReport,
) {
    let ScriptJob::Run {
        source,
        environment,
        collection,
        request_name,
    } = job
    else {
        // A skipped pre-request script has already said so once; saying it
        // again for the other hook would be noise.
        return;
    };

    tests.ran = true;
    let context = ScriptContext {
        phase: ScriptPhase::PostResponse,
        request: view(draft),
        response: Some(ScriptResponse {
            code: exchange.status,
            status: exchange.reason.clone(),
            headers: exchange.headers.clone(),
            body: exchange.body.clone(),
            elapsed_millis: millis(exchange.elapsed),
            size_bytes: exchange.size_bytes,
        }),
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
            api_explorer::Text::ConsoleRunTruncated(run.dropped_logs).into(),
        ));
    }

    tests.results.extend(run.tests);
    tests.dropped += run.dropped_tests;
    tests.elapsed = run.duration;
    writes.extend(run.writes.iter().cloned());

    match run.error {
        Some(error) => {
            // The failure is reported, twice over — the Console keeps the
            // sequence, the Tests tab makes it the headline — and the response
            // is untouched.
            logs.push(ConsoleEntry::runtime(ConsoleLevel::Error, error.message()));
            tests.error = Some(error.message());
        }
        None => {
            logs.push(ConsoleEntry::runtime(
                ConsoleLevel::Debug,
                api_explorer::Text::TestScriptFinished {
                    millis: millis(run.duration),
                }
                .into(),
            ));
            if !run.writes.is_empty() {
                logs.push(ConsoleEntry::runtime(
                    ConsoleLevel::Debug,
                    api_explorer::Text::ScriptWroteVariables(run.writes.len()).into(),
                ));
            }
        }
    }
}

/// A duration as whole milliseconds, saturating rather than wrapping.
fn millis(duration: std::time::Duration) -> u64 {
    duration.as_millis().min(u128::from(u64::MAX)) as u64
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
            api_explorer::Text::ScriptUnknownMethod(request.method.clone()).into(),
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
    use crate::models::body::BodyDraft;
    use crate::models::console::ConsoleLevel;
    use crate::models::exchange::{BodyKind, Exchange};
    use crate::models::key_value::KeyValue;
    use crate::models::method::HttpMethod;
    use crate::models::request::RequestDraft;
    use crate::models::script::{
        ScriptError, ScriptRequest, ScriptRun, ScriptSyntaxError, SkipReason, VariableWrite,
        WriteScope,
    };
    use crate::models::test_result::{ScriptPhase, TestOutcome, TestResult};
    use crate::models::variables::{Variable, VariableScope, VariableSet};
    use crate::services::script::{QuickJsEngine, ScriptContext, ScriptEngine};
    use crate::services::{PreparedRequest, Protocol, Transport, TransportError};
    use std::collections::BTreeMap;
    use std::sync::Mutex;
    use std::time::Duration;

    /// A transport that records what it was asked to send and answers 200.
    #[derive(Default)]
    struct Recorder {
        seen: Mutex<Option<PreparedRequest>>,
        /// The body it answers with, so a post-response test has something to
        /// read.
        body: &'static str,
    }

    impl Recorder {
        fn answering(body: &'static str) -> Self {
            Self {
                body,
                ..Self::default()
            }
        }
    }

    impl Transport for Recorder {
        fn protocol(&self) -> Protocol {
            Protocol::Http
        }

        fn execute(&self, request: PreparedRequest) -> Result<Exchange, TransportError> {
            *self.seen.lock().expect("lock") = Some(request);
            Ok(Exchange {
                status: 200,
                reason: "OK".into(),
                headers: vec![("Content-Type".into(), "application/json".into())],
                body: self.body.to_string(),
                kind: BodyKind::Text,
                size_bytes: self.body.len(),
                truncated: false,
                elapsed: Duration::from_millis(12),
            })
        }
    }

    /// An engine that returns canned runs without evaluating anything, so the
    /// pipeline can be tested apart from the language. Each `run` takes the next
    /// canned outcome, so a pre-request and a post-response hook can be scripted
    /// independently.
    #[derive(Default)]
    struct Canned {
        queue: Mutex<Vec<ScriptRun>>,
        /// Every phase it was asked to run, in order.
        phases: Mutex<Vec<ScriptPhase>>,
        /// Whether the last run could see a response.
        saw_response: Mutex<Vec<bool>>,
    }

    impl Canned {
        fn of(runs: Vec<ScriptRun>) -> Self {
            Self {
                // Reversed so `pop` hands them back in order.
                queue: Mutex::new(runs.into_iter().rev().collect()),
                ..Self::default()
            }
        }
    }

    impl ScriptEngine for Canned {
        fn run(&self, _script: &str, context: ScriptContext) -> ScriptRun {
            self.phases.lock().expect("lock").push(context.phase);
            self.saw_response
                .lock()
                .expect("lock")
                .push(context.response.is_some());
            self.queue.lock().expect("lock").pop().unwrap_or_default()
        }

        fn check(&self, _script: &str) -> Option<ScriptSyntaxError> {
            None
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
            pre: script,
            post: ScriptJob::None,
        }
    }

    fn run_job(job: SendJob, run: ScriptRun) -> (SendOutcome, Recorder) {
        let transport = Recorder::default();
        let engine = Canned::of(vec![run]);
        let outcome = send(job, &engine, &transport);
        (outcome, transport)
    }

    fn test(name: &str, outcome: TestOutcome) -> TestResult {
        TestResult {
            name: name.into(),
            outcome,
            elapsed: Duration::from_millis(1),
            // Overwritten by the real engine; the canned one reports what it is
            // given, and the send path never rewrites it.
            phase: ScriptPhase::PostResponse,
        }
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
            logs: vec![crate::models::console::ConsoleEntry::script(
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

    // ---- the post-response hook ---------------------------------------------

    /// A job with both hooks armed.
    fn both(pre: &str, post: &str) -> SendJob {
        SendJob {
            pre: running(pre),
            post: running(post),
            ..job(ScriptJob::None)
        }
    }

    #[test]
    fn the_post_response_hook_runs_after_the_request_and_can_see_it() {
        let engine = Canned::of(vec![
            ScriptRun::default(),
            ScriptRun {
                tests: vec![test("Status is 200", TestOutcome::Passed)],
                ..ScriptRun::default()
            },
        ]);
        let transport = Recorder::answering("{\"id\":7}");
        let outcome = send(both("…", "…"), &engine, &transport);

        assert!(outcome.result.is_ok());
        assert_eq!(
            *engine.phases.lock().expect("lock"),
            vec![ScriptPhase::PreRequest, ScriptPhase::PostResponse]
        );
        assert_eq!(
            *engine.saw_response.lock().expect("lock"),
            vec![false, true],
            "only the post-response hook may see a response"
        );
        assert_eq!(outcome.tests.results.len(), 1);
        assert!(outcome.tests.ran);
    }

    #[test]
    fn a_throwing_post_response_script_keeps_the_response() {
        // The asymmetry this module's doc argues: the request already happened,
        // so the response is a fact and a script bug must not erase it.
        let engine = Canned::of(vec![
            ScriptRun::default(),
            ScriptRun {
                error: Some(ScriptError::Threw {
                    detail: "TypeError: null has no property 'id'".into(),
                }),
                tests: vec![test("First check", TestOutcome::Passed)],
                ..ScriptRun::default()
            },
        ]);
        let outcome = send(both("…", "…"), &engine, &Recorder::answering("{}"));

        let exchange = outcome.result.expect("the response survived the script");
        assert_eq!(exchange.status, 200);
        assert!(
            outcome.tests.error.is_some(),
            "the failure is still reported"
        );
        assert_eq!(
            outcome.tests.results.len(),
            1,
            "a test that ran before the throw is still a result"
        );
        assert!(
            outcome
                .logs
                .iter()
                .any(|entry| entry.level == ConsoleLevel::Error)
        );
    }

    #[test]
    fn a_failing_assertion_is_a_failed_result_rather_than_an_aborted_run() {
        let engine = Canned::of(vec![
            ScriptRun::default(),
            ScriptRun {
                tests: vec![
                    test("Status is 200", TestOutcome::Passed),
                    test(
                        "Body has an id",
                        TestOutcome::Failed {
                            message: "expected {} to have property 'id'".into(),
                        },
                    ),
                ],
                ..ScriptRun::default()
            },
        ]);
        let outcome = send(both("…", "…"), &engine, &Recorder::answering("{}"));

        assert!(outcome.result.is_ok());
        assert!(outcome.tests.error.is_none(), "the run itself did not fail");
        let summary = outcome.tests.summary();
        assert_eq!(summary.passed, 1);
        assert_eq!(summary.failed, 1);
        assert_eq!(summary.badge(), "1/2");
    }

    #[test]
    fn a_post_response_script_writes_variables_back() {
        // `pm.environment.set("id", pm.response.json().id)` — the whole point.
        let engine = Canned::of(vec![
            ScriptRun::default(),
            ScriptRun {
                writes: vec![VariableWrite {
                    scope: WriteScope::Environment,
                    key: "id".into(),
                    value: Some("7".into()),
                }],
                ..ScriptRun::default()
            },
        ]);
        let outcome = send(both("…", "…"), &engine, &Recorder::answering("{\"id\":7}"));
        assert_eq!(outcome.writes.len(), 1);
        assert_eq!(outcome.writes[0].key, "id");
    }

    #[test]
    fn a_request_that_never_returned_runs_no_post_response_script() {
        let mut job = both("…", "…");
        job.draft.url = "https://example.com/{{missing}}".into();

        let engine = Canned::of(vec![ScriptRun::default(), ScriptRun::default()]);
        let outcome = send(job, &engine, &Recorder::default());

        assert!(outcome.result.is_err());
        assert_eq!(
            *engine.phases.lock().expect("lock"),
            vec![ScriptPhase::PreRequest],
            "there is no response for the hook to read"
        );
        assert!(!outcome.tests.ran);
    }

    #[test]
    fn a_request_with_no_post_response_script_reports_that_none_ran() {
        let (outcome, _) = run_job(job(ScriptJob::None), ScriptRun::default());
        assert!(!outcome.tests.ran);
        assert!(outcome.tests.is_empty());
    }

    /// End to end with the real engine, through the whole pipeline.
    #[test]
    fn the_shipped_assertion_template_passes_against_a_real_response() {
        let job = SendJob {
            pre: ScriptJob::None,
            post: running(
                "pm.test(\"Status is 200\", function () {\n\
                     pm.response.to.have.status(200);\n\
                 });\n\
                 pm.test(\"Body has an id\", function () {\n\
                     pm.expect(pm.response.json()).to.have.property(\"id\", 7);\n\
                 });\n\
                 pm.environment.set(\"id\", pm.response.json().id);",
            ),
            ..job(ScriptJob::None)
        };

        let outcome = send(job, &QuickJsEngine, &Recorder::answering("{\"id\":7}"));
        assert!(outcome.result.is_ok());
        assert!(outcome.tests.error.is_none(), "{:?}", outcome.tests.error);
        assert!(
            outcome.tests.summary().all_passed(),
            "{:?}",
            outcome.tests.results
        );
        assert_eq!(outcome.writes.len(), 1);
        assert_eq!(outcome.writes[0].value.as_deref(), Some("7"));
    }
}
