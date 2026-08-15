//! What a `pm.test` produced, as plain data.
//!
//! Nothing here knows about an engine or about GPUI, so all of it is unit
//! testable — the same rule [`script`](crate::api_explorer::models::script)
//! follows. The types cross back from the background executor with the
//! response.
//!
//! # Where these attach, and where they must not
//!
//! A [`TestReport`] hangs off `state::response::ResponseState`, **never off
//! [`Exchange`]**. `Exchange` is documented as protocol-neutral — "a future
//! WebSocket or gRPC transport fills the same struct" — and a test result is not
//! something a server sent. `report.md` §7.2 flags putting them there as a
//! one-way door, and it is right: a scripting concept inside the protocol model
//! would have to be carried by every future transport.
//!
//! The second reason is more concrete. A *pre-request* script can define tests
//! and then the request can fail before any response exists, so there would be
//! no `Exchange` to hang them on. Beside the outcome is the only place that
//! renders both cases.
//!
//! # `Failed` and `Errored` are kept apart
//!
//! Postman paints both red. The distinction is the whole diagnostic value: a
//! failure says **the API is wrong** (an assertion looked and said no), an error
//! says **the script is wrong** (`TypeError: Cannot read property 'id' of
//! undefined`). Different message, different icon, different fix.
//!
//! [`Exchange`]: crate::api_explorer::models::exchange::Exchange

use std::time::Duration;

use crate::i18n::{Str, api_scripts};

/// Test results kept from a single run. A script can define them in a loop;
/// beyond this the rest are dropped and counted, never silently discarded —
/// the rule `api_scripts::Text::BodyTruncated` already follows.
pub const MAX_RESULTS: usize = 500;

/// Which hook a result came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScriptPhase {
    PreRequest,
    PostResponse,
}

impl ScriptPhase {
    pub const ALL: [ScriptPhase; 2] = [ScriptPhase::PreRequest, ScriptPhase::PostResponse];

    /// The name `pm.info.eventName` reports. Postman's own values, not
    /// translated: a script compares against them.
    pub fn event_name(self) -> &'static str {
        match self {
            ScriptPhase::PreRequest => "prerequest",
            ScriptPhase::PostResponse => "test",
        }
    }

    /// The heading the Tests tab groups by.
    pub fn label(self) -> Str {
        match self {
            ScriptPhase::PreRequest => api_scripts::Text::PreRequestScriptLabel.into(),
            ScriptPhase::PostResponse => api_scripts::Text::PostResponseScriptLabel.into(),
        }
    }
}

/// How one `pm.test` ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TestOutcome {
    Passed,
    /// An assertion said no. The message is the matcher's own wording
    /// (`expected 404 to equal 200`) and is script-shaped text, not a [`Str`] —
    /// there is nothing to translate it with.
    Failed {
        message: String,
    },
    /// The test threw something that was not an assertion.
    Errored {
        message: String,
    },
}

impl TestOutcome {
    /// The failure text, when there is one.
    pub fn message(&self) -> Option<&str> {
        match self {
            TestOutcome::Passed => None,
            TestOutcome::Failed { message } | TestOutcome::Errored { message } => Some(message),
        }
    }
}

/// One `pm.test` call.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TestResult {
    /// `pm.test`'s first argument, verbatim. User-authored script content, the
    /// same category as a header value — **not** a [`Str`].
    pub name: String,
    pub outcome: TestOutcome,
    pub elapsed: Duration,
    pub phase: ScriptPhase,
}

/// Passed / failed / errored, for a badge and a summary bar.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TestSummary {
    pub passed: usize,
    pub failed: usize,
    pub errored: usize,
}

impl TestSummary {
    pub fn of(results: &[TestResult]) -> Self {
        let mut summary = Self::default();
        for result in results {
            match result.outcome {
                TestOutcome::Passed => summary.passed += 1,
                TestOutcome::Failed { .. } => summary.failed += 1,
                TestOutcome::Errored { .. } => summary.errored += 1,
            }
        }
        summary
    }

    pub fn total(&self) -> usize {
        self.passed + self.failed + self.errored
    }

    pub fn is_empty(&self) -> bool {
        self.total() == 0
    }

    /// Whether every test that ran passed. An empty run is **not** "all passed"
    /// — a green badge over nothing is the lie `report.md` §3.4b is about.
    pub fn all_passed(&self) -> bool {
        !self.is_empty() && self.failed == 0 && self.errored == 0
    }

    /// The `3/4` badge: passed over total.
    pub fn badge(&self) -> String {
        format!("{}/{}", self.passed, self.total())
    }
}

