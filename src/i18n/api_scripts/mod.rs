//! The API Explorer's scripting: the Scripts tab, the consent gate, the
//! engine's own failures, the Console and the Tests tab.
//!
//! `en` and `vi` each render every variant below; the compiler names any
//! string a language has not been given.

pub(crate) mod en;
pub(crate) mod vi;

#[cfg(test)]
pub(crate) mod samples;

/// The strings this area owns.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Text {
    // API Explorer — Scripts tab.
    /// The note at the top of the Scripts tab, saying what the engine will and
    /// will not do with what is typed below it.
    ScriptsSandboxNotice,
    PreRequestScriptLabel,
    PostResponseScriptLabel,
    Copy,
    BodyTruncated,

    // API Explorer — Scripts templates (phase 4).
    InsertTemplate,
    TemplateSetHeader,
    TemplateSetBearerToken,
    TemplateSetTimestamp,
    TemplateAssertStatus,
    TemplateLogResponse,
    TemplateExtractField,
    /// The script threw or did not parse. `detail` is the engine's own
    /// `TypeError: …`, third-party English kept verbatim inside a translated
    /// frame.
    Threw(String),
    /// "The script did not finish within {seconds} seconds and was stopped."
    Deadline(u64),
    OutOfMemory,
    /// "{name} is not supported in dodo." — the named failure that replaces an
    /// opaque `undefined is not a function`.
    Unsupported(String),
    NoEngine,
    SkippedByPolicy,
    SkippedByConsent,

    // API Explorer — the Console tab.
    ConsoleLevelDebug,
    ConsoleLevelLog,
    ConsoleLevelWarn,
    ConsoleLevelError,
    /// "Run {run} · {summary}" — the rule between two sends' output.
    ConsoleRunSeparator {
        run: usize,
        summary: String,
    },
    ConsoleEmpty,
    ConsoleEmptyHint,
    ConsoleClear,
    /// "{count} older lines dropped."
    ConsoleDropped(usize),
    RunScriptsNever,
    RunScriptsAskImported,
    RunScriptsAlways,
    ConsentTitle,
    ConsentExplain,
    /// "Request: {name}" above the script in the approval dialog.
    ConsentRequest(String),
    ConsentRun,
    ConsentSkip,
    /// The approvals file could not be read or written. `detail` as above.
    ConsentStoreError(String),
    ConsentStoreMissingVersion,
    /// "This approvals file was written by a newer dodo (schema {found}; this
    /// build reads {supported})."
    ConsentStoreUnsupportedVersion {
        found: u64,
        supported: u32,
    },
    /// Shown instead of [`Text::ConsentExplain`] when an approval already
    /// existed and an edit re-armed the gate. "Has not run before" is untrue
    /// there, and the prompt has to say what actually happened.
    ConsentExplainChanged,
    /// The strip under the editor header: which line, and what is wrong.
    SyntaxErrorAt {
        line: usize,
        detail: String,
    },
    /// The request carries no post-response script at all.
    TestsNone,
    TestsNoneHint,
    /// The button that opens the Scripts tab with an assertion inserted.
    TestsAddOne,
    /// There is a script, and it ran, and it defined no `pm.test`.
    TestsScriptDefinedNone,
    TestsScriptDefinedNoneHint,
    /// There is a script and it did not run — the consent gate or the setting.
    TestsNotRun,
    TestsPassedCount(usize),
    TestsFailedCount(usize),
    TestsErroredCount(usize),
    /// Results the per-run cap dropped. Said out loud, never hidden.
    TestsDropped(usize),
}
