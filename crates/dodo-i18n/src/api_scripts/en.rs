//! The English column of the API Explorer's scripting.

use std::borrow::Cow;

use super::Text;

pub(crate) fn text(text: Text) -> Cow<'static, str> {
    match text {
        Text::ScriptsSandboxNotice => {
            "Both scripts run in a sandbox with no filesystem, no network and no modules. \
                 pm.sendRequest, require and setTimeout are not available."
                .into()
        }
        Text::PreRequestScriptLabel => "Pre-request script".into(),
        Text::PostResponseScriptLabel => "Post-response script".into(),
        Text::Copy => "Copy".into(),
        Text::BodyTruncated => "The body was too large and was cut short.".into(),
        Text::InsertTemplate => "Insert template".into(),
        Text::TemplateSetHeader => "Set a header".into(),
        Text::TemplateSetBearerToken => "Set a bearer token".into(),
        Text::TemplateSetTimestamp => "Set a timestamp variable".into(),
        Text::TemplateAssertStatus => "Assert status is 200".into(),
        Text::TemplateLogResponse => "Log the response body".into(),
        Text::TemplateExtractField => "Extract a JSON field".into(),
        Text::Threw(detail) => format!("The script failed: {detail}").into(),
        Text::Deadline(seconds) => {
            format!("The script did not finish within {seconds} s and was stopped.").into()
        }
        Text::OutOfMemory => "The script asked for more memory than one run is allowed.".into(),
        Text::Unsupported(name) => {
            format!("{name} is not supported in dodo, so this script cannot run.").into()
        }
        Text::NoEngine => "This build has no script engine, so nothing ran.".into(),
        Text::SkippedByPolicy => {
            "Scripts are switched off in Settings, so this one did not run.".into()
        }
        Text::SkippedByConsent => {
            "This imported script was not approved, so it did not run.".into()
        }
        Text::ConsoleLevelDebug => "Debug".into(),
        Text::ConsoleLevelLog => "Log".into(),
        Text::ConsoleLevelWarn => "Warn".into(),
        Text::ConsoleLevelError => "Error".into(),
        Text::ConsoleRunSeparator { run, summary } => format!("Run {run} · {summary}").into(),
        Text::ConsoleEmpty => "Nothing logged yet".into(),
        Text::ConsoleEmptyHint => {
            "console.log from a script appears here, and stays across sends.".into()
        }
        Text::ConsoleClear => "Clear".into(),
        Text::ConsoleDropped(count) => format!("{count} older lines dropped").into(),
        Text::RunScriptsNever => "Never".into(),
        Text::RunScriptsAskImported => "Ask for imported".into(),
        Text::RunScriptsAlways => "Always".into(),
        Text::ConsentTitle => "Run this imported script?".into(),
        Text::ConsentExplain => {
            "This script came from an imported collection and has not run before. Read it \
                 before approving: it can change this request and write your variables."
                .into()
        }
        Text::ConsentRequest(name) => format!("Request: {name}").into(),
        Text::ConsentRun => "Run script".into(),
        Text::ConsentSkip => "Send without it".into(),
        Text::ConsentStoreError(detail) => {
            format!("Could not read or write the script approvals: {detail}").into()
        }
        Text::ConsentStoreMissingVersion => {
            "The script approvals file carries no schema version, so it was not read.".into()
        }
        Text::ConsentStoreUnsupportedVersion { found, supported } => format!(
            "This script approvals file uses schema {found}; this build of dodo reads \
                     {supported}. Every imported script will ask again."
        )
        .into(),
        Text::ConsentExplainChanged => {
            "This imported script has changed since you approved it, so the earlier \
                 approval no longer applies. Read it again before approving: it can change \
                 this request and write your variables."
                .into()
        }
        Text::SyntaxErrorAt { line, detail } => format!("Line {line}: {detail}").into(),
        Text::TestsNone => "This request has no tests".into(),
        Text::TestsNoneHint => {
            "A post-response script can assert what came back with pm.test.".into()
        }
        Text::TestsAddOne => "Add a test".into(),
        Text::TestsScriptDefinedNone => "The script ran and defined no tests".into(),
        Text::TestsScriptDefinedNoneHint => "Anything it printed is in the Console.".into(),
        Text::TestsNotRun => "This request has a test script, but it did not run".into(),
        Text::TestsPassedCount(count) => format!("{count} passed").into(),
        Text::TestsFailedCount(count) => format!("{count} failed").into(),
        Text::TestsErroredCount(count) => format!("{count} errored").into(),
        Text::TestsDropped(count) => format!("{count} more results were dropped").into(),
    }
}