/// Everything the Tests tab shows for the last send of one tab.
///
/// `Default` is "no send has finished here yet", which is a different state from
/// "a script ran and defined no tests" — see [`TestReport::ran`].
#[derive(Clone, Debug, Default)]
pub struct TestReport {
    pub results: Vec<TestResult>,
    /// The post-response script failed **outside** any `pm.test`: it did not
    /// parse, or it threw at the top level. Held as a [`Str`] so a report
    /// already on screen re-translates with the language.
    ///
    /// A *pre-request* failure never lands here: it stops the send, and the
    /// response pane shows the error banner instead of any tab.
    pub error: Option<Str>,
    /// Whether a post-response script ran at all. `false` with no results means
    /// "this request has no tests"; `true` with no results means "the script ran
    /// and defined none", and the two need different words.
    pub ran: bool,
    /// How long the post-response hook took, for the summary bar.
    pub elapsed: Duration,
    /// Results the cap dropped. Said out loud, never hidden.
    pub dropped: usize,
}

impl TestReport {
    pub fn summary(&self) -> TestSummary {
        TestSummary::of(&self.results)
    }

    /// Whether there is anything at all to show.
    pub fn is_empty(&self) -> bool {
        self.results.is_empty() && self.error.is_none()
    }

    /// Whether both hooks produced tests, which is the only case worth grouping
    /// by phase.
    pub fn spans_both_phases(&self) -> bool {
        self.results
            .iter()
            .any(|result| result.phase == ScriptPhase::PreRequest)
            && self
                .results
                .iter()
                .any(|result| result.phase == ScriptPhase::PostResponse)
    }

    /// The results of one hook, in the order the script defined them.
    pub fn phase(&self, phase: ScriptPhase) -> impl Iterator<Item = &TestResult> {
        self.results
            .iter()
            .filter(move |result| result.phase == phase)
    }
}

#[cfg(test)]
mod tests {
    use super::{ScriptPhase, TestOutcome, TestReport, TestResult, TestSummary};
    use crate::i18n::api_scripts;
    use std::time::Duration;

    fn result(name: &str, outcome: TestOutcome, phase: ScriptPhase) -> TestResult {
        TestResult {
            name: name.into(),
            outcome,
            elapsed: Duration::from_millis(1),
            phase,
        }
    }

    fn passed(name: &str) -> TestResult {
        result(name, TestOutcome::Passed, ScriptPhase::PostResponse)
    }

    fn failed(name: &str) -> TestResult {
        result(
            name,
            TestOutcome::Failed {
                message: "expected 404 to equal 200".into(),
            },
            ScriptPhase::PostResponse,
        )
    }

    #[test]
    fn a_summary_counts_the_three_outcomes_apart() {
        let summary = TestSummary::of(&[
            passed("a"),
            failed("b"),
            result(
                "c",
                TestOutcome::Errored {
                    message: "TypeError".into(),
                },
                ScriptPhase::PostResponse,
            ),
        ]);
        assert_eq!(summary.passed, 1);
        assert_eq!(summary.failed, 1);
        assert_eq!(summary.errored, 1);
        assert_eq!(summary.total(), 3);
        assert_eq!(summary.badge(), "1/3");
    }

    #[test]
    fn no_tests_is_not_all_passed() {
        // A green badge over nothing would say the API was checked when it was
        // not.
        assert!(!TestSummary::default().all_passed());
        assert!(TestSummary::of(&[passed("a")]).all_passed());
        assert!(!TestSummary::of(&[passed("a"), failed("b")]).all_passed());
    }

    #[test]
    fn a_report_tells_no_script_apart_from_a_script_that_defined_nothing() {
        let none = TestReport::default();
        assert!(!none.ran);
        assert!(none.is_empty());

        let ran = TestReport {
            ran: true,
            ..TestReport::default()
        };
        assert!(ran.ran);
        assert!(ran.is_empty(), "no results is still no results");
    }

    #[test]
    fn an_error_alone_makes_a_report_worth_showing() {
        let report = TestReport {
            error: Some(api_scripts::Text::OutOfMemory.into()),
            ran: true,
            ..TestReport::default()
        };
        assert!(!report.is_empty());
    }

    #[test]
    fn grouping_by_phase_only_applies_when_both_produced_tests() {
        let one_phase = TestReport {
            results: vec![passed("a"), failed("b")],
            ..TestReport::default()
        };
        assert!(!one_phase.spans_both_phases());

        let both = TestReport {
            results: vec![
                result("pre", TestOutcome::Passed, ScriptPhase::PreRequest),
                passed("post"),
            ],
            ..TestReport::default()
        };
        assert!(both.spans_both_phases());
        assert_eq!(both.phase(ScriptPhase::PreRequest).count(), 1);
        assert_eq!(both.phase(ScriptPhase::PostResponse).count(), 1);
    }

    #[test]
    fn the_two_hooks_keep_postmans_own_event_names() {
        // A script may compare against these, so they are values, not labels.
        assert_eq!(ScriptPhase::PreRequest.event_name(), "prerequest");
        assert_eq!(ScriptPhase::PostResponse.event_name(), "test");
    }
}
