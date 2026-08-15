//! One sample per [`Text`] variant, for the language tests.
//!
//! `samples!` also emits an exhaustive `match` over [`Text`], so a variant
//! with no entry here is a compile error.

use crate::i18n::tests::{DETAIL, NUMBER, NUMBER_TEXT, Sample, plain, with};

use super::Text;

samples! {
    plain ScriptsSandboxNotice;
    plain PreRequestScriptLabel;
    plain PostResponseScriptLabel;
    plain Copy;
    plain BodyTruncated;
    plain InsertTemplate;
    plain TemplateSetHeader;
    plain TemplateSetBearerToken;
    plain TemplateSetTimestamp;
    plain TemplateAssertStatus;
    plain TemplateLogResponse;
    plain TemplateExtractField;
    with Threw(DETAIL.into()) [DETAIL];
    with Deadline(NUMBER as u64) [NUMBER_TEXT];
    plain OutOfMemory;
    with Unsupported(DETAIL.into()) [DETAIL];
    plain NoEngine;
    plain SkippedByPolicy;
    plain SkippedByConsent;
    plain ConsoleLevelDebug;
    plain ConsoleLevelLog;
    plain ConsoleLevelWarn;
    plain ConsoleLevelError;
    with ConsoleRunSeparator { run: NUMBER, summary: DETAIL.into() } [NUMBER_TEXT, DETAIL];
    plain ConsoleEmpty;
    plain ConsoleEmptyHint;
    plain ConsoleClear;
    with ConsoleDropped(NUMBER) [NUMBER_TEXT];
    plain RunScriptsNever;
    plain RunScriptsAskImported;
    plain RunScriptsAlways;
    plain ConsentTitle;
    plain ConsentExplain;
    with ConsentRequest(DETAIL.into()) [DETAIL];
    plain ConsentRun;
    plain ConsentSkip;
    with ConsentStoreError(DETAIL.into()) [DETAIL];
    plain ConsentStoreMissingVersion;
    with ConsentStoreUnsupportedVersion { found: NUMBER as u64, supported: 7 } [NUMBER_TEXT, "7"];
    plain ConsentExplainChanged;
    with SyntaxErrorAt { line: NUMBER, detail: DETAIL.into() } [NUMBER_TEXT, DETAIL];
    plain TestsNone;
    plain TestsNoneHint;
    plain TestsAddOne;
    plain TestsScriptDefinedNone;
    plain TestsScriptDefinedNoneHint;
    plain TestsNotRun;
    with TestsPassedCount(NUMBER) [NUMBER_TEXT];
    with TestsFailedCount(NUMBER) [NUMBER_TEXT];
    with TestsErroredCount(NUMBER) [NUMBER_TEXT];
    with TestsDropped(NUMBER) [NUMBER_TEXT];
}
