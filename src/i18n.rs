//! A deliberately small localization mechanism: one enum per translatable
//! string, one column per language, and a global holding the active choice.
//!
//! Adding a string means adding a [`Str`] variant and its row in [`Str::text`];
//! adding a language means a [`Language`] variant, a row in [`Language::ALL`],
//! and a column in every `Str::text` row (the compiler lists the ones you
//! missed). No catalogue files, no runtime key lookup, no missing-key fallback
//! to get wrong.
//!
//! Messages that carry runtime values — a position, a count, a third-party
//! parser's own text — are [`Str`] variants with fields, so each language owns
//! the whole sentence and word order rather than a translated prefix glued onto
//! an English tail. Third-party error text (serde_json, base64, …) is English
//! and stays English inside the translated frame; there is nothing to translate
//! it with.

use std::borrow::Cow;

use gpui::{App, Global, SharedString};

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum Language {
    #[default]
    English,
    Vietnamese,
}

impl Global for Language {}

impl Language {
    pub const ALL: [Language; 2] = [Language::English, Language::Vietnamese];

    /// The stable identifier used as the settings dropdown value.
    pub fn code(self) -> &'static str {
        match self {
            Language::English => "en",
            Language::Vietnamese => "vi",
        }
    }

    pub fn from_code(code: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|language| language.code() == code)
            .unwrap_or_default()
    }

    /// The language's name in that language, as language pickers conventionally
    /// show it.
    pub fn label(self) -> &'static str {
        match self {
            Language::English => "English",
            Language::Vietnamese => "Tiếng Việt",
        }
    }

    /// The active language. Defaults to English until [`Language::set`] runs.
    pub fn current(cx: &App) -> Language {
        cx.try_global::<Language>().copied().unwrap_or_default()
    }

    /// Switches language and repaints every window so already-rendered strings
    /// pick the new column up.
    pub fn set(self, cx: &mut App) {
        cx.set_global(self);
        cx.refresh_windows();
    }
}

/// Which part of a JWT an error is about. Its own row per language so that a
/// new language has to say how it names the part, even if — as in Vietnamese —
/// the answer is to keep the English term of art.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JwtPart {
    Header,
    Payload,
}

impl JwtPart {
    /// The part's name as it reads mid-sentence.
    fn name(self, language: Language) -> &'static str {
        match (self, language) {
            (JwtPart::Header, Language::English) => "header",
            (JwtPart::Header, Language::Vietnamese) => "header",
            (JwtPart::Payload, Language::English) => "payload",
            (JwtPart::Payload, Language::Vietnamese) => "payload",
        }
    }
}

/// Every string this app localizes.
///
/// "Dodo" is the product name and is never translated, so it has no variant
/// here. Neither do the technical terms that stay put in both languages —
/// JSON, Base64, hex, JWT, URL — they appear inside the strings below.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Str {
    // Settings dialog.
    Settings,
    General,
    Appearance,
    Language,
    LanguageDescription,
    Theme,
    ThemeDescription,
    FontSize,
    FontSizeDescription,
    BorderRadius,
    BorderRadiusDescription,
    Large,
    Medium,
    Small,
    SearchSettingsPlaceholder,
    NoSettingsMatch,

    // Sidebar.
    Tools,
    JsonFormatterTitle,
    EncoderDecoderTitle,
    ApiExplorerTitle,
    CleanerTitle,

    // Cleaner.
    CleanerUnsupportedPlatform,
    CleanerScan,
    CleanerCancelScan,
    CleanerNoResultsYet,
    CleanerStatusScanning,
    CleanerStatusCancelling,
    CleanerStatusPartial,
    CleanerStatusCompleted,
    CleanerStatusCleaning,
    CleanerStatusFailed,
    CleanerSectionCleanup,
    CleanerSectionApplications,
    CleanerSectionAdvanced,
    CleanerCategorySystemJunk,
    CleanerCategoryUserCache,
    CleanerCategoryMailFiles,
    CleanerCategoryTrashBins,
    CleanerCategoryLargeOldFiles,
    CleanerCategoryInstalledApps,
    CleanerCategoryOrphanedFiles,
    CleanerCategoryAiApps,
    CleanerCategoryXcodeJunk,
    CleanerCategoryHomebrewCache,
    CleanerCategoryNodeToolingCache,
    CleanerCategoryDockerCache,
    CleanerCategoryUniversalBinaries,
    CleanerCategoryLanguageFiles,

    // JSON formatter.
    JsonPlaceholder,
    FormatButton,
    IndentLabel,
    /// "{count} spaces" — the indent-width dropdown options.
    IndentSpaces(usize),
    /// serde_json's message is third-party English and is kept verbatim.
    InvalidJson {
        line: usize,
        column: usize,
        detail: String,
    },

    // Encoder / decoder.
    FormatLabel,
    EncodeButton,
    DecodeButton,
    DecodeJwtButton,
    InputLabel,
    OutputLabel,
    JwtHeaderLabel,
    JwtPayloadLabel,
    JwtSignatureLabel,
    EncoderInputPlaceholder,
    EncoderOutputPlaceholder,
    FormatBase64,
    FormatBase64UrlSafe,
    FormatUrl,
    FormatHex,
    FormatJwt,

    // Encoder / decoder errors.
    JwtEncodeUnsupported,
    InvalidHexOddLength(usize),
    InvalidHexDigit {
        digit: char,
        position: usize,
    },
    /// base64's message is third-party English and is kept verbatim.
    InvalidBase64(String),
    InvalidPercentAt(usize),
    /// The UTF-8 error is third-party English and is kept verbatim.
    InvalidPercentEncoding(String),
    NotUtf8(String),
    JwtEmpty,
    JwtPartCount(usize),
    JwtPartNotBase64 {
        part: JwtPart,
        detail: String,
    },
    JwtPartNotJson {
        part: JwtPart,
        detail: String,
    },
    JwtPartNotRenderable {
        part: JwtPart,
        detail: String,
    },

    // API Explorer — collections panel.
    Collections,
    NoCollections,
    NoCollectionsHint,

    // API Explorer — request bar and tab strip.
    UrlPlaceholder,
    Send,
    NewRequest,
    CloseRequest,
    NameRequest,
    NameRequestPlaceholder,
    SaveName,
    /// The request bar's code-generation button, and the dialog's own title.
    GenerateCode,
    // API Explorer — request tabs.
    RequestTabParams,
    RequestTabHeaders,
    RequestTabBody,
    RequestTabAuth,
    RequestTabScripts,

    // API Explorer — key/value tables.
    ColumnKey,
    ColumnValue,
    Add,
    AddParameter,
    AddHeader,
    DeleteRow,
    NoActiveParams,
    /// "{count} active params" — the summary above the params table.
    ActiveParams(usize),
    NoActiveHeaders,
    /// "{count} active headers" — the summary above the headers table.
    ActiveHeaders(usize),
    ParamKeyPlaceholder,
    ParamValuePlaceholder,
    HeaderKeyPlaceholder,
    HeaderValuePlaceholder,
    ColumnDescription,
    DescriptionPlaceholder,
    DuplicateRow,
    MoveRowUp,
    MoveRowDown,
    AddField,
    NoActiveFields,
    /// "{count} active fields" — the summary above the form-body table.
    ActiveFields(usize),
    FieldKeyPlaceholder,
    FieldValuePlaceholder,

    // API Explorer — Body tab.
    BodyTypeNone,
    BodyTypeJson,
    BodyTypeText,
    BodyTypeXml,
    BodyTypeHtml,
    BodyTypeFormData,
    BodyTypeUrlEncoded,
    BodyTypeBinary,
    BodyPlaceholder,
    NoBodyTitle,
    NoBodyHint,
    BinaryBodyHint,
    /// "GET requests are sent without a body." The method is a wire token and
    /// is not translated; the sentence around it is.
    MethodSendsNoBody(String),

    // API Explorer — Auth tab.
    AuthTypeLabel,
    AuthTypeNone,
    AuthTypeBearer,
    AuthTypeBasic,
    AuthTypeApiKey,
    AuthTypeOAuth2,
    OAuth2Later,
    NoAuthTitle,
    NoAuthHint,
    AuthTokenLabel,
    AuthTokenPlaceholder,
    AuthUsernameLabel,
    AuthUsernamePlaceholder,
    AuthPasswordLabel,
    AuthPasswordPlaceholder,
    ApiKeyNameLabel,
    ApiKeyNamePlaceholder,
    ApiKeyValueLabel,
    ApiKeyValuePlaceholder,
    ApiKeySendAs,
    ApiKeyInHeader,
    ApiKeyInQuery,

    // API Explorer — Scripts tab.
    /// The note at the top of the Scripts tab, saying what the engine will and
    /// will not do with what is typed below it.
    ScriptsSandboxNotice,
    PreRequestScriptLabel,
    PreRequestScriptPlaceholder,
    PostResponseScriptLabel,
    PostResponseScriptPlaceholder,

    // API Explorer — response viewer.
    ResponseTabBody,
    ResponseTabHeaders,
    ResponseTabCookies,
    ResponseTabTests,
    ResponseTabConsole,
    NoResponseYet,
    NoResponseHint,
    Sending,
    RequestFailed,
    CollapseResponse,
    ExpandResponse,
    BodyPretty,
    BodyRaw,
    Copy,
    LoadMoreLines,
    BodyTruncated,
    /// "{shown} of {total} lines" — the response body footer.
    LineRange {
        shown: usize,
        total: usize,
    },

    // API Explorer — status classes.
    StatusClassInfo,
    StatusClassSuccess,
    StatusClassRedirect,
    StatusClassClientError,
    StatusClassServerError,
    StatusClassUnknown,

    // API Explorer — request failures.
    /// The URL parser's message is third-party English and is kept verbatim.
    HttpInvalidUrl(String),
    HttpUnsupportedScheme(String),
    HttpInvalidHeader(String),
    HttpTimeout(u64),
    HttpDnsFailure(String),
    /// The underlying error chain is third-party English and is kept verbatim.
    HttpConnectFailure(String),
    HttpTlsFailure(String),
    HttpBodyNotText(String),
    HttpUnexpected(String),

    // API Explorer — collections panel (phase 3).
    ImportCollection,
    NewCollection,
    NewFolder,
    SearchCollectionsPlaceholder,
    Rename,
    Delete,
    Duplicate,
    Open,
    MoreActions,
    NamePlaceholder,
    DefaultCollectionName,
    DefaultFolderName,
    SaveToCollectionNote,
    /// The store's own IO/serde message is third-party English, kept verbatim.
    CollectionStoreError(String),
    CollectionImportError(String),

    // API Explorer — request history (phase 3).
    History,
    NoHistory,
    NoHistoryHint,
    HistoryReopen,
    HistoryResend,
    HistoryClearAll,
    HistoryJustNow,
    /// "{minutes}m ago" — how long ago a request in the history ran.
    HistoryMinutesAgo(u64),
    HistoryHoursAgo(u64),
    HistoryDaysAgo(u64),

    // API Explorer — response viewer polish (phase 3).
    BodyPreview,
    BodyTree,
    SaveToFile,
    /// "Showing the first {count} nodes — collapse some to see the rest."
    JsonTreeTruncated(usize),
    HtmlPreviewNote,
    NoCookies,
    NoCookiesHint,

    // API Explorer — key/value table refinements (phase 4).
    ToggleAllRows,
    EditModeTable,
    EditModeBulk,
    BulkEditPlaceholder,

    // API Explorer — Scripts templates (phase 4).
    InsertTemplate,
    TemplateSetHeader,
    TemplateSetBearerToken,
    TemplateSetTimestamp,
    TemplateAssertStatus,
    TemplateLogResponse,
    TemplateExtractField,

    // Docker module — sidebar section and page names. These are Docker's own
    // resource types (and the product name), the same words in both languages we
    // ship, so they are terms of art like JSON/JWT above rather than prose.
    Docker,
    Containers,
    Images,
    Volumes,
    Networks,

    // Docker module — Containers toolbar.
    DockerSearchPlaceholder,
    DockerRefresh,
    DockerFilter,
    DockerCreate,

    // Docker module — Containers table columns.
    DockerColumnName,
    DockerColumnImage,
    DockerColumnStatus,
    DockerColumnCpu,
    DockerColumnPorts,
    DockerColumnLastStarted,
    DockerColumnActions,

    // Docker module — status badges.
    DockerStatusRunning,
    DockerStatusExited,
    DockerStatusCreated,
    DockerStatusRestarting,
    DockerStatusPaused,
    DockerStatusDead,
    DockerStatusRemoving,
    DockerStatusStopping,
    DockerStatusUnknown,

    // Docker module — per-row actions and the delete confirmation.
    DockerStart,
    DockerStop,
    DockerRestart,
    DockerDeleteTitle,
    /// "Permanently remove \"{name}\"? …" — the container name is user data.
    DockerDeleteMessage(String),
    DockerCancel,

    // Docker module — empty and error states.
    NoContainers,
    NoContainersHint,
    DockerRetry,
    /// bollard's own connection message is third-party English, kept verbatim.
    DockerConnectionError(String),
    /// bollard's own operation message is third-party English, kept verbatim.
    DockerOperationError(String),

    // Docker module — row selection.
    DockerSelectAll,
    DockerSelectRow,

    // Docker module — Last Started relative time.
    DockerRelNever,
    DockerRelJustNow,
    DockerRelSecondsAgo(u64),
    DockerRelMinutesAgo(u64),
    DockerRelHoursAgo(u64),
    DockerRelDaysAgo(u64),
    DockerRelWeeksAgo(u64),
    DockerRelMonthsAgo(u64),
    DockerRelYearsAgo(u64),

    // Docker module — error-state title (the detail follows below it).
    DockerUnreachableTitle,

    // Docker module (round 2) — compose grouping.
    DockerUngrouped,
    DockerGroupContainers(usize),
    DockerGroupRunning(usize),

    // Docker module (round 2) — the filter popover.
    DockerFilterWithCount(usize),
    DockerFilterTitle,
    DockerFilterProject,
    DockerFilterPublishedPorts,
    DockerFilterFavorites,
    DockerFilterClear,

    // Docker module (round 2) — bulk actions on the selection.
    DockerBulkSelected(usize),
    DockerBulkStart,
    DockerBulkStop,
    DockerBulkDelete,
    DockerBulkClear,
    DockerBulkDeleteTitle,
    DockerBulkDeleteMessage(usize),
    DockerBulkFailures(usize),

    // Docker module (round 3) — Images, Volumes and Networks pages: their extra
    // column headers, per-resource search placeholders, empty states and the
    // shared Inspect action / N/A / "<none>" tokens.
    DockerColumnRepository,
    DockerColumnTag,
    DockerColumnImageId,
    DockerColumnSize,
    DockerColumnCreated,
    DockerColumnContainersUsing,
    DockerColumnDriver,
    DockerColumnMountPoint,
    DockerColumnScope,
    DockerSearchImages,
    DockerSearchVolumes,
    DockerSearchNetworks,
    NoImages,
    NoImagesHint,
    NoVolumes,
    NoVolumesHint,
    NoNetworks,
    NoNetworksHint,
    DockerNotAvailable,
    DockerNone,
    DockerInspect,
    DockerNetworkPredefined,

    // Docker module (round 4) — right-click context-menu items for the container
    // detail views a later round fills in, and the section label that marks them
    // as not yet available.
    DockerViewLogs,
    DockerOpenTerminal,
    DockerComingSoonLabel,

    // Docker module (round 5) — the read-only Inspect panel and Logs viewer:
    // their chrome, and the field labels the Inspect field list uses that no
    // table column already names.
    DockerDetails,
    DockerRawJson,
    DockerDetailErrorTitle,
    DockerNoLogs,
    DockerNoLogsHint,
    DockerLogsTail(usize),
    DockerYes,
    DockerNo,
    DockerFieldId,
    DockerFieldCommand,
    DockerFieldStarted,
    DockerFieldExitCode,
    DockerFieldRestartPolicy,
    DockerFieldIpAddress,
    DockerFieldMounts,
    DockerFieldTags,
    DockerFieldDigest,
    DockerFieldArchitecture,
    DockerFieldOs,
    DockerFieldLayers,
    DockerFieldLabels,
    DockerFieldOptions,
    DockerFieldInternal,
    DockerFieldAttachable,
    DockerFieldSubnet,
    DockerFieldGateway,

    // Docker module (round 5) — the remaining "coming soon" placeholders, named
    // so they read as future features rather than broken controls.
    DockerPull,
    DockerBuild,
    DockerStats,

    // Docker module (round 6) — the tooltip on a row's identifying cell, which
    // is now the click target that opens the detail dialog. The dialog's own
    // tab labels reuse `DockerInspect` and `DockerViewLogs`.
    DockerOpenDetails,

    // Docker module (round 7) — the Runtimes tab: automatic detection of the
    // container runtimes/daemons on this machine plus Start/Stop. The tab
    // title is a term of art like the other three page names; row names reuse
    // `Docker` for the Docker row, and `DockerStart`/`DockerStop`/
    // `DockerRefresh`/`DockerOperationError` for the actions and their
    // failure, since those are exactly the same concepts already named above.
    Runtimes,
    RuntimesDescription,
    RuntimePodmanMachine,
    RuntimeKubernetes,
    RuntimeContainerd,
    RuntimeStatusRunning,
    RuntimeStatusStopped,
    RuntimeStatusNotInstalled,
    RuntimeStatusUnsupported,
    RuntimeStatusUnknown,
    RuntimeManagedExternally,
    RuntimeStarting,
    RuntimeStopping,
    RuntimeBinaryNotFound,
    RuntimeActionUnsupported,

    // API Explorer (round 7) — typed form rows, the binary body, and the tab
    // title. Appended rather than slotted in beside the strings they read next
    // to, for the reason the block above says: a new string must not renumber
    // every existing one.
    UntitledRequest,
    ColumnType,
    FieldKindText,
    FieldKindFile,
    ChooseFile,
    ReplaceFile,
    ClearFile,
    NoFileSelected,
    /// "{count} file fields have no file" — the warning above a form table
    /// holding rows that will be skipped.
    IncompleteFileFields(usize),
    /// "A file that no longer exists at {path} …" — a saved upload that has
    /// moved. `detail` is the operating system's own wording and stays in its
    /// own language, the convention this module's doc records.
    HttpFileUnreadable {
        path: String,
        detail: String,
    },
    /// "{path} is larger than the {limit_mb} MB this build will send."
    HttpFileTooLarge {
        path: String,
        limit_mb: u64,
    },

    // API Explorer (round 8) — variables and environments: the request-bar
    // picker, the editor dialog, the two scope names, the unencrypted-storage
    // notice, and the two new send-time failures.
    /// The picker's label for "resolve against no environment at all".
    NoEnvironment,
    /// The picker trigger's tooltip.
    SelectEnvironment,
    /// The row at the foot of the picker, and the dialog's own title.
    ManageEnvironments,
    Environments,
    NewEnvironment,
    DefaultEnvironmentName,
    /// Appended to a duplicated environment's name.
    EnvironmentCopySuffix,
    DuplicateEnvironment,
    DeleteEnvironment,
    ImportEnvironment,
    /// The two variable scopes, named in the editor's scope list and in the
    /// resolved-value preview.
    CollectionVariables,
    EnvironmentVariables,
    /// The wording under the collection scope, saying what it is for.
    CollectionVariablesNote,
    /// The empty state when no environment has been created yet.
    NoEnvironmentsYet,
    NoEnvironmentsYetHint,
    /// The variables table's own column and controls.
    ColumnSecret,
    AddVariable,
    NoActiveVariables,
    /// "{count} variables active" above the editor's table.
    ActiveVariables(usize),
    VariableKeyPlaceholder,
    VariableValuePlaceholder,
    MarkSecret,
    RevealSecret,
    HideSecret,
    /// The notice the editor shows about secret values. The captain's decision
    /// is that this is on screen, not only in the docs.
    SecretStorageWarning,
    /// The resolved-value preview under the request bar.
    ResolvedUrlLabel,
    /// "{name} is not defined" — the preview's wording for a missing variable,
    /// which is the same sentence the send-time failure uses.
    UnresolvedVariablePreview(String),
    /// The tooltip on the preview row, naming where a value came from.
    ResolvesFrom {
        name: String,
        scope: String,
    },
    /// "No variable named {name} is defined in this environment." — the send
    /// failure. Its own wording rather than a shared stem, because it is read
    /// in an error banner rather than beside the URL.
    HttpUnresolvedVariable(String),
    /// "{name} refers to itself."
    HttpRecursiveVariable(String),
    /// The environments file could not be read or written. `detail` is
    /// third-party English, kept verbatim inside a translated frame.
    VariableStoreError(String),
    VariableStoreMissingVersion,
    /// "This environments file was written by a newer dodo (schema {found};
    /// this build reads {supported})."
    VariableStoreUnsupportedVersion {
        found: u64,
        supported: u32,
    },
    /// An environment file could not be imported. `detail` as above.
    EnvironmentImportError(String),

    // API Explorer — the script engine.
    /// The precedence layer `pm.variables.set` writes into.
    ScriptVariables,
    /// The script threw or did not parse. `detail` is the engine's own
    /// `TypeError: …`, third-party English kept verbatim inside a translated
    /// frame.
    ScriptThrew(String),
    /// "The script did not finish within {seconds} seconds and was stopped."
    ScriptDeadline(u64),
    ScriptOutOfMemory,
    /// "{name} is not supported in dodo." — the named failure that replaces an
    /// opaque `undefined is not a function`.
    ScriptUnsupported(String),
    ScriptNoEngine,
    ScriptSkippedByPolicy,
    ScriptSkippedByConsent,
    /// "Pre-request script finished in {millis} ms."
    ScriptFinished {
        millis: u64,
    },
    /// "The script wrote {count} variables."
    ScriptWroteVariables(usize),
    /// "The script asked for method {method}, which dodo does not support."
    ScriptUnknownMethod(String),

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
    /// "{count} lines from this run were dropped."
    ConsoleRunTruncated(usize),
    ConsoleEmpty,
    ConsoleEmptyHint,
    ConsoleClear,
    /// "{count} older lines dropped."
    ConsoleDropped(usize),

    // API Explorer — the consent gate and its setting.
    RunScripts,
    RunScriptsDescription,
    RunScriptsNever,
    RunScriptsAskImported,
    RunScriptsAlways,
    ScriptConsentTitle,
    ScriptConsentExplain,
    /// "Request: {name}" above the script in the approval dialog.
    ScriptConsentRequest(String),
    ScriptConsentRun,
    ScriptConsentSkip,
    /// The approvals file could not be read or written. `detail` as above.
    ConsentStoreError(String),
    ConsentStoreMissingVersion,
    /// "This approvals file was written by a newer dodo (schema {found}; this
    /// build reads {supported})."
    ConsentStoreUnsupportedVersion {
        found: u64,
        supported: u32,
    },
    /// Shown instead of [`Str::ScriptConsentExplain`] when an approval already
    /// existed and an edit re-armed the gate. "Has not run before" is untrue
    /// there, and the prompt has to say what actually happened.
    ScriptConsentExplainChanged,

    // API Explorer — the script editors' syntax check.
    /// The wavy-underline message inside the editor. `detail` is QuickJS's own
    /// wording and stays English inside the translated frame.
    ScriptSyntaxError(String),
    /// The strip under the editor header: which line, and what is wrong.
    ScriptSyntaxErrorAt {
        line: usize,
        detail: String,
    },

    // API Explorer — the Tests tab.
    /// The Console line the post-response hook leaves, matching
    /// [`Str::ScriptFinished`] for the other hook.
    TestScriptFinished {
        millis: u64,
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

    // API Explorer — the Generate code dialog.
    /// The four target tabs. Each is the name of a tool or of a browser API, so
    /// each is the same word in both languages — declared with `term()`.
    CodeTargetCurl,
    CodeTargetFetch,
    CodeTargetAxios,
    CodeTargetXhr,
    /// The notice when nothing was withheld: the snippet holds the request's
    /// real values, whatever they are.
    GenerateCodeCarriesValues,
    /// The notice when secret variables were left as placeholders. `names` is
    /// the comma-separated list, and the sentence must also say what is *not*
    /// withheld — see `views::generate_code`.
    GenerateCodeSecretsWithheld(String),
    /// The notice once the toggle resolves them. Deliberately uncounted.
    GenerateCodeSecretsRevealed,
    /// The toggle itself.
    GenerateCodeRevealSecrets,

    // The in-app updater: the sidebar affordance and the dialog.
    CheckForUpdates,
    SoftwareUpdate,
    UpdateChecking,
    UpdateUpToDate,
    /// The version this binary is, shown under every verdict so the user can
    /// tell what they are comparing against.
    UpdateCurrentVersion(String),
    UpdateAvailableHeadline(String),
    /// The manifest's `published_at`, verbatim. An ISO-8601 UTC timestamp is
    /// the same characters in every language.
    UpdatePublished(String),
    UpdateDownloadSize(String),
    UpdateReleaseNotes,
    UpdateDownloadAction,
    /// Nothing is downloaded until the user presses the action above, so this
    /// only ever appears after an explicit agreement.
    UpdateDownloadProgress {
        done: String,
        total: String,
        percent: u8,
    },
    UpdateVerifying,
    UpdateInstalling,
    UpdateInstalledHeadline(String),
    UpdateRestartNow,
    UpdateLater,
    UpdateSkipVersion,
    UpdateCancel,
    UpdateRetry,
    UpdateCheckAutomatically,
    /// The install could not be done here. **Not a failure** — the archive is
    /// downloaded and verified, and this says where it is.
    UpdateManualInstall(String),
    UpdateManualNotABundle,
    UpdateManualNotWritable,
    UpdateManualReadOnly,
    UpdateFailedHeadline,
    /// `reqwest`'s own message is third-party English, kept verbatim inside a
    /// translated frame — the convention this module records.
    UpdateErrorNetwork(String),
    UpdateErrorManifestMalformed(String),
    UpdateErrorManifestMissingVersion,
    UpdateErrorManifestUnsupportedVersion {
        found: u64,
        supported: u32,
    },
    UpdateErrorManifestUnreadableVersion(String),
    /// Frames one of the three reasons below, which are written as sentence
    /// fragments so the whole message reads as one sentence in each language.
    /// Boxed because a `Str` cannot contain itself by value.
    UpdateErrorManifestInvalidFile {
        platform: String,
        detail: Box<Str>,
    },
    UpdateErrorManifestBadDigest(String),
    UpdateErrorManifestZeroSize,
    UpdateErrorManifestInsecureUrl(String),
    UpdateErrorPlatformMissing(String),
    UpdateErrorDownload(String),
    UpdateErrorChecksum {
        expected: String,
        actual: String,
    },
    UpdateErrorSize {
        expected: u64,
        actual: u64,
    },
    UpdateErrorInstall(String),
    UpdateErrorIo(String),

    // Database Explorer. Product names — PostgreSQL, SQLite — are proper nouns
    // and live in `database::models::engine`, untranslated, the same treatment
    // "Dodo" gets. Identifiers a server reports (a table's name, a column's
    // type) are data and never reach this enum at all.
    DatabaseTitle,
    DbConnections,
    DbNewConnection,
    DbNoConnections,
    DbNoConnectionsHint,
    DbConnect,
    DbDisconnect,
    DbReconnect,
    DbEditConnection,
    DbEditConnectionTitle,
    DbDuplicateConnection,
    DbDeleteConnection,
    /// Appended to a duplicated connection's name so the two are told apart.
    DbCopySuffix,
    DbStatusConnected,
    DbStatusConnecting,
    DbStatusDisconnected,
    DbStatusError,
    DbDeleteConnectionTitle,
    /// The connection's display name.
    DbDeleteConnectionMessage(String),
    DbCancel,
    DbSave,
    DbFieldName,
    DbFieldNamePlaceholder,
    DbFieldEngine,
    DbFieldHost,
    DbFieldPort,
    DbFieldDatabase,
    DbFieldUser,
    DbFieldUrl,
    DbFieldPassword,
    DbFieldFile,
    DbFieldFilePlaceholder,
    DbFieldSsl,
    DbSslDisable,
    DbSslPrefer,
    DbSslRequire,
    /// Never hidden while a password field is on screen. See
    /// `database::models::connection`'s module doc for why the posture is
    /// plaintext-and-say-so rather than an OS keychain.
    DbPasswordStorageNotice,
    DbRevealPassword,
    DbHidePassword,
    DbTestConnection,
    DbTesting,
    DbTestSucceeded,
    DbProfileHostMissing,
    DbProfilePortMissing,
    DbProfileDatabaseMissing,
    DbProfileFileMissing,
    DbGroupTables,
    DbGroupViews,
    DbGroupColumns,
    DbGroupIndexes,
    DbGroupConstraints,
    DbTreeLoading,
    DbTreeEmpty,
    DbTreeNotConnected,
    DbRefreshTree,
    DbQuery,
    DbQueryPlaceholder,
    DbExecute,
    DbFormat,
    DbRunning,
    DbNoStatement,
    DbResult,
    DbNoResultYet,
    DbNoResultYetHint,
    DbNoRows,
    /// How many rows the grid is holding.
    DbFooterRows(usize),
    /// How many rows a statement changed.
    DbFooterRowsAffected(u64),
    /// The elapsed time, already formatted — the unit is chosen by magnitude,
    /// so the value arrives as text rather than as a number plus a guess.
    DbFooterElapsed(String),
    /// The page bound stopped the read: how many rows are shown.
    DbFooterTruncated(usize),
    /// How many oversized cells were shortened to fit the page budget.
    DbFooterCapped(usize),
    DbStatementLabel,
    DbColumnNull,
    DbSelectConnection,
    DbSelectConnectionHint,
    DbConnectionStoreError(String),
    DbConnectionStoreMissingVersion,
    DbConnectionStoreUnsupportedVersion {
        found: u64,
        supported: u32,
    },
    /// The driver's own message, kept verbatim inside a translated frame.
    DbUnreachable(String),
    DbServerError(String),
    DbServerErrorCoded {
        code: String,
        detail: String,
    },

    // Round 2: query tabs.
    /// A tab's default title, numbered in the order tabs were opened.
    DbQueryTabTitle(usize),
    DbNewQueryTab,
    DbCloseQueryTab,

    // Round 2: cancelling a running statement, at the server.
    DbCancelQuery,
    /// What [`DbError::Cancelled`](crate::database::models::error::DbError)
    /// reads as wherever a driver error is shown.
    DbCancelledMessage,
    DbCancelledTitle,
    DbCancelledHint,
    /// dodo could not reach the server to *ask* it to stop. The driver's own
    /// words, kept verbatim inside a translated frame.
    DbCancelFailed(String),

    // Round 2: PostgreSQL's non-executing query plan.
    DbExplain,

    // Round 2: result-grid clipboard actions.
    DbCopyCell,
    DbCopyRow,

    // Round 2: full-result streaming export.
    DbExportCsv,
    DbExportJson,
    DbExportSucceeded {
        rows: usize,
        path: String,
    },
    DbExportCancelled,
    DbExportFailed(String),

    // Round 2: searchable in-session query history.
    DbHistory,
    DbHistorySearch,
    DbHistoryEmpty,
    DbHistoryNoMatches,

    // Database Explorer round 3: table and view detail.
    DbDetailData,
    DbDetailDdl,
    DbDetailFieldNullable,
    DbDetailFieldNotNull,
    DbDetailFieldDefault,
    DbDetailFieldUnique,
    DbDetailFieldPrimary,
    DbDetailFieldDefinition,
    DbDetailClose,
    DbDetailUnavailable,
    DbDetailNoRows,
    DbDetailNoMetadata,
    DbDetailPrevious,
    DbDetailNext,
    DbDetailPage(usize),
    DbDetailRowsRange {
        first: u64,
        last: u64,
    },
    DbDetailDdlReconstructed,
    DbDetailConstraintsPartial,
    DbDetailCopyDdl,
    DbDetailMetadataTruncated(usize),

    // Database Explorer round 4: non-SQL console and keyspace paging.
    DbProfileRedisDatabaseInvalid,
    DbGroupMore,
    DbCommandPlaceholder,

    // Database Explorer round 5: safe pending table-data mutations.
    DbEditUnsupported,
    DbEditNoColumns,
    DbEditMissingOrigin(String),
    DbEditMultipleTables,
    DbEditDuplicateColumn(String),
    DbEditNoUniqueIdentity(String),
    DbEditMissingIdentityColumns {
        table: String,
        columns: String,
    },
    DbEditMetadataFailed(String),
    DbEditCell,
    DbAddRow,
    DbDeleteRow,
    DbDuplicateRow,
    DbCommit,
    DbRollback,
    DbEditSelectRow,
    DbEditIdentityColumn,
    DbEditIdentityUnavailable,
    DbEditUnsupportedCell,
    DbEditNoPending,
    DbPendingChanges(usize),
    DbEditCellTitle(String),
    DbAddRowTitle,
    DbDuplicateRowTitle,
    DbSetNull,
    DbIdentityRequired(String),
    DbCommitTitle,
    DbCommitSummary(usize),
    DbCommitExactStatements,
    DbCommitParameters,
    DbCommitLostUpdateNotice,
    DbCommitRunning,
    DbCommitSucceeded(usize),
    DbCommitAffectedMismatch {
        statement: usize,
        actual: u64,
    },
    DbCommitFailed {
        statement: usize,
        detail: String,
    },
    DbCommitTransactionFailed(String),
    DbCommitStatementLabel(usize),
    DbExpectedOneRow,
    DbCommitBuildFailed,
    DbResolvePending,
    DbEditDuplicateRows,

    // Database Explorer round 6: saved queries and persisted history.
    DbQueryStoreError(String),
    DbQueryStoreMissingVersion,
    DbQueryStoreUnsupportedVersion {
        found: u64,
        supported: u32,
    },
    DbSavedQueries,
    DbSaveQuery,
    DbSavedQuerySearch,
    DbSavedQueryEmpty,
    DbSavedQueryNoMatches,
    DbSavedQueryCreateTitle,
    DbSavedQueryEditTitle,
    DbSavedQueryName,
    DbSavedQueryNamePlaceholder,
    DbSavedQueryStatement,
    DbSavedQueryScope,
    DbSavedQueryPlaintextNotice,
    DbSavedQueryNameRequired,
    DbSavedQueryStatementRequired,
    DbSavedQueryEdit,
    DbSavedQueryDelete,
    DbSavedQueryDeleteTitle,
    DbSavedQueryDeleteMessage(String),
    DbSavedQueryScopeMismatch(String),
    DbHistoryClear,
    DbHistoryClearTitle,
    DbHistoryClearMessage,
    DbHistorySucceeded,
    DbHistoryFailed,
    DbHistoryJustNow,
    DbHistoryMinutesAgo(u64),
    DbHistoryHoursAgo(u64),
    DbHistoryDaysAgo(u64),

    // Database Explorer round 6: bounded global catalog search.
    DbCatalogSearch,
    DbCatalogSearchPlaceholder,
    DbCatalogSearchLoading,
    DbCatalogSearchEmpty,
    DbCatalogSearchNoMatches,
    DbCatalogSearchConnectedOnly,
    DbCatalogSearchTruncated(usize),
    DbCatalogSearchPartial(usize),
    DbCatalogSearchConnectionUnavailable(String),
    DbCatalogKindDatabase,
    DbCatalogKindSchema,
    DbCatalogKindTable,
    DbCatalogKindView,
    DbCatalogKindColumn,
    DbCatalogKindIndex,
    DbCatalogKindConstraint,
    DbCatalogKindNamespace,
    DbCatalogKindKey,
    DbCatalogKindObject,

    // Database Explorer: filling the connection form from a pasted URI.
    DbFieldUri,
    DbFieldUriPlaceholder,
    DbFillFromUri,
    DbUriFilled,
    DbUriIgnored(String),
    DbUriTlsNotApplied,
    DbUriEmpty,
    DbUriNoScheme,
    DbUriUnknownScheme(String),
    DbUriInvalidPort(String),
    DbUriMissingFile,
    DbUriInvalidEscape,

    // Quick navigation: the settings page, and what a jump reports.
    QuickNavigation,
    QuickNavEnabled,
    QuickNavEnabledDescription,
    QuickNavCurlPattern,
    QuickNavDatabasePattern,
    QuickNavJwtPattern,
    QuickNavJsonPattern,
    QuickNavBase64Pattern,
    QuickNavGateDescription,
    QuickNavShapeDescription,
    QuickNavPatternInvalid(String),
    QuickNavPatternTooLong {
        length: usize,
        limit: usize,
    },
    QuickNavStorageProblem,
    QuickNavStoreError(String),
    QuickNavStoreMissingVersion,
    QuickNavStoreUnsupportedVersion {
        found: u64,
        understood: u32,
    },
    QuickNavOpenedConnection(String),
    QuickNavKeptStoredPassword(String),
    QuickNavCreatedConnection(String),
    QuickNavConnectionsLoading,
    CleanerWarnings,
    CleanerPath,
    CleanerExplanation,
    CleanerCopyPath,
    CleanerRevealInFinder,
    /// Only reachable from `results_table::reveal_label` on a Windows build
    /// (its Windows `#[cfg]` arm). The blocking `clippy` CI job runs on
    /// `macos-15`, where cfg strips that arm and this variant is never
    /// constructed at all — dead code on this platform's build, live on
    /// Windows'. Comes off if `reveal_label` ever stops being cfg'd per
    /// platform.
    #[allow(dead_code)]
    CleanerRevealInExplorer,
    /// See `CleanerRevealInExplorer` — same reasoning, for Linux's arm.
    #[allow(dead_code)]
    CleanerRevealInFileManager,
    CleanerMoreActions,
    CleanerColumnName,
    CleanerColumnRisk,
    CleanerColumnSize,
    CleanerColumnActions,
    CleanerRiskSafe,
    CleanerRiskReview,
    CleanerRiskUserData,
    CleanerRiskAppChange,
    CleanerRiskProtected,
    CleanerSelectItem,
    CleanerDeselectItem,
    CleanerSelectSafeItems,
    CleanerCleanSelected,
    CleanerCleanupReport,
    CleanerCleanupConfirmTitle,
    CleanerCleanupConfirmMessage {
        count: usize,
        size: String,
    },
    CleanerCleanupSuccessCount(usize),
    CleanerCleanupFailureCount(usize),
    CleanerPermissionTitle,
    CleanerPermissionExplanation,
    CleanerPermissionOpenSettings,
    CleanerPartialPermissionDenied,
    CleanerPartialRootUnavailable,
    CleanerPartialCancelled,
    CleanerPartialUnsupported,
    CleanerBeginUninstallReview,
    CleanerUninstallReviewTitle {
        name: String,
    },
    CleanerUninstallLoading,
    CleanerUninstallRefusedProtected,
    CleanerUninstallRefusedNotApplication,
    CleanerUninstallRelatedFilesHeader,
    CleanerUninstallNoRelatedFiles,
    CleanerUninstallDestinationNote,
    CleanerUninstallScanOnlyBadge,
    CleanerUninstallMoveToTrash,
    CleanerUninstallClose,
    CleanerConfidenceConfirmed,
    CleanerConfidenceHigh,
    CleanerConfidenceMedium,
    CleanerConfidenceLow,
    CleanerConfidenceSharedOrUnsafe,
    CleanerKeepItem,
    CleanerIgnoreStoreError(String),
    CleanerIgnoreStoreMissingVersion,
    CleanerIgnoreStoreUnsupportedVersion {
        found: u64,
        understood: u32,
    },
    CleanerDockerCleanupConfirmTitle,
    CleanerDockerCleanupConfirmMessage {
        count: usize,
        size: String,
    },

    // Session restoration: what `session.json` can go wrong with. Appended
    // rather than filed beside the Settings block above because `position` is
    // a fixed numbering and renumbering it would touch every variant.
    SessionStorageProblem,
    SessionStoreError(String),
    SessionStoreMissingVersion,
    SessionStoreUnsupportedVersion {
        found: u64,
        understood: u32,
    },

    // The Features settings page: which tools the sidebar lists, and in what
    // order. Appended for the same reason as the block above — `position` is a
    // fixed numbering.
    Features,
    FeaturesDescription,
    FeatureShowInSidebar,
    FeatureDragToReorder,
    FeatureMoveUp,
    FeatureMoveDown,
    FeatureLastVisibleTool,

    // The macOS menu bar item. Appended for the same reason as the two blocks
    // above — `position` is a fixed numbering.
    //
    // The **input languages themselves are not here**: their names are endonyms
    // shown in their own language, so they never enter this mechanism. See
    // `tray::menu::label`.
    TrayOpenDodo,
    TrayKeyboardInput,
    TrayQuitDodo,

    // The macOS input method — installing it, and the settings dodo writes for
    // it. Appended for the same reason as the blocks above: `position` is a fixed
    // numbering.
    //
    // These are **dodo's** strings, not the bundle's. The input method's own two
    // user-visible strings — its name and its mode's — are read by *macOS* out of
    // `InfoPlist.strings` and deliberately do not go through this mechanism; see
    // `docs/macos-input-method.md` §10.
    InputMethod,
    InputMethodDescription,
    InputMethodInstall,
    InputMethodReinstall,
    InputMethodInstalling,
    InputMethodInstalled,
    /// "Installed, but macOS would not switch to it (error {0})." The number is
    /// an `OSStatus`, shown because `-50` is the one a reader can look up.
    InputMethodInstalledNotActive(i32),
    InputMethodNoBundle,
    /// `ditto`'s own message, kept verbatim inside a translated frame.
    InputMethodCopyFailed(String),
    /// `codesign`'s own message, kept verbatim inside a translated frame.
    InputMethodInvalidSignature(String),
    /// How many `TISRegisterInputSource` calls were made before giving up.
    InputMethodNeverAppeared(u32),
    InputMethodStatus,
    InputMethodNotInstalled,
    /// The installed bundle's version, as the bundle itself reported it.
    InputMethodRunning(String),
    InputMethodInstalledIdle,
    InputMethodSettingsPending,
    InputMethodStorageProblem,
    InputMethodStoreError(String),
    InputMethodStoreMissingVersion,
    InputMethodStoreUnsupportedVersion {
        found: u64,
        supported: u32,
    },
    InputMethodScheme,
    InputMethodSchemeDescription,
    InputMethodTelex,
    InputMethodVni,
    InputMethodTonePlacement,
    InputMethodTonePlacementDescription,
    InputMethodToneModern,
    InputMethodToneTraditional,
    InputMethodSpellCheck,
    InputMethodSpellCheckDescription,
    InputMethodBracketShortcuts,
    InputMethodBracketShortcutsDescription,
    InputMethodBackend,
    InputMethodBackendDescription,
    InputMethodNative,
    InputMethodEventTap,
    InputMethodEventTapStatus,
    InputMethodEventTapInactive,
    InputMethodEventTapWaitingForNative,
    InputMethodEventTapNeedsAccessibility,
    InputMethodEventTapRunning,
    InputMethodEventTapFailed,

    // Windows input method. Kept separate from the macOS backend names: TSF
    // installation and the no-install Keyboard Hook have different promises.
    #[cfg_attr(
        not(target_os = "windows"),
        allow(dead_code, reason = "Windows-only input-method copy.")
    )]
    InputMethodWindowsDescription,
    #[cfg_attr(
        not(target_os = "windows"),
        allow(dead_code, reason = "Windows-only input-method copy.")
    )]
    InputMethodWindowsLanguageDescription,
    #[cfg_attr(
        not(target_os = "windows"),
        allow(dead_code, reason = "Windows-only input-method copy.")
    )]
    InputMethodNativeTsf,
    #[cfg_attr(
        not(target_os = "windows"),
        allow(dead_code, reason = "Windows-only input-method copy.")
    )]
    InputMethodWindowsTsfStatus,
    #[cfg_attr(
        not(target_os = "windows"),
        allow(dead_code, reason = "Windows-only input-method copy.")
    )]
    InputMethodWindowsTsfNotInstalled,
    #[cfg_attr(
        not(target_os = "windows"),
        allow(dead_code, reason = "Windows-only input-method copy.")
    )]
    InputMethodWindowsTsfInstalled,
    #[cfg_attr(
        not(target_os = "windows"),
        allow(dead_code, reason = "Windows-only input-method copy.")
    )]
    InputMethodWindowsTsfRemoved,
    #[cfg_attr(
        not(target_os = "windows"),
        allow(dead_code, reason = "Windows-only input-method copy.")
    )]
    InputMethodWindowsTsfNoDll,
    #[cfg_attr(
        not(target_os = "windows"),
        allow(dead_code, reason = "Windows-only input-method copy.")
    )]
    InputMethodWindowsTsfRegisterFailed(String),
    #[cfg_attr(
        not(target_os = "windows"),
        allow(dead_code, reason = "Windows-only input-method copy.")
    )]
    InputMethodWindowsTsfUnregisterFailed(String),
    #[cfg_attr(
        not(target_os = "windows"),
        allow(dead_code, reason = "Windows-only input-method copy.")
    )]
    InputMethodKeyboardHook,
    #[cfg_attr(
        not(target_os = "windows"),
        allow(dead_code, reason = "Windows-only input-method copy.")
    )]
    InputMethodKeyboardHookStatus,
    #[cfg_attr(
        not(target_os = "windows"),
        allow(dead_code, reason = "Windows-only input-method copy.")
    )]
    InputMethodKeyboardHookInactive,
    #[cfg_attr(
        not(target_os = "windows"),
        allow(dead_code, reason = "Windows-only input-method copy.")
    )]
    InputMethodKeyboardHookRunning,
    #[cfg_attr(
        not(target_os = "windows"),
        allow(dead_code, reason = "Windows-only input-method copy.")
    )]
    InputMethodKeyboardHookFailed,
    #[cfg_attr(
        not(target_os = "windows"),
        allow(dead_code, reason = "Windows-only input-method copy.")
    )]
    InputMethodUninstall,
    #[cfg_attr(
        not(target_os = "windows"),
        allow(dead_code, reason = "Windows-only input-method copy.")
    )]
    InputMethodUninstalling,

    // Cleaner UX/state refactor — appended rather than slotted in beside the
    // strings they replace, so no existing `position()` slot is renumbered.
    CleanerScanDescription,
    CleanerEntriesScannedCount(u64),
    CleanerBytesDiscovered(String),
    CleanerReclaimableAmount(String),
    CleanerItemsFound(usize),
    CleanerSafeItemsCount(usize),
    CleanerWarningCount(usize),
    CleanerSelectedSummary {
        count: usize,
        size: String,
    },
    CleanerCleanCount {
        count: usize,
        size: String,
    },
    CleanerScanWarningsSummary(usize),
    CleanerScanWarningsShowDetails,
    CleanerScanWarningsHideDetails,
    CleanerRescan,
    CleanerSelectAll,
    CleanerDeselectAll,
    CleanerPermissionNotNow,
    CleanerStatusCompletedWithWarnings,
    CleanerStatusCancelled,

    // Close-to-tray and OS startup. Appended rather than slotted into Settings
    // so `position()` stays a fixed compatibility table.
    StartWithOs,
    StartWithOsDescription,
}

impl Str {
    /// The string in one language.
    ///
    /// `pub(crate)` rather than private because a [`Str`] can now be *held*
    /// rather than rendered on the spot — a `ConsoleEntry` keeps dodo's own
    /// lines unrendered so they re-translate — and those holders have to be
    /// testable without a `Window`. Views still go through [`t`].
    pub(crate) fn text(self, language: Language) -> Cow<'static, str> {
        match (self, language) {
            (Str::Settings, Language::English) => "Settings".into(),
            (Str::Settings, Language::Vietnamese) => "Cài đặt".into(),
            (Str::General, Language::English) => "General".into(),
            (Str::General, Language::Vietnamese) => "Chung".into(),
            (Str::Appearance, Language::English) => "Appearance".into(),
            (Str::Appearance, Language::Vietnamese) => "Giao diện".into(),
            (Str::Language, Language::English) => "Language".into(),
            (Str::Language, Language::Vietnamese) => "Ngôn ngữ".into(),
            (Str::LanguageDescription, Language::English) => {
                "The language used for the app's own labels.".into()
            }
            (Str::LanguageDescription, Language::Vietnamese) => {
                "Ngôn ngữ dùng cho các nhãn của ứng dụng.".into()
            }
            (Str::Theme, Language::English) => "Theme".into(),
            (Str::Theme, Language::Vietnamese) => "Chủ đề".into(),
            (Str::ThemeDescription, Language::English) => {
                "The colour scheme of the whole app.".into()
            }
            (Str::ThemeDescription, Language::Vietnamese) => "Bảng màu của toàn bộ ứng dụng.".into(),
            (Str::FontSize, Language::English) => "Font size".into(),
            (Str::FontSize, Language::Vietnamese) => "Cỡ chữ".into(),
            (Str::FontSizeDescription, Language::English) => "The base text size of the app.".into(),
            (Str::FontSizeDescription, Language::Vietnamese) => {
                "Cỡ chữ cơ bản của ứng dụng.".into()
            }
            (Str::BorderRadius, Language::English) => "Border radius".into(),
            (Str::BorderRadius, Language::Vietnamese) => "Bo góc".into(),
            (Str::BorderRadiusDescription, Language::English) => {
                "How rounded buttons, inputs and panels are.".into()
            }
            (Str::BorderRadiusDescription, Language::Vietnamese) => {
                "Độ bo góc của nút, ô nhập và khung.".into()
            }
            (Str::Large, Language::English) => "Large".into(),
            (Str::Large, Language::Vietnamese) => "Lớn".into(),
            (Str::Medium, Language::English) => "Medium".into(),
            (Str::Medium, Language::Vietnamese) => "Vừa".into(),
            (Str::Small, Language::English) => "Small".into(),
            (Str::Small, Language::Vietnamese) => "Nhỏ".into(),
            (Str::SearchSettingsPlaceholder, Language::English) => {
                "Search settings, then press Enter to jump".into()
            }
            (Str::SearchSettingsPlaceholder, Language::Vietnamese) => {
                "Tìm cài đặt, rồi nhấn Enter để chuyển tới".into()
            }
            (Str::NoSettingsMatch, Language::English) => "No setting matches that search.".into(),
            (Str::NoSettingsMatch, Language::Vietnamese) => {
                "Không có cài đặt nào khớp với tìm kiếm đó.".into()
            }

            (Str::Tools, Language::English) => "Tools".into(),
            (Str::Tools, Language::Vietnamese) => "Công cụ".into(),
            (Str::JsonFormatterTitle, Language::English) => "Json formatter".into(),
            (Str::JsonFormatterTitle, Language::Vietnamese) => "Định dạng JSON".into(),
            (Str::EncoderDecoderTitle, Language::English) => "Encoder / Decoder".into(),
            (Str::EncoderDecoderTitle, Language::Vietnamese) => "Mã hoá / Giải mã".into(),
            (Str::ApiExplorerTitle, Language::English) => "API Explorer".into(),
            (Str::ApiExplorerTitle, Language::Vietnamese) => "Khám phá API".into(),
            (Str::CleanerTitle, Language::English) => "Cleaner".into(),
            (Str::CleanerTitle, Language::Vietnamese) => "Dọn dẹp".into(),

            (Str::CleanerUnsupportedPlatform, Language::English) => {
                "Cleaner is currently available on macOS. Windows and Linux support will be added in future versions.".into()
            }
            (Str::CleanerUnsupportedPlatform, Language::Vietnamese) => {
                "Cleaner hiện chỉ có trên macOS. Hỗ trợ Windows và Linux sẽ được bổ sung ở các phiên bản sau.".into()
            }
            (Str::CleanerScan, Language::English) => "Scan".into(),
            (Str::CleanerScan, Language::Vietnamese) => "Quét".into(),
            (Str::CleanerCancelScan, Language::English) => "Cancel".into(),
            (Str::CleanerCancelScan, Language::Vietnamese) => "Huỷ".into(),
            (Str::CleanerNoResultsYet, Language::English) => {
                "No scan results for this category yet.".into()
            }
            (Str::CleanerNoResultsYet, Language::Vietnamese) => {
                "Chưa có kết quả quét cho danh mục này.".into()
            }
            (Str::CleanerStatusScanning, Language::English) => "Scanning".into(),
            (Str::CleanerStatusScanning, Language::Vietnamese) => "Đang quét".into(),
            (Str::CleanerStatusCancelling, Language::English) => "Cancelling".into(),
            (Str::CleanerStatusCancelling, Language::Vietnamese) => "Đang huỷ".into(),
            (Str::CleanerStatusPartial, Language::English) => "Partially completed".into(),
            (Str::CleanerStatusPartial, Language::Vietnamese) => "Hoàn tất một phần".into(),
            (Str::CleanerStatusCompleted, Language::English) => "Completed".into(),
            (Str::CleanerStatusCompleted, Language::Vietnamese) => "Hoàn tất".into(),
            (Str::CleanerStatusCleaning, Language::English) => "Cleaning".into(),
            (Str::CleanerStatusCleaning, Language::Vietnamese) => "Đang dọn dẹp".into(),
            (Str::CleanerStatusFailed, Language::English) => "Failed".into(),
            (Str::CleanerStatusFailed, Language::Vietnamese) => "Thất bại".into(),
            (Str::CleanerSectionCleanup, Language::English) => "Cleanup".into(),
            (Str::CleanerSectionCleanup, Language::Vietnamese) => "Dọn dẹp".into(),
            (Str::CleanerSectionApplications, Language::English) => "Applications".into(),
            (Str::CleanerSectionApplications, Language::Vietnamese) => "Ứng dụng".into(),
            (Str::CleanerSectionAdvanced, Language::English) => "Advanced".into(),
            (Str::CleanerSectionAdvanced, Language::Vietnamese) => "Nâng cao".into(),
            (Str::CleanerCategorySystemJunk, Language::English) => "System Junk".into(),
            (Str::CleanerCategorySystemJunk, Language::Vietnamese) => "Rác hệ thống".into(),
            (Str::CleanerCategoryUserCache, Language::English) => "User Cache".into(),
            (Str::CleanerCategoryUserCache, Language::Vietnamese) => "Bộ đệm người dùng".into(),
            (Str::CleanerCategoryMailFiles, Language::English) => "Mail Files".into(),
            (Str::CleanerCategoryMailFiles, Language::Vietnamese) => "Tệp Mail".into(),
            (Str::CleanerCategoryTrashBins, Language::English) => "Trash Bins".into(),
            (Str::CleanerCategoryTrashBins, Language::Vietnamese) => "Thùng rác".into(),
            (Str::CleanerCategoryLargeOldFiles, Language::English) => "Large & Old Files".into(),
            (Str::CleanerCategoryLargeOldFiles, Language::Vietnamese) => {
                "Tệp lớn & cũ".into()
            }
            (Str::CleanerCategoryInstalledApps, Language::English) => "Installed Apps".into(),
            (Str::CleanerCategoryInstalledApps, Language::Vietnamese) => {
                "Ứng dụng đã cài".into()
            }
            (Str::CleanerCategoryOrphanedFiles, Language::English) => "Orphaned Files".into(),
            (Str::CleanerCategoryOrphanedFiles, Language::Vietnamese) => "Tệp mồ côi".into(),
            (Str::CleanerCategoryAiApps, Language::English) => "AI Apps".into(),
            (Str::CleanerCategoryAiApps, Language::Vietnamese) => "Ứng dụng AI".into(),
            (Str::CleanerCategoryXcodeJunk, Language::English) => "Xcode Junk".into(),
            (Str::CleanerCategoryXcodeJunk, Language::Vietnamese) => "Rác Xcode".into(),
            (Str::CleanerCategoryHomebrewCache, Language::English) => "Homebrew Cache".into(),
            (Str::CleanerCategoryHomebrewCache, Language::Vietnamese) => {
                "Bộ đệm Homebrew".into()
            }
            (Str::CleanerCategoryNodeToolingCache, Language::English) => {
                "Node Tooling Cache".into()
            }
            (Str::CleanerCategoryNodeToolingCache, Language::Vietnamese) => {
                "Bộ đệm công cụ Node".into()
            }
            (Str::CleanerCategoryDockerCache, Language::English) => "Docker Cache".into(),
            (Str::CleanerCategoryDockerCache, Language::Vietnamese) => "Bộ đệm Docker".into(),
            (Str::CleanerCategoryUniversalBinaries, Language::English) => {
                "Universal Binaries".into()
            }
            (Str::CleanerCategoryUniversalBinaries, Language::Vietnamese) => {
                "Universal Binary".into()
            }
            (Str::CleanerCategoryLanguageFiles, Language::English) => "Language Files".into(),
            (Str::CleanerCategoryLanguageFiles, Language::Vietnamese) => {
                "Tệp ngôn ngữ".into()
            }
            (Str::CleanerWarnings, Language::English) => "Warnings".into(),
            (Str::CleanerWarnings, Language::Vietnamese) => "Cảnh báo".into(),
            (Str::CleanerPath, Language::English) => "Path".into(),
            (Str::CleanerPath, Language::Vietnamese) => "Đường dẫn".into(),
            (Str::CleanerExplanation, Language::English) => "Explanation".into(),
            (Str::CleanerExplanation, Language::Vietnamese) => "Giải thích".into(),
            (Str::CleanerCopyPath, Language::English) => "Copy path".into(),
            (Str::CleanerCopyPath, Language::Vietnamese) => "Sao chép đường dẫn".into(),
            (Str::CleanerRevealInFinder, Language::English) => "Reveal in Finder".into(),
            (Str::CleanerRevealInFinder, Language::Vietnamese) => {
                "Hiện trong Finder".into()
            }
            (Str::CleanerRevealInExplorer, Language::English) => "Reveal in Explorer".into(),
            (Str::CleanerRevealInExplorer, Language::Vietnamese) => {
                "Hiện trong Explorer".into()
            }
            (Str::CleanerRevealInFileManager, Language::English) => {
                "Reveal in file manager".into()
            }
            (Str::CleanerRevealInFileManager, Language::Vietnamese) => {
                "Hiện trong trình quản lý tệp".into()
            }
            (Str::CleanerMoreActions, Language::English) => "More actions".into(),
            (Str::CleanerMoreActions, Language::Vietnamese) => "Thêm hành động".into(),
            (Str::CleanerColumnName, Language::English) => "Name".into(),
            (Str::CleanerColumnName, Language::Vietnamese) => "Tên".into(),
            (Str::CleanerColumnRisk, Language::English) => "Risk".into(),
            (Str::CleanerColumnRisk, Language::Vietnamese) => "Rủi ro".into(),
            (Str::CleanerColumnSize, Language::English) => "Size".into(),
            (Str::CleanerColumnSize, Language::Vietnamese) => "Kích thước".into(),
            (Str::CleanerColumnActions, Language::English) => "Actions".into(),
            (Str::CleanerColumnActions, Language::Vietnamese) => "Hành động".into(),
            (Str::CleanerRiskSafe, Language::English) => "Safe".into(),
            (Str::CleanerRiskSafe, Language::Vietnamese) => "An toàn".into(),
            (Str::CleanerRiskReview, Language::English) => "Review".into(),
            (Str::CleanerRiskReview, Language::Vietnamese) => "Cần xem lại".into(),
            (Str::CleanerRiskUserData, Language::English) => "User Data".into(),
            (Str::CleanerRiskUserData, Language::Vietnamese) => "Dữ liệu người dùng".into(),
            (Str::CleanerRiskAppChange, Language::English) => "App Change".into(),
            (Str::CleanerRiskAppChange, Language::Vietnamese) => "Thay đổi ứng dụng".into(),
            (Str::CleanerRiskProtected, Language::English) => "Protected".into(),
            (Str::CleanerRiskProtected, Language::Vietnamese) => "Được bảo vệ".into(),
            (Str::CleanerSelectItem, Language::English) => "Select".into(),
            (Str::CleanerSelectItem, Language::Vietnamese) => "Chọn".into(),
            (Str::CleanerDeselectItem, Language::English) => "Deselect".into(),
            (Str::CleanerDeselectItem, Language::Vietnamese) => "Bỏ chọn".into(),
            (Str::CleanerSelectSafeItems, Language::English) => "Select safe items".into(),
            (Str::CleanerSelectSafeItems, Language::Vietnamese) => {
                "Chọn mục an toàn".into()
            }
            (Str::CleanerCleanSelected, Language::English) => "Clean selected".into(),
            (Str::CleanerCleanSelected, Language::Vietnamese) => {
                "Dọn mục đã chọn".into()
            }
            (Str::CleanerCleanupReport, Language::English) => "Cleanup report".into(),
            (Str::CleanerCleanupReport, Language::Vietnamese) => {
                "Báo cáo dọn dẹp".into()
            }
            (Str::CleanerCleanupConfirmTitle, Language::English) => {
                "Move selected items to Trash?".into()
            }
            (Str::CleanerCleanupConfirmTitle, Language::Vietnamese) => {
                "Chuyển các mục đã chọn vào Thùng rác?".into()
            }
            (
                Str::CleanerCleanupConfirmMessage { count, size },
                Language::English,
            ) => format!("{count} items will be moved to the macOS Trash. Estimated size: {size}.").into(),
            (
                Str::CleanerCleanupConfirmMessage { count, size },
                Language::Vietnamese,
            ) => format!("{count} mục sẽ được chuyển vào Thùng rác của macOS. Dung lượng ước tính: {size}.").into(),
            (Str::CleanerCleanupSuccessCount(count), Language::English) => {
                format!("Moved to Trash: {count}").into()
            }
            (Str::CleanerCleanupSuccessCount(count), Language::Vietnamese) => {
                format!("Đã chuyển vào Thùng rác: {count}").into()
            }
            (Str::CleanerCleanupFailureCount(count), Language::English) => {
                format!("Failed: {count}").into()
            }
            (Str::CleanerCleanupFailureCount(count), Language::Vietnamese) => {
                format!("Thất bại: {count}").into()
            }
            (Str::CleanerPermissionTitle, Language::English) => {
                "Full Disk Access".into()
            }
            (Str::CleanerPermissionTitle, Language::Vietnamese) => {
                "Toàn quyền truy cập ổ đĩa".into()
            }
            (Str::CleanerPermissionExplanation, Language::English) => {
                "Some Cleaner categories need Full Disk Access to inspect protected macOS data safely."
                    .into()
            }
            (Str::CleanerPermissionExplanation, Language::Vietnamese) => {
                "Một số danh mục Cleaner cần Toàn quyền truy cập ổ đĩa để kiểm tra an toàn dữ liệu macOS được bảo vệ.".into()
            }
            (Str::CleanerPermissionOpenSettings, Language::English) => {
                "Open settings".into()
            }
            (Str::CleanerPermissionOpenSettings, Language::Vietnamese) => {
                "Mở cài đặt".into()
            }
            (Str::CleanerPartialPermissionDenied, Language::English) => {
                "Some locations were skipped because permission was denied.".into()
            }
            (Str::CleanerPartialPermissionDenied, Language::Vietnamese) => {
                "Một số vị trí đã bị bỏ qua vì không đủ quyền.".into()
            }
            (Str::CleanerPartialRootUnavailable, Language::English) => {
                "Some configured roots were unavailable on this machine.".into()
            }
            (Str::CleanerPartialRootUnavailable, Language::Vietnamese) => {
                "Một số thư mục gốc cấu hình sẵn không có trên máy này.".into()
            }
            (Str::CleanerPartialCancelled, Language::English) => {
                "The scan was cancelled before every root completed.".into()
            }
            (Str::CleanerPartialCancelled, Language::Vietnamese) => {
                "Lượt quét đã bị huỷ trước khi mọi thư mục gốc hoàn tất.".into()
            }
            (Str::CleanerPartialUnsupported, Language::English) => {
                "This category will land in a later Cleaner phase.".into()
            }
            (Str::CleanerPartialUnsupported, Language::Vietnamese) => {
                "Danh mục này sẽ được bổ sung ở giai đoạn Cleaner sau.".into()
            }
            (Str::CleanerBeginUninstallReview, Language::English) => {
                "Begin uninstall review".into()
            }
            (Str::CleanerBeginUninstallReview, Language::Vietnamese) => {
                "Bắt đầu xem xét gỡ cài đặt".into()
            }
            (Str::CleanerUninstallReviewTitle { name }, Language::English) => {
                format!("Uninstall {name}?").into()
            }
            (Str::CleanerUninstallReviewTitle { name }, Language::Vietnamese) => {
                format!("Gỡ cài đặt {name}?").into()
            }
            (Str::CleanerUninstallLoading, Language::English) => {
                "Analyzing related files…".into()
            }
            (Str::CleanerUninstallLoading, Language::Vietnamese) => {
                "Đang phân tích các tệp liên quan…".into()
            }
            (Str::CleanerUninstallRefusedProtected, Language::English) => {
                "System apps cannot be uninstalled.".into()
            }
            (Str::CleanerUninstallRefusedProtected, Language::Vietnamese) => {
                "Không thể gỡ cài đặt ứng dụng hệ thống.".into()
            }
            (Str::CleanerUninstallRefusedNotApplication, Language::English) => {
                "This item cannot be reviewed for uninstall.".into()
            }
            (Str::CleanerUninstallRefusedNotApplication, Language::Vietnamese) => {
                "Không thể xem xét gỡ cài đặt cho mục này.".into()
            }
            (Str::CleanerUninstallRelatedFilesHeader, Language::English) => {
                "Related files".into()
            }
            (Str::CleanerUninstallRelatedFilesHeader, Language::Vietnamese) => {
                "Tệp liên quan".into()
            }
            (Str::CleanerUninstallNoRelatedFiles, Language::English) => {
                "No related files were found.".into()
            }
            (Str::CleanerUninstallNoRelatedFiles, Language::Vietnamese) => {
                "Không tìm thấy tệp liên quan nào.".into()
            }
            (Str::CleanerUninstallDestinationNote, Language::English) => {
                "The app and every checked file will move to the macOS Trash. You can restore them from Trash until it is emptied."
                    .into()
            }
            (Str::CleanerUninstallDestinationNote, Language::Vietnamese) => {
                "Ứng dụng và các tệp đã chọn sẽ được chuyển vào Thùng rác của macOS. Bạn có thể khôi phục từ Thùng rác cho đến khi nó được dọn sạch."
                    .into()
            }
            (Str::CleanerUninstallScanOnlyBadge, Language::English) => {
                "Scan-only (system location)".into()
            }
            (Str::CleanerUninstallScanOnlyBadge, Language::Vietnamese) => {
                "Chỉ quét (vị trí hệ thống)".into()
            }
            (Str::CleanerUninstallMoveToTrash, Language::English) => "Move to Trash".into(),
            (Str::CleanerUninstallMoveToTrash, Language::Vietnamese) => {
                "Chuyển vào Thùng rác".into()
            }
            (Str::CleanerUninstallClose, Language::English) => "Close".into(),
            (Str::CleanerUninstallClose, Language::Vietnamese) => "Đóng".into(),
            (Str::CleanerConfidenceConfirmed, Language::English) => "Confirmed".into(),
            (Str::CleanerConfidenceConfirmed, Language::Vietnamese) => "Chắc chắn".into(),
            (Str::CleanerConfidenceHigh, Language::English) => "High".into(),
            (Str::CleanerConfidenceHigh, Language::Vietnamese) => "Cao".into(),
            (Str::CleanerConfidenceMedium, Language::English) => "Medium".into(),
            (Str::CleanerConfidenceMedium, Language::Vietnamese) => "Trung bình".into(),
            (Str::CleanerConfidenceLow, Language::English) => "Low".into(),
            (Str::CleanerConfidenceLow, Language::Vietnamese) => "Thấp".into(),
            (Str::CleanerConfidenceSharedOrUnsafe, Language::English) => "Shared or unsafe".into(),
            (Str::CleanerConfidenceSharedOrUnsafe, Language::Vietnamese) => {
                "Chia sẻ hoặc không an toàn".into()
            }
            (Str::CleanerKeepItem, Language::English) => "Keep".into(),
            (Str::CleanerKeepItem, Language::Vietnamese) => "Giữ lại".into(),
            (Str::CleanerIgnoreStoreError(detail), Language::English) => format!(
                "cleaner-ignored-items.json could not be read or written: {detail}"
            )
            .into(),
            (Str::CleanerIgnoreStoreError(detail), Language::Vietnamese) => format!(
                "Không đọc hoặc ghi được cleaner-ignored-items.json: {detail}"
            )
            .into(),
            (Str::CleanerIgnoreStoreMissingVersion, Language::English) => {
                "cleaner-ignored-items.json carries no version, so it was not written by dodo. \
                 It is being left alone and no items are marked kept."
                    .into()
            }
            (Str::CleanerIgnoreStoreMissingVersion, Language::Vietnamese) => {
                "cleaner-ignored-items.json không có trường version nên không phải do dodo ghi. \
                 dodo giữ nguyên tệp và không mục nào được đánh dấu giữ lại."
                    .into()
            }
            (
                Str::CleanerIgnoreStoreUnsupportedVersion { found, understood },
                Language::English,
            ) => format!(
                "cleaner-ignored-items.json is version {found}; this dodo understands \
                 {understood}. The file is being left alone and no items are marked kept."
            )
            .into(),
            (
                Str::CleanerIgnoreStoreUnsupportedVersion { found, understood },
                Language::Vietnamese,
            ) => format!(
                "cleaner-ignored-items.json là phiên bản {found}; bản dodo này hiểu phiên bản \
                 {understood}. dodo giữ nguyên tệp và không mục nào được đánh dấu giữ lại."
            )
            .into(),
            (Str::CleanerDockerCleanupConfirmTitle, Language::English) => {
                "Remove selected Docker objects?".into()
            }
            (Str::CleanerDockerCleanupConfirmTitle, Language::Vietnamese) => {
                "Xoá các đối tượng Docker đã chọn?".into()
            }
            (
                Str::CleanerDockerCleanupConfirmMessage { count, size },
                Language::English,
            ) => format!(
                "{count} Docker objects will be removed via the Docker CLI. This does not use \
                 the Trash and cannot be undone through dodo. Estimated size: {size}."
            )
            .into(),
            (
                Str::CleanerDockerCleanupConfirmMessage { count, size },
                Language::Vietnamese,
            ) => format!(
                "{count} đối tượng Docker sẽ bị xoá qua Docker CLI. Việc này không dùng Thùng \
                 rác và không thể hoàn tác qua dodo. Dung lượng ước tính: {size}."
            )
            .into(),

            (Str::JsonPlaceholder, Language::English) => {
                "Paste JSON here, then click Format.".into()
            }
            (Str::JsonPlaceholder, Language::Vietnamese) => {
                "Dán JSON vào đây rồi bấm Định dạng.".into()
            }
            (Str::FormatButton, Language::English) => "Format".into(),
            (Str::FormatButton, Language::Vietnamese) => "Định dạng".into(),
            (Str::IndentLabel, Language::English) => "Indent:".into(),
            (Str::IndentLabel, Language::Vietnamese) => "Thụt lề:".into(),
            (Str::IndentSpaces(count), Language::English) => format!("{count} spaces").into(),
            (Str::IndentSpaces(count), Language::Vietnamese) => {
                format!("{count} khoảng trắng").into()
            }
            (
                Str::InvalidJson {
                    line,
                    column,
                    detail,
                },
                Language::English,
            ) => format!("Invalid JSON at line {line}, column {column}: {detail}").into(),
            (
                Str::InvalidJson {
                    line,
                    column,
                    detail,
                },
                Language::Vietnamese,
            ) => format!("JSON không hợp lệ tại dòng {line}, cột {column}: {detail}").into(),

            (Str::FormatLabel, Language::English) => "Format:".into(),
            (Str::FormatLabel, Language::Vietnamese) => "Định dạng:".into(),
            (Str::EncodeButton, Language::English) => "Encode".into(),
            (Str::EncodeButton, Language::Vietnamese) => "Mã hoá".into(),
            (Str::DecodeButton, Language::English) => "Decode".into(),
            (Str::DecodeButton, Language::Vietnamese) => "Giải mã".into(),
            (Str::DecodeJwtButton, Language::English) => "Decode JWT".into(),
            (Str::DecodeJwtButton, Language::Vietnamese) => "Giải mã JWT".into(),
            (Str::InputLabel, Language::English) => "Input".into(),
            (Str::InputLabel, Language::Vietnamese) => "Đầu vào".into(),
            (Str::OutputLabel, Language::English) => "Output".into(),
            (Str::OutputLabel, Language::Vietnamese) => "Đầu ra".into(),
            (Str::JwtHeaderLabel, Language::English) => "Header".into(),
            (Str::JwtHeaderLabel, Language::Vietnamese) => "Header".into(),
            (Str::JwtPayloadLabel, Language::English) => "Payload".into(),
            (Str::JwtPayloadLabel, Language::Vietnamese) => "Payload".into(),
            (Str::JwtSignatureLabel, Language::English) => "Signature (not verified)".into(),
            (Str::JwtSignatureLabel, Language::Vietnamese) => "Chữ ký (chưa xác thực)".into(),
            (Str::EncoderInputPlaceholder, Language::English) => {
                "Paste the text or token to convert here.".into()
            }
            (Str::EncoderInputPlaceholder, Language::Vietnamese) => {
                "Dán văn bản hoặc token cần chuyển đổi vào đây.".into()
            }
            (Str::EncoderOutputPlaceholder, Language::English) => "Result appears here.".into(),
            (Str::EncoderOutputPlaceholder, Language::Vietnamese) => {
                "Kết quả hiển thị ở đây.".into()
            }
            (Str::FormatBase64, Language::English) => "Base64 (standard)".into(),
            (Str::FormatBase64, Language::Vietnamese) => "Base64 (chuẩn)".into(),
            (Str::FormatBase64UrlSafe, Language::English) => "Base64 (URL-safe)".into(),
            (Str::FormatBase64UrlSafe, Language::Vietnamese) => "Base64 (an toàn cho URL)".into(),
            (Str::FormatUrl, Language::English) => "URL percent-encoding".into(),
            (Str::FormatUrl, Language::Vietnamese) => "Mã hoá phần trăm URL".into(),
            (Str::FormatHex, Language::English) => "Hex".into(),
            (Str::FormatHex, Language::Vietnamese) => "Hex".into(),
            (Str::FormatJwt, Language::English) => "JWT (decode only)".into(),
            (Str::FormatJwt, Language::Vietnamese) => "JWT (chỉ giải mã)".into(),

            (Str::JwtEncodeUnsupported, Language::English) => {
                "JWT is decode-only: no signing key is available.".into()
            }
            (Str::JwtEncodeUnsupported, Language::Vietnamese) => {
                "JWT chỉ hỗ trợ giải mã: không có khoá ký.".into()
            }
            (Str::InvalidHexOddLength(count), Language::English) => {
                format!("Invalid hex: expected an even number of digits, got {count}.").into()
            }
            (Str::InvalidHexOddLength(count), Language::Vietnamese) => {
                format!("Hex không hợp lệ: cần số ký tự chẵn, nhận được {count}.").into()
            }
            (Str::InvalidHexDigit { digit, position }, Language::English) => {
                format!("Invalid hex: '{digit}' at position {position} is not a hex digit.").into()
            }
            (Str::InvalidHexDigit { digit, position }, Language::Vietnamese) => {
                format!("Hex không hợp lệ: '{digit}' ở vị trí {position} không phải ký tự hex.")
                    .into()
            }
            (Str::InvalidBase64(detail), Language::English) => {
                format!("Invalid base64: {detail}").into()
            }
            (Str::InvalidBase64(detail), Language::Vietnamese) => {
                format!("Base64 không hợp lệ: {detail}").into()
            }
            (Str::InvalidPercentAt(position), Language::English) => format!(
                "Invalid percent-encoding: '%' at position {position} is not followed by two hex digits."
            )
            .into(),
            (Str::InvalidPercentAt(position), Language::Vietnamese) => format!(
                "Mã hoá phần trăm không hợp lệ: '%' ở vị trí {position} không được theo sau bởi hai ký tự hex."
            )
            .into(),
            (Str::InvalidPercentEncoding(detail), Language::English) => {
                format!("Invalid percent-encoding: {detail}").into()
            }
            (Str::InvalidPercentEncoding(detail), Language::Vietnamese) => {
                format!("Mã hoá phần trăm không hợp lệ: {detail}").into()
            }
            (Str::NotUtf8(detail), Language::English) => {
                format!("Decoded bytes are not valid UTF-8 text: {detail}").into()
            }
            (Str::NotUtf8(detail), Language::Vietnamese) => {
                format!("Dữ liệu giải mã không phải văn bản UTF-8 hợp lệ: {detail}").into()
            }
            (Str::JwtEmpty, Language::English) => "Invalid JWT: the input is empty.".into(),
            (Str::JwtEmpty, Language::Vietnamese) => {
                "JWT không hợp lệ: chưa có dữ liệu đầu vào.".into()
            }
            (Str::JwtPartCount(count), Language::English) => {
                format!("Invalid JWT: expected 3 dot-separated parts, got {count}.").into()
            }
            (Str::JwtPartCount(count), Language::Vietnamese) => {
                format!("JWT không hợp lệ: cần 3 phần ngăn cách bởi dấu chấm, nhận được {count}.")
                    .into()
            }
            (Str::JwtPartNotBase64 { part, detail }, Language::English) => {
                let part = part.name(Language::English);
                format!("Invalid JWT: the {part} is not valid base64url ({detail}).").into()
            }
            (Str::JwtPartNotBase64 { part, detail }, Language::Vietnamese) => {
                let part = part.name(Language::Vietnamese);
                format!("JWT không hợp lệ: phần {part} không phải base64url hợp lệ ({detail}).")
                    .into()
            }
            (Str::JwtPartNotJson { part, detail }, Language::English) => {
                let part = part.name(Language::English);
                format!("Invalid JWT: the {part} is not valid JSON ({detail}).").into()
            }
            (Str::JwtPartNotJson { part, detail }, Language::Vietnamese) => {
                let part = part.name(Language::Vietnamese);
                format!("JWT không hợp lệ: phần {part} không phải JSON hợp lệ ({detail}).").into()
            }
            (Str::JwtPartNotRenderable { part, detail }, Language::English) => {
                let part = part.name(Language::English);
                format!("Invalid JWT: could not render the {part} ({detail}).").into()
            }
            (Str::JwtPartNotRenderable { part, detail }, Language::Vietnamese) => {
                let part = part.name(Language::Vietnamese);
                format!("JWT không hợp lệ: không thể hiển thị phần {part} ({detail}).").into()
            }

            (Str::Collections, Language::English) => "Collections".into(),
            (Str::Collections, Language::Vietnamese) => "Bộ sưu tập".into(),
            (Str::NoCollections, Language::English) => "No collections yet".into(),
            (Str::NoCollections, Language::Vietnamese) => "Chưa có bộ sưu tập nào".into(),
            (Str::NoCollectionsHint, Language::English) => {
                "Saved requests will be grouped here.".into()
            }
            (Str::NoCollectionsHint, Language::Vietnamese) => {
                "Các yêu cầu đã lưu sẽ được nhóm ở đây.".into()
            }

            (Str::UrlPlaceholder, Language::English) => {
                "Enter a URL, then press Send.".into()
            }
            (Str::UrlPlaceholder, Language::Vietnamese) => {
                "Nhập URL rồi bấm Gửi.".into()
            }
            (Str::Send, Language::English) => "Send".into(),
            (Str::Send, Language::Vietnamese) => "Gửi".into(),
            (Str::NewRequest, Language::English) => "New request".into(),
            (Str::NewRequest, Language::Vietnamese) => "Yêu cầu mới".into(),
            (Str::CloseRequest, Language::English) => "Close request".into(),
            (Str::CloseRequest, Language::Vietnamese) => "Đóng yêu cầu".into(),
            (Str::NameRequest, Language::English) => "Name this request".into(),
            (Str::NameRequest, Language::Vietnamese) => "Đặt tên yêu cầu này".into(),
            (Str::NameRequestPlaceholder, Language::English) => "Request name".into(),
            (Str::NameRequestPlaceholder, Language::Vietnamese) => "Tên yêu cầu".into(),
            (Str::SaveName, Language::English) => "Save name".into(),
            (Str::SaveName, Language::Vietnamese) => "Lưu tên".into(),
            (Str::GenerateCode, Language::English) => "Generate code".into(),
            (Str::GenerateCode, Language::Vietnamese) => "Sinh mã".into(),
            (Str::RequestTabParams, Language::English) => "Params".into(),
            (Str::RequestTabParams, Language::Vietnamese) => "Tham số".into(),
            (Str::RequestTabHeaders, Language::English) => "Headers".into(),
            (Str::RequestTabHeaders, Language::Vietnamese) => "Header".into(),
            (Str::RequestTabBody, Language::English) => "Body".into(),
            (Str::RequestTabBody, Language::Vietnamese) => "Nội dung".into(),
            (Str::RequestTabAuth, Language::English) => "Auth".into(),
            (Str::RequestTabAuth, Language::Vietnamese) => "Xác thực".into(),
            (Str::RequestTabScripts, Language::English) => "Scripts".into(),
            (Str::RequestTabScripts, Language::Vietnamese) => "Kịch bản".into(),

            (Str::ColumnKey, Language::English) => "KEY".into(),
            (Str::ColumnKey, Language::Vietnamese) => "KHOÁ".into(),
            (Str::ColumnValue, Language::English) => "VALUE".into(),
            (Str::ColumnValue, Language::Vietnamese) => "GIÁ TRỊ".into(),
            (Str::Add, Language::English) => "Add".into(),
            (Str::Add, Language::Vietnamese) => "Thêm".into(),
            (Str::AddParameter, Language::English) => "Add parameter".into(),
            (Str::AddParameter, Language::Vietnamese) => "Thêm tham số".into(),
            (Str::AddHeader, Language::English) => "Add header".into(),
            (Str::AddHeader, Language::Vietnamese) => "Thêm header".into(),
            (Str::DeleteRow, Language::English) => "Delete row".into(),
            (Str::DeleteRow, Language::Vietnamese) => "Xoá dòng".into(),
            (Str::NoActiveParams, Language::English) => "No active params".into(),
            (Str::NoActiveParams, Language::Vietnamese) => "Không có tham số nào bật".into(),
            (Str::ActiveParams(count), Language::English) => {
                format!("{count} active params").into()
            }
            (Str::ActiveParams(count), Language::Vietnamese) => {
                format!("{count} tham số đang bật").into()
            }
            (Str::NoActiveHeaders, Language::English) => "No active headers".into(),
            (Str::NoActiveHeaders, Language::Vietnamese) => "Không có header nào bật".into(),
            (Str::ActiveHeaders(count), Language::English) => {
                format!("{count} active headers").into()
            }
            (Str::ActiveHeaders(count), Language::Vietnamese) => {
                format!("{count} header đang bật").into()
            }
            (Str::ParamKeyPlaceholder, Language::English) => "Parameter".into(),
            (Str::ParamKeyPlaceholder, Language::Vietnamese) => "Tham số".into(),
            (Str::ParamValuePlaceholder, Language::English) => "Value".into(),
            (Str::ParamValuePlaceholder, Language::Vietnamese) => "Giá trị".into(),
            (Str::HeaderKeyPlaceholder, Language::English) => "Header".into(),
            (Str::HeaderKeyPlaceholder, Language::Vietnamese) => "Tên header".into(),
            (Str::HeaderValuePlaceholder, Language::English) => "Value".into(),
            (Str::HeaderValuePlaceholder, Language::Vietnamese) => "Giá trị".into(),
            (Str::ColumnDescription, Language::English) => "DESCRIPTION".into(),
            (Str::ColumnDescription, Language::Vietnamese) => "MÔ TẢ".into(),
            (Str::DescriptionPlaceholder, Language::English) => "Description".into(),
            (Str::DescriptionPlaceholder, Language::Vietnamese) => "Mô tả".into(),
            (Str::DuplicateRow, Language::English) => "Duplicate row".into(),
            (Str::DuplicateRow, Language::Vietnamese) => "Nhân đôi dòng".into(),
            (Str::MoveRowUp, Language::English) => "Move row up".into(),
            (Str::MoveRowUp, Language::Vietnamese) => "Chuyển dòng lên".into(),
            (Str::MoveRowDown, Language::English) => "Move row down".into(),
            (Str::MoveRowDown, Language::Vietnamese) => "Chuyển dòng xuống".into(),
            (Str::AddField, Language::English) => "Add field".into(),
            (Str::AddField, Language::Vietnamese) => "Thêm trường".into(),
            (Str::NoActiveFields, Language::English) => "No active fields".into(),
            (Str::NoActiveFields, Language::Vietnamese) => "Không có trường nào đang bật".into(),
            (Str::ActiveFields(count), Language::English) => format!("{count} active fields").into(),
            (Str::ActiveFields(count), Language::Vietnamese) => {
                format!("{count} trường đang bật").into()
            }
            (Str::FieldKeyPlaceholder, Language::English) => "Field".into(),
            (Str::FieldKeyPlaceholder, Language::Vietnamese) => "Trường".into(),
            (Str::FieldValuePlaceholder, Language::English) => "Value".into(),
            (Str::FieldValuePlaceholder, Language::Vietnamese) => "Giá trị".into(),

            (Str::BodyTypeNone, Language::English) => "None".into(),
            (Str::BodyTypeNone, Language::Vietnamese) => "Không có".into(),
            (Str::BodyTypeJson, Language::English) => "JSON".into(),
            (Str::BodyTypeJson, Language::Vietnamese) => "JSON".into(),
            (Str::BodyTypeText, Language::English) => "Raw text".into(),
            (Str::BodyTypeText, Language::Vietnamese) => "Văn bản thô".into(),
            (Str::BodyTypeXml, Language::English) => "XML".into(),
            (Str::BodyTypeXml, Language::Vietnamese) => "XML".into(),
            (Str::BodyTypeHtml, Language::English) => "HTML".into(),
            (Str::BodyTypeHtml, Language::Vietnamese) => "HTML".into(),
            (Str::BodyTypeFormData, Language::English) => "Form data".into(),
            (Str::BodyTypeFormData, Language::Vietnamese) => "Dữ liệu biểu mẫu".into(),
            (Str::BodyTypeUrlEncoded, Language::English) => "x-www-form-urlencoded".into(),
            (Str::BodyTypeUrlEncoded, Language::Vietnamese) => "x-www-form-urlencoded".into(),
            (Str::BodyTypeBinary, Language::English) => "Binary".into(),
            (Str::BodyTypeBinary, Language::Vietnamese) => "Nhị phân".into(),
            (Str::BodyPlaceholder, Language::English) => {
                "Type or paste the request body here.".into()
            }
            (Str::BodyPlaceholder, Language::Vietnamese) => {
                "Nhập hoặc dán nội dung yêu cầu vào đây.".into()
            }
            (Str::NoBodyTitle, Language::English) => "No body".into(),
            (Str::NoBodyTitle, Language::Vietnamese) => "Không có nội dung".into(),
            (Str::NoBodyHint, Language::English) => {
                "This request is sent without a body. Choose a type above to add one.".into()
            }
            (Str::NoBodyHint, Language::Vietnamese) => {
                "Yêu cầu này được gửi mà không có nội dung. Chọn một loại ở trên để thêm.".into()
            }
            (Str::BinaryBodyHint, Language::English) => {
                "Pick a file to send as the raw request body.".into()
            }
            (Str::BinaryBodyHint, Language::Vietnamese) => {
                "Chọn một tệp để gửi làm nội dung thô của yêu cầu.".into()
            }
            (Str::MethodSendsNoBody(method), Language::English) => {
                format!("{method} requests are sent without a body.").into()
            }
            (Str::MethodSendsNoBody(method), Language::Vietnamese) => {
                format!("Yêu cầu {method} được gửi mà không có nội dung.").into()
            }

            (Str::AuthTypeLabel, Language::English) => "Auth type".into(),
            (Str::AuthTypeLabel, Language::Vietnamese) => "Kiểu xác thực".into(),
            (Str::AuthTypeNone, Language::English) => "No auth".into(),
            (Str::AuthTypeNone, Language::Vietnamese) => "Không xác thực".into(),
            (Str::AuthTypeBearer, Language::English) => "Bearer token".into(),
            (Str::AuthTypeBearer, Language::Vietnamese) => "Bearer token".into(),
            (Str::AuthTypeBasic, Language::English) => "Basic auth".into(),
            (Str::AuthTypeBasic, Language::Vietnamese) => "Basic auth".into(),
            (Str::AuthTypeApiKey, Language::English) => "API key".into(),
            (Str::AuthTypeApiKey, Language::Vietnamese) => "API key".into(),
            (Str::AuthTypeOAuth2, Language::English) => "OAuth 2.0".into(),
            (Str::AuthTypeOAuth2, Language::Vietnamese) => "OAuth 2.0".into(),
            (Str::OAuth2Later, Language::English) => {
                "OAuth 2.0 needs a browser redirect and a token store; it arrives in a later step."
                    .into()
            }
            (Str::OAuth2Later, Language::Vietnamese) => {
                "OAuth 2.0 cần chuyển hướng trình duyệt và nơi lưu token; phần này sẽ có ở bước sau."
                    .into()
            }
            (Str::NoAuthTitle, Language::English) => "No authorization".into(),
            (Str::NoAuthTitle, Language::Vietnamese) => "Không có xác thực".into(),
            (Str::NoAuthHint, Language::English) => {
                "This request carries no Authorization header. Choose a scheme above to add one."
                    .into()
            }
            (Str::NoAuthHint, Language::Vietnamese) => {
                "Yêu cầu này không mang header Authorization. Chọn một cách ở trên để thêm.".into()
            }
            (Str::AuthTokenLabel, Language::English) => "Token".into(),
            (Str::AuthTokenLabel, Language::Vietnamese) => "Token".into(),
            (Str::AuthTokenPlaceholder, Language::English) => "Paste the bearer token".into(),
            (Str::AuthTokenPlaceholder, Language::Vietnamese) => "Dán bearer token vào đây".into(),
            (Str::AuthUsernameLabel, Language::English) => "Username".into(),
            (Str::AuthUsernameLabel, Language::Vietnamese) => "Tên đăng nhập".into(),
            (Str::AuthUsernamePlaceholder, Language::English) => "Your username".into(),
            (Str::AuthUsernamePlaceholder, Language::Vietnamese) => {
                "Tên đăng nhập của bạn".into()
            }
            (Str::AuthPasswordLabel, Language::English) => "Password".into(),
            (Str::AuthPasswordLabel, Language::Vietnamese) => "Mật khẩu".into(),
            (Str::AuthPasswordPlaceholder, Language::English) => "Your password".into(),
            (Str::AuthPasswordPlaceholder, Language::Vietnamese) => "Mật khẩu của bạn".into(),
            (Str::ApiKeyNameLabel, Language::English) => "Key".into(),
            (Str::ApiKeyNameLabel, Language::Vietnamese) => "Khoá".into(),
            (Str::ApiKeyNamePlaceholder, Language::English) => "For example X-Api-Key".into(),
            (Str::ApiKeyNamePlaceholder, Language::Vietnamese) => "Ví dụ X-Api-Key".into(),
            (Str::ApiKeyValueLabel, Language::English) => "Value".into(),
            (Str::ApiKeyValueLabel, Language::Vietnamese) => "Giá trị".into(),
            (Str::ApiKeyValuePlaceholder, Language::English) => "The key's value".into(),
            (Str::ApiKeyValuePlaceholder, Language::Vietnamese) => "Giá trị của khoá".into(),
            (Str::ApiKeySendAs, Language::English) => "Send as".into(),
            (Str::ApiKeySendAs, Language::Vietnamese) => "Gửi dưới dạng".into(),
            (Str::ApiKeyInHeader, Language::English) => "Header".into(),
            (Str::ApiKeyInHeader, Language::Vietnamese) => "Header".into(),
            (Str::ApiKeyInQuery, Language::English) => "Query parameter".into(),
            (Str::ApiKeyInQuery, Language::Vietnamese) => "Tham số truy vấn".into(),

            (Str::ScriptsSandboxNotice, Language::English) => {
                "Both scripts run in a sandbox with no filesystem, no network and no modules. \
                 pm.sendRequest, require and setTimeout are not available."
                    .into()
            }
            (Str::ScriptsSandboxNotice, Language::Vietnamese) => {
                "Cả hai kịch bản chạy trong hộp cát không có tệp, không có mạng và không có \
                 mô-đun. pm.sendRequest, require và setTimeout không khả dụng."
                    .into()
            }
            (Str::PreRequestScriptLabel, Language::English) => "Pre-request script".into(),
            (Str::PreRequestScriptLabel, Language::Vietnamese) => "Kịch bản trước yêu cầu".into(),
            (Str::PreRequestScriptPlaceholder, Language::English) => {
                "Runs before the request is sent.".into()
            }
            (Str::PreRequestScriptPlaceholder, Language::Vietnamese) => {
                "Chạy trước khi yêu cầu được gửi.".into()
            }
            (Str::PostResponseScriptLabel, Language::English) => "Post-response script".into(),
            (Str::PostResponseScriptLabel, Language::Vietnamese) => "Kịch bản sau phản hồi".into(),
            (Str::PostResponseScriptPlaceholder, Language::English) => {
                "Runs after the response arrives.".into()
            }
            (Str::PostResponseScriptPlaceholder, Language::Vietnamese) => {
                "Chạy sau khi phản hồi về.".into()
            }

            (Str::ResponseTabBody, Language::English) => "Body".into(),
            (Str::ResponseTabBody, Language::Vietnamese) => "Nội dung".into(),
            (Str::ResponseTabHeaders, Language::English) => "Headers".into(),
            (Str::ResponseTabHeaders, Language::Vietnamese) => "Header".into(),
            (Str::ResponseTabCookies, Language::English) => "Cookies".into(),
            (Str::ResponseTabCookies, Language::Vietnamese) => "Cookie".into(),
            (Str::ResponseTabTests, Language::English) => "Tests".into(),
            (Str::ResponseTabTests, Language::Vietnamese) => "Kiểm thử".into(),
            (Str::ResponseTabConsole, Language::English) => "Console".into(),
            (Str::ResponseTabConsole, Language::Vietnamese) => "Nhật ký".into(),
            (Str::NoResponseYet, Language::English) => "No response yet".into(),
            (Str::NoResponseYet, Language::Vietnamese) => "Chưa có phản hồi".into(),
            (Str::NoResponseHint, Language::English) => {
                "Send the request to see the response here.".into()
            }
            (Str::NoResponseHint, Language::Vietnamese) => {
                "Gửi yêu cầu để xem phản hồi ở đây.".into()
            }
            (Str::Sending, Language::English) => "Sending…".into(),
            (Str::Sending, Language::Vietnamese) => "Đang gửi…".into(),
            (Str::RequestFailed, Language::English) => "FAILED".into(),
            (Str::RequestFailed, Language::Vietnamese) => "THẤT BẠI".into(),
            (Str::CollapseResponse, Language::English) => "Collapse response".into(),
            (Str::CollapseResponse, Language::Vietnamese) => "Thu gọn phản hồi".into(),
            (Str::ExpandResponse, Language::English) => "Expand response".into(),
            (Str::ExpandResponse, Language::Vietnamese) => "Mở rộng phản hồi".into(),
            (Str::BodyPretty, Language::English) => "Pretty".into(),
            (Str::BodyPretty, Language::Vietnamese) => "Đẹp".into(),
            (Str::BodyRaw, Language::English) => "Raw".into(),
            (Str::BodyRaw, Language::Vietnamese) => "Thô".into(),
            (Str::Copy, Language::English) => "Copy".into(),
            (Str::Copy, Language::Vietnamese) => "Sao chép".into(),
            (Str::LoadMoreLines, Language::English) => "Load more lines".into(),
            (Str::LoadMoreLines, Language::Vietnamese) => "Tải thêm dòng".into(),
            (Str::BodyTruncated, Language::English) => {
                "The body was too large and was cut short.".into()
            }
            (Str::BodyTruncated, Language::Vietnamese) => {
                "Nội dung quá lớn nên đã bị cắt bớt.".into()
            }
            (Str::LineRange { shown, total }, Language::English) => {
                format!("{shown} of {total} lines").into()
            }
            (Str::LineRange { shown, total }, Language::Vietnamese) => {
                format!("{shown} trên {total} dòng").into()
            }

            (Str::StatusClassInfo, Language::English) => "INFO".into(),
            (Str::StatusClassInfo, Language::Vietnamese) => "THÔNG TIN".into(),
            (Str::StatusClassSuccess, Language::English) => "SUCCESS".into(),
            (Str::StatusClassSuccess, Language::Vietnamese) => "THÀNH CÔNG".into(),
            (Str::StatusClassRedirect, Language::English) => "REDIRECT".into(),
            (Str::StatusClassRedirect, Language::Vietnamese) => "CHUYỂN HƯỚNG".into(),
            (Str::StatusClassClientError, Language::English) => "CLIENT ERR".into(),
            (Str::StatusClassClientError, Language::Vietnamese) => "LỖI PHÍA GỌI".into(),
            (Str::StatusClassServerError, Language::English) => "SERVER ERR".into(),
            (Str::StatusClassServerError, Language::Vietnamese) => "LỖI MÁY CHỦ".into(),
            (Str::StatusClassUnknown, Language::English) => "UNKNOWN".into(),
            (Str::StatusClassUnknown, Language::Vietnamese) => "KHÔNG RÕ".into(),

            (Str::HttpInvalidUrl(detail), Language::English) => {
                if detail.is_empty() {
                    "Enter a URL before sending.".into()
                } else {
                    format!("That URL could not be read: {detail}").into()
                }
            }
            (Str::HttpInvalidUrl(detail), Language::Vietnamese) => {
                if detail.is_empty() {
                    "Hãy nhập URL trước khi gửi.".into()
                } else {
                    format!("Không đọc được URL đó: {detail}").into()
                }
            }
            (Str::HttpUnsupportedScheme(scheme), Language::English) => {
                format!("This tool can only fetch http and https, not {scheme}.").into()
            }
            (Str::HttpUnsupportedScheme(scheme), Language::Vietnamese) => {
                format!("Công cụ này chỉ gọi được http và https, không phải {scheme}.").into()
            }
            (Str::HttpInvalidHeader(name), Language::English) => {
                format!("The header \"{name}\" cannot be sent as written.").into()
            }
            (Str::HttpInvalidHeader(name), Language::Vietnamese) => {
                format!("Header \"{name}\" không gửi được như đang viết.").into()
            }
            (Str::HttpTimeout(seconds), Language::English) => {
                format!("No response within {seconds} seconds.").into()
            }
            (Str::HttpTimeout(seconds), Language::Vietnamese) => {
                format!("Không có phản hồi trong {seconds} giây.").into()
            }
            (Str::HttpDnsFailure(host), Language::English) => {
                format!("The address \"{host}\" could not be found.").into()
            }
            (Str::HttpDnsFailure(host), Language::Vietnamese) => {
                format!("Không tìm thấy địa chỉ \"{host}\".").into()
            }
            (Str::HttpConnectFailure(detail), Language::English) => {
                format!("Could not connect: {detail}").into()
            }
            (Str::HttpConnectFailure(detail), Language::Vietnamese) => {
                format!("Không kết nối được: {detail}").into()
            }
            (Str::HttpTlsFailure(detail), Language::English) => {
                format!("The secure connection was refused: {detail}").into()
            }
            (Str::HttpTlsFailure(detail), Language::Vietnamese) => {
                format!("Kết nối bảo mật bị từ chối: {detail}").into()
            }
            (Str::HttpBodyNotText(detail), Language::English) => {
                format!("The response is not text this viewer can show ({detail}).").into()
            }
            (Str::HttpBodyNotText(detail), Language::Vietnamese) => {
                format!("Phản hồi không phải văn bản có thể hiển thị ({detail}).").into()
            }
            (Str::HttpUnexpected(detail), Language::English) => {
                format!("The request failed: {detail}").into()
            }
            (Str::HttpUnexpected(detail), Language::Vietnamese) => {
                format!("Yêu cầu thất bại: {detail}").into()
            }

            (Str::ImportCollection, Language::English) => "Import a collection".into(),
            (Str::ImportCollection, Language::Vietnamese) => "Nhập bộ sưu tập".into(),
            (Str::NewCollection, Language::English) => "New collection".into(),
            (Str::NewCollection, Language::Vietnamese) => "Bộ sưu tập mới".into(),
            (Str::NewFolder, Language::English) => "New folder".into(),
            (Str::NewFolder, Language::Vietnamese) => "Thư mục mới".into(),
            (Str::SearchCollectionsPlaceholder, Language::English) => "Search collections".into(),
            (Str::SearchCollectionsPlaceholder, Language::Vietnamese) => "Tìm bộ sưu tập".into(),
            (Str::Rename, Language::English) => "Rename".into(),
            (Str::Rename, Language::Vietnamese) => "Đổi tên".into(),
            (Str::Delete, Language::English) => "Delete".into(),
            (Str::Delete, Language::Vietnamese) => "Xoá".into(),
            (Str::Duplicate, Language::English) => "Duplicate".into(),
            (Str::Duplicate, Language::Vietnamese) => "Nhân đôi".into(),
            (Str::Open, Language::English) => "Open".into(),
            (Str::Open, Language::Vietnamese) => "Mở".into(),
            (Str::MoreActions, Language::English) => "Actions".into(),
            (Str::MoreActions, Language::Vietnamese) => "Thao tác".into(),
            (Str::NamePlaceholder, Language::English) => "Name".into(),
            (Str::NamePlaceholder, Language::Vietnamese) => "Tên".into(),
            (Str::DefaultCollectionName, Language::English) => "New collection".into(),
            (Str::DefaultCollectionName, Language::Vietnamese) => "Bộ sưu tập mới".into(),
            (Str::DefaultFolderName, Language::English) => "New folder".into(),
            (Str::DefaultFolderName, Language::Vietnamese) => "Thư mục mới".into(),
            (Str::SaveToCollectionNote, Language::English) => {
                "Saved into your collections.".into()
            }
            (Str::SaveToCollectionNote, Language::Vietnamese) => {
                "Đã lưu vào bộ sưu tập của bạn.".into()
            }
            (Str::CollectionStoreError(detail), Language::English) => {
                format!("Could not save collections: {detail}").into()
            }
            (Str::CollectionStoreError(detail), Language::Vietnamese) => {
                format!("Không lưu được bộ sưu tập: {detail}").into()
            }
            (Str::CollectionImportError(detail), Language::English) => {
                format!("Could not import that file: {detail}").into()
            }
            (Str::CollectionImportError(detail), Language::Vietnamese) => {
                format!("Không nhập được tệp đó: {detail}").into()
            }

            (Str::History, Language::English) => "History".into(),
            (Str::History, Language::Vietnamese) => "Lịch sử".into(),
            (Str::NoHistory, Language::English) => "No requests yet".into(),
            (Str::NoHistory, Language::Vietnamese) => "Chưa có yêu cầu nào".into(),
            (Str::NoHistoryHint, Language::English) => {
                "Requests you send appear here, newest first.".into()
            }
            (Str::NoHistoryHint, Language::Vietnamese) => {
                "Các yêu cầu bạn gửi sẽ hiện ở đây, mới nhất trước.".into()
            }
            (Str::HistoryReopen, Language::English) => "Reopen in a new tab".into(),
            (Str::HistoryReopen, Language::Vietnamese) => "Mở lại trong thẻ mới".into(),
            (Str::HistoryResend, Language::English) => "Resend".into(),
            (Str::HistoryResend, Language::Vietnamese) => "Gửi lại".into(),
            (Str::HistoryClearAll, Language::English) => "Clear all".into(),
            (Str::HistoryClearAll, Language::Vietnamese) => "Xoá tất cả".into(),
            (Str::HistoryJustNow, Language::English) => "just now".into(),
            (Str::HistoryJustNow, Language::Vietnamese) => "vừa xong".into(),
            (Str::HistoryMinutesAgo(minutes), Language::English) => {
                format!("{minutes}m ago").into()
            }
            (Str::HistoryMinutesAgo(minutes), Language::Vietnamese) => {
                format!("{minutes} phút trước").into()
            }
            (Str::HistoryHoursAgo(hours), Language::English) => format!("{hours}h ago").into(),
            (Str::HistoryHoursAgo(hours), Language::Vietnamese) => {
                format!("{hours} giờ trước").into()
            }
            (Str::HistoryDaysAgo(days), Language::English) => format!("{days}d ago").into(),
            (Str::HistoryDaysAgo(days), Language::Vietnamese) => format!("{days} ngày trước").into(),

            (Str::BodyPreview, Language::English) => "Preview".into(),
            (Str::BodyPreview, Language::Vietnamese) => "Xem trước".into(),
            (Str::BodyTree, Language::English) => "Tree".into(),
            (Str::BodyTree, Language::Vietnamese) => "Cây".into(),
            (Str::SaveToFile, Language::English) => "Save to file".into(),
            (Str::SaveToFile, Language::Vietnamese) => "Lưu ra tệp".into(),
            (Str::JsonTreeTruncated(count), Language::English) => {
                format!("Showing the first {count} nodes — collapse some to see the rest.").into()
            }
            (Str::JsonTreeTruncated(count), Language::Vietnamese) => {
                format!("Đang hiện {count} nút đầu — thu gọn bớt để xem phần còn lại.").into()
            }
            (Str::HtmlPreviewNote, Language::English) => {
                "Text preview — markup is shown as readable text, not rendered.".into()
            }
            (Str::HtmlPreviewNote, Language::Vietnamese) => {
                "Xem trước văn bản — mã đánh dấu hiển thị dạng chữ, không kết xuất.".into()
            }
            (Str::NoCookies, Language::English) => "No cookies set".into(),
            (Str::NoCookies, Language::Vietnamese) => "Không có cookie nào".into(),
            (Str::NoCookiesHint, Language::English) => {
                "This response sent no Set-Cookie headers.".into()
            }
            (Str::NoCookiesHint, Language::Vietnamese) => {
                "Phản hồi này không gửi header Set-Cookie nào.".into()
            }

            (Str::ToggleAllRows, Language::English) => "Enable or disable all rows".into(),
            (Str::ToggleAllRows, Language::Vietnamese) => "Bật hoặc tắt tất cả các dòng".into(),
            (Str::EditModeTable, Language::English) => "Table".into(),
            (Str::EditModeTable, Language::Vietnamese) => "Bảng".into(),
            (Str::EditModeBulk, Language::English) => "Bulk edit".into(),
            (Str::EditModeBulk, Language::Vietnamese) => "Sửa hàng loạt".into(),
            (Str::BulkEditPlaceholder, Language::English) => {
                "One entry per line as Key: Value. Begin a line with # to disable it.".into()
            }
            (Str::BulkEditPlaceholder, Language::Vietnamese) => {
                "Mỗi dòng một mục dạng Key: Value. Bắt đầu dòng bằng # để tắt mục đó.".into()
            }

            (Str::InsertTemplate, Language::English) => "Insert template".into(),
            (Str::InsertTemplate, Language::Vietnamese) => "Chèn mẫu".into(),
            (Str::TemplateSetHeader, Language::English) => "Set a header".into(),
            (Str::TemplateSetHeader, Language::Vietnamese) => "Đặt một header".into(),
            (Str::TemplateSetBearerToken, Language::English) => "Set a bearer token".into(),
            (Str::TemplateSetBearerToken, Language::Vietnamese) => "Đặt bearer token".into(),
            (Str::TemplateSetTimestamp, Language::English) => "Set a timestamp variable".into(),
            (Str::TemplateSetTimestamp, Language::Vietnamese) => "Đặt biến thời gian".into(),
            (Str::TemplateAssertStatus, Language::English) => "Assert status is 200".into(),
            (Str::TemplateAssertStatus, Language::Vietnamese) => "Kiểm tra trạng thái là 200".into(),
            (Str::TemplateLogResponse, Language::English) => "Log the response body".into(),
            (Str::TemplateLogResponse, Language::Vietnamese) => {
                "Ghi nhật ký nội dung phản hồi".into()
            }
            (Str::TemplateExtractField, Language::English) => "Extract a JSON field".into(),
            (Str::TemplateExtractField, Language::Vietnamese) => "Trích một trường JSON".into(),

            // Docker module — section and page names (terms of art, identical).
            (Str::Docker, _) => "Docker".into(),
            (Str::Containers, _) => "Containers".into(),
            (Str::Images, _) => "Images".into(),
            (Str::Volumes, _) => "Volumes".into(),
            (Str::Networks, _) => "Networks".into(),

            (Str::DockerSearchPlaceholder, Language::English) => "Search containers".into(),
            (Str::DockerSearchPlaceholder, Language::Vietnamese) => "Tìm container".into(),
            (Str::DockerRefresh, Language::English) => "Refresh".into(),
            (Str::DockerRefresh, Language::Vietnamese) => "Làm mới".into(),
            (Str::DockerFilter, Language::English) => "Filter".into(),
            (Str::DockerFilter, Language::Vietnamese) => "Bộ lọc".into(),
            (Str::DockerCreate, Language::English) => "Create".into(),
            (Str::DockerCreate, Language::Vietnamese) => "Tạo mới".into(),

            (Str::DockerColumnName, Language::English) => "Name".into(),
            (Str::DockerColumnName, Language::Vietnamese) => "Tên".into(),
            (Str::DockerColumnImage, _) => "Image".into(),
            (Str::DockerColumnStatus, Language::English) => "Status".into(),
            (Str::DockerColumnStatus, Language::Vietnamese) => "Trạng thái".into(),
            (Str::DockerColumnCpu, _) => "CPU %".into(),
            (Str::DockerColumnPorts, Language::English) => "Ports".into(),
            (Str::DockerColumnPorts, Language::Vietnamese) => "Cổng".into(),
            (Str::DockerColumnLastStarted, Language::English) => "Last Started".into(),
            (Str::DockerColumnLastStarted, Language::Vietnamese) => "Khởi động lần cuối".into(),
            (Str::DockerColumnActions, Language::English) => "Actions".into(),
            (Str::DockerColumnActions, Language::Vietnamese) => "Thao tác".into(),

            (Str::DockerStatusRunning, Language::English) => "Running".into(),
            (Str::DockerStatusRunning, Language::Vietnamese) => "Đang chạy".into(),
            (Str::DockerStatusExited, Language::English) => "Exited".into(),
            (Str::DockerStatusExited, Language::Vietnamese) => "Đã dừng".into(),
            (Str::DockerStatusCreated, Language::English) => "Created".into(),
            (Str::DockerStatusCreated, Language::Vietnamese) => "Đã tạo".into(),
            (Str::DockerStatusRestarting, Language::English) => "Restarting".into(),
            (Str::DockerStatusRestarting, Language::Vietnamese) => "Đang khởi động lại".into(),
            (Str::DockerStatusPaused, Language::English) => "Paused".into(),
            (Str::DockerStatusPaused, Language::Vietnamese) => "Tạm dừng".into(),
            (Str::DockerStatusDead, Language::English) => "Dead".into(),
            (Str::DockerStatusDead, Language::Vietnamese) => "Đã hỏng".into(),
            (Str::DockerStatusRemoving, Language::English) => "Removing".into(),
            (Str::DockerStatusRemoving, Language::Vietnamese) => "Đang xoá".into(),
            (Str::DockerStatusStopping, Language::English) => "Stopping".into(),
            (Str::DockerStatusStopping, Language::Vietnamese) => "Đang dừng".into(),
            (Str::DockerStatusUnknown, Language::English) => "Unknown".into(),
            (Str::DockerStatusUnknown, Language::Vietnamese) => "Không rõ".into(),

            (Str::DockerStart, Language::English) => "Start".into(),
            (Str::DockerStart, Language::Vietnamese) => "Khởi động".into(),
            (Str::DockerStop, Language::English) => "Stop".into(),
            (Str::DockerStop, Language::Vietnamese) => "Dừng".into(),
            (Str::DockerRestart, Language::English) => "Restart".into(),
            (Str::DockerRestart, Language::Vietnamese) => "Khởi động lại".into(),
            (Str::DockerDeleteTitle, Language::English) => "Delete container?".into(),
            (Str::DockerDeleteTitle, Language::Vietnamese) => "Xoá container?".into(),
            (Str::DockerDeleteMessage(name), Language::English) => {
                format!("Permanently remove \"{name}\"? This cannot be undone.").into()
            }
            (Str::DockerDeleteMessage(name), Language::Vietnamese) => {
                format!("Xoá vĩnh viễn \"{name}\"? Hành động này không thể hoàn tác.").into()
            }
            (Str::DockerCancel, Language::English) => "Cancel".into(),
            (Str::DockerCancel, Language::Vietnamese) => "Huỷ".into(),

            (Str::NoContainers, Language::English) => "No containers found.".into(),
            (Str::NoContainers, Language::Vietnamese) => "Không tìm thấy container nào.".into(),
            (Str::NoContainersHint, Language::English) => {
                "Containers you create will appear here.".into()
            }
            (Str::NoContainersHint, Language::Vietnamese) => {
                "Các container bạn tạo sẽ hiển thị ở đây.".into()
            }
            (Str::DockerRetry, Language::English) => "Retry".into(),
            (Str::DockerRetry, Language::Vietnamese) => "Thử lại".into(),
            (Str::DockerConnectionError(detail), Language::English) => {
                format!("Could not reach the Docker engine: {detail}").into()
            }
            (Str::DockerConnectionError(detail), Language::Vietnamese) => {
                format!("Không kết nối được tới Docker engine: {detail}").into()
            }
            (Str::DockerOperationError(detail), Language::English) => {
                format!("That action could not be completed: {detail}").into()
            }
            (Str::DockerOperationError(detail), Language::Vietnamese) => {
                format!("Không thể hoàn tất thao tác đó: {detail}").into()
            }

            (Str::DockerSelectAll, Language::English) => "Select all".into(),
            (Str::DockerSelectAll, Language::Vietnamese) => "Chọn tất cả".into(),
            (Str::DockerSelectRow, Language::English) => "Select container".into(),
            (Str::DockerSelectRow, Language::Vietnamese) => "Chọn container".into(),

            (Str::DockerRelNever, Language::English) => "Never".into(),
            (Str::DockerRelNever, Language::Vietnamese) => "Chưa bao giờ".into(),
            (Str::DockerRelJustNow, Language::English) => "just now".into(),
            (Str::DockerRelJustNow, Language::Vietnamese) => "vừa xong".into(),
            (Str::DockerRelSecondsAgo(n), Language::English) => {
                format!("{n} second{} ago", if n == 1 { "" } else { "s" }).into()
            }
            (Str::DockerRelSecondsAgo(n), Language::Vietnamese) => format!("{n} giây trước").into(),
            (Str::DockerRelMinutesAgo(n), Language::English) => {
                format!("{n} minute{} ago", if n == 1 { "" } else { "s" }).into()
            }
            (Str::DockerRelMinutesAgo(n), Language::Vietnamese) => format!("{n} phút trước").into(),
            (Str::DockerRelHoursAgo(n), Language::English) => {
                format!("{n} hour{} ago", if n == 1 { "" } else { "s" }).into()
            }
            (Str::DockerRelHoursAgo(n), Language::Vietnamese) => format!("{n} giờ trước").into(),
            (Str::DockerRelDaysAgo(n), Language::English) => {
                format!("{n} day{} ago", if n == 1 { "" } else { "s" }).into()
            }
            (Str::DockerRelDaysAgo(n), Language::Vietnamese) => format!("{n} ngày trước").into(),
            (Str::DockerRelWeeksAgo(n), Language::English) => {
                format!("{n} week{} ago", if n == 1 { "" } else { "s" }).into()
            }
            (Str::DockerRelWeeksAgo(n), Language::Vietnamese) => format!("{n} tuần trước").into(),
            (Str::DockerRelMonthsAgo(n), Language::English) => {
                format!("{n} month{} ago", if n == 1 { "" } else { "s" }).into()
            }
            (Str::DockerRelMonthsAgo(n), Language::Vietnamese) => format!("{n} tháng trước").into(),
            (Str::DockerRelYearsAgo(n), Language::English) => {
                format!("{n} year{} ago", if n == 1 { "" } else { "s" }).into()
            }
            (Str::DockerRelYearsAgo(n), Language::Vietnamese) => format!("{n} năm trước").into(),

            (Str::DockerUnreachableTitle, Language::English) => "Can't reach the Docker engine".into(),
            (Str::DockerUnreachableTitle, Language::Vietnamese) => {
                "Không kết nối được Docker engine".into()
            }

            // Docker module (round 2) — compose grouping.
            (Str::DockerUngrouped, Language::English) => "Ungrouped".into(),
            (Str::DockerUngrouped, Language::Vietnamese) => "Chưa nhóm".into(),
            (Str::DockerGroupContainers(n), Language::English) => {
                format!("{n} container{}", if n == 1 { "" } else { "s" }).into()
            }
            (Str::DockerGroupContainers(n), Language::Vietnamese) => {
                format!("{n} container").into()
            }
            (Str::DockerGroupRunning(n), Language::English) => format!("{n} running").into(),
            (Str::DockerGroupRunning(n), Language::Vietnamese) => {
                format!("{n} đang chạy").into()
            }

            // Docker module (round 2) — the filter popover.
            (Str::DockerFilterWithCount(n), Language::English) => format!("Filter ({n})").into(),
            (Str::DockerFilterWithCount(n), Language::Vietnamese) => {
                format!("Bộ lọc ({n})").into()
            }
            (Str::DockerFilterTitle, Language::English) => "Filters".into(),
            (Str::DockerFilterTitle, Language::Vietnamese) => "Bộ lọc".into(),
            (Str::DockerFilterProject, Language::English) => "Compose project".into(),
            (Str::DockerFilterProject, Language::Vietnamese) => "Dự án Compose".into(),
            (Str::DockerFilterPublishedPorts, Language::English) => "Has published ports".into(),
            (Str::DockerFilterPublishedPorts, Language::Vietnamese) => "Có cổng công bố".into(),
            (Str::DockerFilterFavorites, Language::English) => "Favorites (coming soon)".into(),
            (Str::DockerFilterFavorites, Language::Vietnamese) => {
                "Yêu thích (sắp có)".into()
            }
            (Str::DockerFilterClear, Language::English) => "Clear filters".into(),
            (Str::DockerFilterClear, Language::Vietnamese) => "Xoá bộ lọc".into(),

            // Docker module (round 2) — bulk actions on the selection.
            (Str::DockerBulkSelected(n), Language::English) => format!("{n} selected").into(),
            (Str::DockerBulkSelected(n), Language::Vietnamese) => format!("Đã chọn {n}").into(),
            (Str::DockerBulkStart, Language::English) => "Start selected".into(),
            (Str::DockerBulkStart, Language::Vietnamese) => "Khởi động mục đã chọn".into(),
            (Str::DockerBulkStop, Language::English) => "Stop selected".into(),
            (Str::DockerBulkStop, Language::Vietnamese) => "Dừng mục đã chọn".into(),
            (Str::DockerBulkDelete, Language::English) => "Delete selected".into(),
            (Str::DockerBulkDelete, Language::Vietnamese) => "Xoá mục đã chọn".into(),
            (Str::DockerBulkClear, Language::English) => "Clear selection".into(),
            (Str::DockerBulkClear, Language::Vietnamese) => "Bỏ chọn".into(),
            (Str::DockerBulkDeleteTitle, Language::English) => "Delete containers?".into(),
            (Str::DockerBulkDeleteTitle, Language::Vietnamese) => "Xoá các container?".into(),
            (Str::DockerBulkDeleteMessage(n), Language::English) => format!(
                "Permanently remove {n} container{}? This cannot be undone.",
                if n == 1 { "" } else { "s" }
            )
            .into(),
            (Str::DockerBulkDeleteMessage(n), Language::Vietnamese) => {
                format!("Xoá vĩnh viễn {n} container? Hành động này không thể hoàn tác.").into()
            }
            (Str::DockerBulkFailures(n), Language::English) => format!(
                "{n} container{} could not be updated.",
                if n == 1 { "" } else { "s" }
            )
            .into(),
            (Str::DockerBulkFailures(n), Language::Vietnamese) => {
                format!("{n} container không thể cập nhật.").into()
            }

            // Round 3 — Images, Volumes and Networks column headers.
            (Str::DockerColumnRepository, Language::English) => "Repository".into(),
            (Str::DockerColumnRepository, Language::Vietnamese) => "Kho ảnh".into(),
            (Str::DockerColumnTag, Language::English) => "Tag".into(),
            (Str::DockerColumnTag, Language::Vietnamese) => "Thẻ".into(),
            (Str::DockerColumnImageId, Language::English) => "Image ID".into(),
            (Str::DockerColumnImageId, Language::Vietnamese) => "Mã ảnh".into(),
            (Str::DockerColumnSize, Language::English) => "Size".into(),
            (Str::DockerColumnSize, Language::Vietnamese) => "Kích thước".into(),
            (Str::DockerColumnCreated, Language::English) => "Created".into(),
            (Str::DockerColumnCreated, Language::Vietnamese) => "Đã tạo".into(),
            (Str::DockerColumnContainersUsing, Language::English) => "Containers using".into(),
            (Str::DockerColumnContainersUsing, Language::Vietnamese) => "Container đang dùng".into(),
            (Str::DockerColumnDriver, Language::English) => "Driver".into(),
            (Str::DockerColumnDriver, Language::Vietnamese) => "Trình điều khiển".into(),
            (Str::DockerColumnMountPoint, Language::English) => "Mount point".into(),
            (Str::DockerColumnMountPoint, Language::Vietnamese) => "Điểm gắn kết".into(),
            (Str::DockerColumnScope, Language::English) => "Scope".into(),
            (Str::DockerColumnScope, Language::Vietnamese) => "Phạm vi".into(),

            // Round 3 — per-resource search placeholders.
            (Str::DockerSearchImages, Language::English) => "Search images".into(),
            (Str::DockerSearchImages, Language::Vietnamese) => "Tìm ảnh".into(),
            (Str::DockerSearchVolumes, Language::English) => "Search volumes".into(),
            (Str::DockerSearchVolumes, Language::Vietnamese) => "Tìm volume".into(),
            (Str::DockerSearchNetworks, Language::English) => "Search networks".into(),
            (Str::DockerSearchNetworks, Language::Vietnamese) => "Tìm mạng".into(),

            // Round 3 — empty states.
            (Str::NoImages, Language::English) => "No images".into(),
            (Str::NoImages, Language::Vietnamese) => "Không có ảnh".into(),
            (Str::NoImagesHint, Language::English) => {
                "Pull or build an image and it will appear here.".into()
            }
            (Str::NoImagesHint, Language::Vietnamese) => {
                "Kéo về hoặc dựng một ảnh và nó sẽ xuất hiện ở đây.".into()
            }
            (Str::NoVolumes, Language::English) => "No volumes".into(),
            (Str::NoVolumes, Language::Vietnamese) => "Không có volume".into(),
            (Str::NoVolumesHint, Language::English) => {
                "Create a volume and it will appear here.".into()
            }
            (Str::NoVolumesHint, Language::Vietnamese) => {
                "Tạo một volume và nó sẽ xuất hiện ở đây.".into()
            }
            (Str::NoNetworks, Language::English) => "No networks".into(),
            (Str::NoNetworks, Language::Vietnamese) => "Không có mạng".into(),
            (Str::NoNetworksHint, Language::English) => {
                "Create a network and it will appear here.".into()
            }
            (Str::NoNetworksHint, Language::Vietnamese) => {
                "Tạo một mạng và nó sẽ xuất hiện ở đây.".into()
            }

            // Round 3 — shared tokens and the Inspect placeholder action.
            (Str::DockerNotAvailable, _) => "N/A".into(),
            (Str::DockerNone, _) => "<none>".into(),
            (Str::DockerInspect, Language::English) => "Inspect".into(),
            (Str::DockerInspect, Language::Vietnamese) => "Xem chi tiết".into(),
            (Str::DockerNetworkPredefined, Language::English) => {
                "Predefined networks cannot be removed".into()
            }
            (Str::DockerNetworkPredefined, Language::Vietnamese) => {
                "Không thể xoá mạng định sẵn".into()
            }

            (Str::DockerViewLogs, Language::English) => "View Logs".into(),
            (Str::DockerViewLogs, Language::Vietnamese) => "Xem nhật ký".into(),
            (Str::DockerOpenTerminal, Language::English) => "Open Terminal".into(),
            (Str::DockerOpenTerminal, Language::Vietnamese) => "Mở terminal".into(),
            (Str::DockerComingSoonLabel, Language::English) => "Coming soon".into(),
            (Str::DockerComingSoonLabel, Language::Vietnamese) => "Sắp có".into(),

            // Round 5 — the Inspect panel and the Logs viewer.
            (Str::DockerDetails, Language::English) => "Details".into(),
            (Str::DockerDetails, Language::Vietnamese) => "Chi tiết".into(),
            (Str::DockerRawJson, Language::English) => "Raw JSON".into(),
            (Str::DockerRawJson, Language::Vietnamese) => "JSON gốc".into(),
            (Str::DockerDetailErrorTitle, Language::English) => "Couldn't load this".into(),
            (Str::DockerDetailErrorTitle, Language::Vietnamese) => "Không tải được".into(),
            (Str::DockerNoLogs, Language::English) => "No log output.".into(),
            (Str::DockerNoLogs, Language::Vietnamese) => "Không có nhật ký.".into(),
            (Str::DockerNoLogsHint, Language::English) => {
                "This container hasn't written anything to stdout or stderr yet.".into()
            }
            (Str::DockerNoLogsHint, Language::Vietnamese) => {
                "Container này chưa ghi gì ra stdout hoặc stderr.".into()
            }
            (Str::DockerLogsTail(n), Language::English) => {
                format!("Showing the last {n} lines").into()
            }
            (Str::DockerLogsTail(n), Language::Vietnamese) => {
                format!("Đang hiển thị {n} dòng cuối").into()
            }
            (Str::DockerYes, Language::English) => "Yes".into(),
            (Str::DockerYes, Language::Vietnamese) => "Có".into(),
            (Str::DockerNo, Language::English) => "No".into(),
            (Str::DockerNo, Language::Vietnamese) => "Không".into(),
            // Inspect field labels. "ID", "Digest" and "Gateway" are the wire's
            // own terms, unchanged in Vietnamese like JSON and JWT above.
            (Str::DockerFieldId, _) => "ID".into(),
            (Str::DockerFieldCommand, Language::English) => "Command".into(),
            (Str::DockerFieldCommand, Language::Vietnamese) => "Lệnh".into(),
            (Str::DockerFieldStarted, Language::English) => "Started".into(),
            (Str::DockerFieldStarted, Language::Vietnamese) => "Khởi động lúc".into(),
            (Str::DockerFieldExitCode, Language::English) => "Exit code".into(),
            (Str::DockerFieldExitCode, Language::Vietnamese) => "Mã thoát".into(),
            (Str::DockerFieldRestartPolicy, Language::English) => "Restart policy".into(),
            (Str::DockerFieldRestartPolicy, Language::Vietnamese) => {
                "Chính sách khởi động lại".into()
            }
            (Str::DockerFieldIpAddress, Language::English) => "IP address".into(),
            (Str::DockerFieldIpAddress, Language::Vietnamese) => "Địa chỉ IP".into(),
            (Str::DockerFieldMounts, Language::English) => "Mounts".into(),
            (Str::DockerFieldMounts, Language::Vietnamese) => "Điểm gắn".into(),
            (Str::DockerFieldTags, Language::English) => "Tags".into(),
            (Str::DockerFieldTags, Language::Vietnamese) => "Thẻ".into(),
            (Str::DockerFieldDigest, _) => "Digest".into(),
            (Str::DockerFieldArchitecture, Language::English) => "Architecture".into(),
            (Str::DockerFieldArchitecture, Language::Vietnamese) => "Kiến trúc".into(),
            (Str::DockerFieldOs, Language::English) => "OS".into(),
            (Str::DockerFieldOs, Language::Vietnamese) => "Hệ điều hành".into(),
            (Str::DockerFieldLayers, Language::English) => "Layers".into(),
            (Str::DockerFieldLayers, Language::Vietnamese) => "Lớp".into(),
            (Str::DockerFieldLabels, Language::English) => "Labels".into(),
            (Str::DockerFieldLabels, Language::Vietnamese) => "Nhãn".into(),
            (Str::DockerFieldOptions, Language::English) => "Options".into(),
            (Str::DockerFieldOptions, Language::Vietnamese) => "Tuỳ chọn".into(),
            (Str::DockerFieldInternal, Language::English) => "Internal".into(),
            (Str::DockerFieldInternal, Language::Vietnamese) => "Nội bộ".into(),
            (Str::DockerFieldAttachable, Language::English) => "Attachable".into(),
            (Str::DockerFieldAttachable, Language::Vietnamese) => "Cho phép gắn".into(),
            (Str::DockerFieldSubnet, Language::English) => "Subnet".into(),
            (Str::DockerFieldSubnet, Language::Vietnamese) => "Dải mạng".into(),
            (Str::DockerFieldGateway, _) => "Gateway".into(),

            // Round 5 — the placeholders that stay placeholders.
            (Str::DockerPull, Language::English) => "Pull".into(),
            (Str::DockerPull, Language::Vietnamese) => "Tải về".into(),
            (Str::DockerBuild, Language::English) => "Build".into(),
            (Str::DockerBuild, Language::Vietnamese) => "Dựng image".into(),
            (Str::DockerStats, Language::English) => "Stats".into(),
            (Str::DockerStats, Language::Vietnamese) => "Thống kê".into(),
            (Str::DockerOpenDetails, Language::English) => "Open details".into(),
            (Str::DockerOpenDetails, Language::Vietnamese) => "Mở chi tiết".into(),

            (Str::Runtimes, _) => "Runtimes".into(),
            (Str::RuntimesDescription, Language::English) => {
                "Detect the container runtimes on this machine and control them without leaving Dodo.".into()
            }
            (Str::RuntimesDescription, Language::Vietnamese) => {
                "Tự động phát hiện các runtime container trên máy này và điều khiển trực tiếp trong Dodo.".into()
            }
            (Str::RuntimePodmanMachine, _) => "Podman Machine".into(),
            (Str::RuntimeKubernetes, _) => "Kubernetes".into(),
            (Str::RuntimeContainerd, _) => "containerd".into(),
            (Str::RuntimeStatusRunning, Language::English) => "Running".into(),
            (Str::RuntimeStatusRunning, Language::Vietnamese) => "Đang chạy".into(),
            (Str::RuntimeStatusStopped, Language::English) => "Stopped".into(),
            (Str::RuntimeStatusStopped, Language::Vietnamese) => "Đã dừng".into(),
            (Str::RuntimeStatusNotInstalled, Language::English) => "Not installed".into(),
            (Str::RuntimeStatusNotInstalled, Language::Vietnamese) => "Chưa cài đặt".into(),
            (Str::RuntimeStatusUnsupported, Language::English) => "Not supported on this platform".into(),
            (Str::RuntimeStatusUnsupported, Language::Vietnamese) => {
                "Không hỗ trợ trên nền tảng này".into()
            }
            (Str::RuntimeStatusUnknown, Language::English) => "Unknown".into(),
            (Str::RuntimeStatusUnknown, Language::Vietnamese) => "Không rõ".into(),
            (Str::RuntimeManagedExternally, Language::English) => {
                "Managed by your cluster provider (Docker Desktop, minikube, kind, …), not from here.".into()
            }
            (Str::RuntimeManagedExternally, Language::Vietnamese) => {
                "Được quản lý bởi nhà cung cấp cụm của bạn (Docker Desktop, minikube, kind, …), không phải từ đây.".into()
            }
            (Str::RuntimeStarting, Language::English) => "Starting…".into(),
            (Str::RuntimeStarting, Language::Vietnamese) => "Đang khởi động…".into(),
            (Str::RuntimeStopping, Language::English) => "Stopping…".into(),
            (Str::RuntimeStopping, Language::Vietnamese) => "Đang dừng…".into(),
            (Str::RuntimeBinaryNotFound, Language::English) => {
                "The required command-line tool could not be found on this machine.".into()
            }
            (Str::RuntimeBinaryNotFound, Language::Vietnamese) => {
                "Không tìm thấy công cụ dòng lệnh cần thiết trên máy này.".into()
            }
            (Str::RuntimeActionUnsupported, Language::English) => {
                "This action isn't available for this runtime.".into()
            }
            (Str::RuntimeActionUnsupported, Language::Vietnamese) => {
                "Thao tác này không khả dụng cho runtime này.".into()
            }

            // Round 7 — typed form rows, the binary body, the tab title.
            (Str::UntitledRequest, Language::English) => "Untitled".into(),
            (Str::UntitledRequest, Language::Vietnamese) => "Chưa đặt tên".into(),
            (Str::ColumnType, Language::English) => "TYPE".into(),
            (Str::ColumnType, Language::Vietnamese) => "LOẠI".into(),
            (Str::FieldKindText, Language::English) => "Text".into(),
            (Str::FieldKindText, Language::Vietnamese) => "Văn bản".into(),
            (Str::FieldKindFile, Language::English) => "File".into(),
            (Str::FieldKindFile, Language::Vietnamese) => "Tệp".into(),
            (Str::ChooseFile, Language::English) => "Choose file…".into(),
            (Str::ChooseFile, Language::Vietnamese) => "Chọn tệp…".into(),
            (Str::ReplaceFile, Language::English) => "Choose another file".into(),
            (Str::ReplaceFile, Language::Vietnamese) => "Chọn tệp khác".into(),
            (Str::ClearFile, Language::English) => "Remove the chosen file".into(),
            (Str::ClearFile, Language::Vietnamese) => "Bỏ tệp đã chọn".into(),
            (Str::NoFileSelected, Language::English) => "No file chosen".into(),
            (Str::NoFileSelected, Language::Vietnamese) => "Chưa chọn tệp".into(),
            // English marks the plural and Vietnamese does not, which is why
            // each language formats the whole sentence rather than sharing a
            // stem: a count is not a value you can glue a translated prefix to.
            (Str::IncompleteFileFields(count), Language::English) => if count == 1 {
                "1 file field has no file chosen and will not be sent.".to_string()
            } else {
                format!("{count} file fields have no file chosen and will not be sent.")
            }
            .into(),
            (Str::IncompleteFileFields(count), Language::Vietnamese) => {
                format!("{count} trường tệp chưa chọn tệp nên sẽ không được gửi.").into()
            }
            (Str::HttpFileUnreadable { path, detail }, Language::English) => {
                format!("Could not read {path}: {detail}").into()
            }
            (Str::HttpFileUnreadable { path, detail }, Language::Vietnamese) => {
                format!("Không đọc được {path}: {detail}").into()
            }
            (Str::HttpFileTooLarge { path, limit_mb }, Language::English) => {
                format!("{path} is larger than the {limit_mb} MB this build can send.").into()
            }
            (Str::HttpFileTooLarge { path, limit_mb }, Language::Vietnamese) => {
                format!("{path} lớn hơn mức {limit_mb} MB mà bản dựng này gửi được.").into()
            }

            // Round 8 — variables and environments.
            (Str::NoEnvironment, Language::English) => "No environment".into(),
            (Str::NoEnvironment, Language::Vietnamese) => "Không dùng môi trường".into(),
            (Str::SelectEnvironment, Language::English) => "Choose the active environment".into(),
            (Str::SelectEnvironment, Language::Vietnamese) => {
                "Chọn môi trường đang dùng".into()
            }
            (Str::ManageEnvironments, Language::English) => "Manage environments…".into(),
            (Str::ManageEnvironments, Language::Vietnamese) => "Quản lý môi trường…".into(),
            (Str::Environments, Language::English) => "Environments".into(),
            (Str::Environments, Language::Vietnamese) => "Môi trường".into(),
            (Str::NewEnvironment, Language::English) => "New environment".into(),
            (Str::NewEnvironment, Language::Vietnamese) => "Môi trường mới".into(),
            (Str::DefaultEnvironmentName, Language::English) => "New environment".into(),
            (Str::DefaultEnvironmentName, Language::Vietnamese) => "Môi trường mới".into(),
            (Str::EnvironmentCopySuffix, Language::English) => "copy".into(),
            (Str::EnvironmentCopySuffix, Language::Vietnamese) => "bản sao".into(),
            (Str::DuplicateEnvironment, Language::English) => "Duplicate".into(),
            (Str::DuplicateEnvironment, Language::Vietnamese) => "Nhân bản".into(),
            (Str::DeleteEnvironment, Language::English) => "Delete".into(),
            (Str::DeleteEnvironment, Language::Vietnamese) => "Xoá".into(),
            (Str::ImportEnvironment, Language::English) => "Import".into(),
            (Str::ImportEnvironment, Language::Vietnamese) => "Nhập".into(),
            (Str::CollectionVariables, Language::English) => "Collection variables".into(),
            (Str::CollectionVariables, Language::Vietnamese) => "Biến bộ sưu tập".into(),
            (Str::EnvironmentVariables, Language::English) => "Environment variables".into(),
            (Str::EnvironmentVariables, Language::Vietnamese) => "Biến môi trường".into(),
            (Str::CollectionVariablesNote, Language::English) => {
                "Shared by every request, whichever environment is active. An imported \
                 collection files its own variables here."
                    .into()
            }
            (Str::CollectionVariablesNote, Language::Vietnamese) => {
                "Dùng chung cho mọi yêu cầu, bất kể môi trường nào đang bật. Bộ sưu tập được \
                 nhập vào sẽ đặt biến của nó ở đây."
                    .into()
            }
            (Str::NoEnvironmentsYet, Language::English) => "No environments yet".into(),
            (Str::NoEnvironmentsYet, Language::Vietnamese) => "Chưa có môi trường nào".into(),
            (Str::NoEnvironmentsYetHint, Language::English) => {
                "Create one to keep a host, a token or an API key in a single place and refer \
                 to it as {{name}}."
                    .into()
            }
            (Str::NoEnvironmentsYetHint, Language::Vietnamese) => {
                "Hãy tạo một môi trường để giữ tên máy chủ, mã thông báo hay khoá API ở một \
                 chỗ và gọi lại bằng {{name}}."
                    .into()
            }
            (Str::ColumnSecret, Language::English) => "SECRET".into(),
            (Str::ColumnSecret, Language::Vietnamese) => "BÍ MẬT".into(),
            (Str::AddVariable, Language::English) => "Add variable".into(),
            (Str::AddVariable, Language::Vietnamese) => "Thêm biến".into(),
            (Str::NoActiveVariables, Language::English) => "No variables".into(),
            (Str::NoActiveVariables, Language::Vietnamese) => "Chưa có biến nào".into(),
            (Str::ActiveVariables(count), Language::English) => {
                format!("{count} active").into()
            }
            (Str::ActiveVariables(count), Language::Vietnamese) => {
                format!("{count} đang bật").into()
            }
            (Str::VariableKeyPlaceholder, Language::English) => "baseUrl".into(),
            (Str::VariableKeyPlaceholder, Language::Vietnamese) => "baseUrl".into(),
            (Str::VariableValuePlaceholder, Language::English) => "Value".into(),
            (Str::VariableValuePlaceholder, Language::Vietnamese) => "Giá trị".into(),
            (Str::MarkSecret, Language::English) => "Mask this value in the editor".into(),
            (Str::MarkSecret, Language::Vietnamese) => "Che giá trị này trong trình sửa".into(),
            (Str::RevealSecret, Language::English) => "Show the value".into(),
            (Str::RevealSecret, Language::Vietnamese) => "Hiện giá trị".into(),
            (Str::HideSecret, Language::English) => "Hide the value".into(),
            (Str::HideSecret, Language::Vietnamese) => "Ẩn giá trị".into(),
            (Str::SecretStorageWarning, Language::English) => {
                "Secret values are masked here, but they are saved to this machine in plain \
                 text, unencrypted, like every other variable."
                    .into()
            }
            (Str::SecretStorageWarning, Language::Vietnamese) => {
                "Giá trị bí mật được che ở đây, nhưng vẫn lưu trên máy này dưới dạng văn bản \
                 thuần, không mã hoá, như mọi biến khác."
                    .into()
            }
            (Str::ResolvedUrlLabel, Language::English) => "Resolves to".into(),
            (Str::ResolvedUrlLabel, Language::Vietnamese) => "Kết quả thay thế".into(),
            (Str::UnresolvedVariablePreview(name), Language::English) => {
                format!("{name} is not defined").into()
            }
            (Str::UnresolvedVariablePreview(name), Language::Vietnamese) => {
                format!("{name} chưa được định nghĩa").into()
            }
            (Str::ResolvesFrom { name, scope }, Language::English) => {
                format!("{name} — from {scope}").into()
            }
            (Str::ResolvesFrom { name, scope }, Language::Vietnamese) => {
                format!("{name} — lấy từ {scope}").into()
            }
            (Str::HttpUnresolvedVariable(name), Language::English) => {
                format!("No variable named {name} is defined. Add it to an environment, or to \
                         the collection variables, then send again.")
                .into()
            }
            (Str::HttpUnresolvedVariable(name), Language::Vietnamese) => {
                format!("Chưa có biến nào tên {name}. Hãy thêm nó vào một môi trường hoặc vào \
                         biến bộ sưu tập rồi gửi lại.")
                .into()
            }
            (Str::HttpRecursiveVariable(name), Language::English) => {
                format!("The variable {name} refers back to itself, so it cannot be resolved.")
                    .into()
            }
            (Str::HttpRecursiveVariable(name), Language::Vietnamese) => {
                format!("Biến {name} tham chiếu lại chính nó nên không thể thay thế được.").into()
            }
            (Str::VariableStoreError(detail), Language::English) => {
                format!("Could not save or load environments: {detail}").into()
            }
            (Str::VariableStoreError(detail), Language::Vietnamese) => {
                format!("Không lưu hoặc đọc được môi trường: {detail}").into()
            }
            (Str::VariableStoreMissingVersion, Language::English) => {
                "This environments file carries no schema version, so it cannot be read safely."
                    .into()
            }
            (Str::VariableStoreMissingVersion, Language::Vietnamese) => {
                "Tệp môi trường này không ghi phiên bản lược đồ nên không thể đọc an toàn.".into()
            }
            (
                Str::VariableStoreUnsupportedVersion { found, supported },
                Language::English,
            ) => format!(
                "This environments file uses schema {found}; this build of dodo reads {supported}. \
                 Update dodo rather than risk misreading it."
            )
            .into(),
            (
                Str::VariableStoreUnsupportedVersion { found, supported },
                Language::Vietnamese,
            ) => format!(
                "Tệp môi trường này dùng lược đồ {found}; bản dodo này chỉ đọc {supported}. Hãy \
                 cập nhật dodo thay vì đọc sai tệp."
            )
            .into(),
            (Str::EnvironmentImportError(detail), Language::English) => {
                format!("Could not import that environment: {detail}").into()
            }
            (Str::EnvironmentImportError(detail), Language::Vietnamese) => {
                format!("Không nhập được môi trường đó: {detail}").into()
            }

            (Str::ScriptVariables, Language::English) => "Script".into(),
            (Str::ScriptVariables, Language::Vietnamese) => "Kịch bản".into(),
            (Str::ScriptThrew(detail), Language::English) => {
                format!("The script failed: {detail}").into()
            }
            (Str::ScriptThrew(detail), Language::Vietnamese) => {
                format!("Kịch bản lỗi: {detail}").into()
            }
            (Str::ScriptDeadline(seconds), Language::English) => format!(
                "The script did not finish within {seconds} s and was stopped."
            )
            .into(),
            (Str::ScriptDeadline(seconds), Language::Vietnamese) => {
                format!("Kịch bản không kết thúc trong {seconds} giây và đã bị dừng.").into()
            }
            (Str::ScriptOutOfMemory, Language::English) => {
                "The script asked for more memory than one run is allowed.".into()
            }
            (Str::ScriptOutOfMemory, Language::Vietnamese) => {
                "Kịch bản yêu cầu nhiều bộ nhớ hơn mức cho phép mỗi lần chạy.".into()
            }
            (Str::ScriptUnsupported(name), Language::English) => {
                format!("{name} is not supported in dodo, so this script cannot run.").into()
            }
            (Str::ScriptUnsupported(name), Language::Vietnamese) => {
                format!("dodo không hỗ trợ {name}, nên kịch bản này không chạy được.").into()
            }
            (Str::ScriptNoEngine, Language::English) => {
                "This build has no script engine, so nothing ran.".into()
            }
            (Str::ScriptNoEngine, Language::Vietnamese) => {
                "Bản dựng này không có bộ chạy kịch bản, nên không có gì được chạy.".into()
            }
            (Str::ScriptSkippedByPolicy, Language::English) => {
                "Scripts are switched off in Settings, so this one did not run.".into()
            }
            (Str::ScriptSkippedByPolicy, Language::Vietnamese) => {
                "Kịch bản đang tắt trong Cài đặt, nên kịch bản này không chạy.".into()
            }
            (Str::ScriptSkippedByConsent, Language::English) => {
                "This imported script was not approved, so it did not run.".into()
            }
            (Str::ScriptSkippedByConsent, Language::Vietnamese) => {
                "Kịch bản nhập vào này chưa được duyệt, nên nó không chạy.".into()
            }
            (Str::ScriptFinished { millis }, Language::English) => {
                format!("Pre-request script finished in {millis} ms.").into()
            }
            (Str::ScriptFinished { millis }, Language::Vietnamese) => {
                format!("Kịch bản trước yêu cầu chạy xong trong {millis} ms.").into()
            }
            (Str::ScriptWroteVariables(count), Language::English) => {
                format!("The script wrote {count} variables.").into()
            }
            (Str::ScriptWroteVariables(count), Language::Vietnamese) => {
                format!("Kịch bản đã ghi {count} biến.").into()
            }
            (Str::ScriptUnknownMethod(method), Language::English) => format!(
                "The script asked for method {method}, which dodo has no option for; the \
                 method in the editor was kept."
            )
            .into(),
            (Str::ScriptUnknownMethod(method), Language::Vietnamese) => format!(
                "Kịch bản yêu cầu phương thức {method} mà dodo không có; phương thức trong \
                 trình soạn thảo được giữ nguyên."
            )
            .into(),

            (Str::ConsoleLevelDebug, Language::English) => "Debug".into(),
            (Str::ConsoleLevelDebug, Language::Vietnamese) => "Gỡ lỗi".into(),
            (Str::ConsoleLevelLog, Language::English) => "Log".into(),
            (Str::ConsoleLevelLog, Language::Vietnamese) => "Nhật ký".into(),
            (Str::ConsoleLevelWarn, Language::English) => "Warn".into(),
            (Str::ConsoleLevelWarn, Language::Vietnamese) => "Cảnh báo".into(),
            (Str::ConsoleLevelError, Language::English) => "Error".into(),
            (Str::ConsoleLevelError, Language::Vietnamese) => "Lỗi".into(),
            (Str::ConsoleRunSeparator { run, summary }, Language::English) => {
                format!("Run {run} · {summary}").into()
            }
            (Str::ConsoleRunSeparator { run, summary }, Language::Vietnamese) => {
                format!("Lần chạy {run} · {summary}").into()
            }
            (Str::ConsoleRunTruncated(count), Language::English) => {
                format!("{count} lines from this run were dropped.").into()
            }
            (Str::ConsoleRunTruncated(count), Language::Vietnamese) => {
                format!("{count} dòng của lần chạy này đã bị bỏ.").into()
            }
            (Str::ConsoleEmpty, Language::English) => "Nothing logged yet".into(),
            (Str::ConsoleEmpty, Language::Vietnamese) => "Chưa có gì được ghi".into(),
            (Str::ConsoleEmptyHint, Language::English) => {
                "console.log from a script appears here, and stays across sends.".into()
            }
            (Str::ConsoleEmptyHint, Language::Vietnamese) => {
                "console.log từ kịch bản hiện ở đây và được giữ qua các lần gửi.".into()
            }
            (Str::ConsoleClear, Language::English) => "Clear".into(),
            (Str::ConsoleClear, Language::Vietnamese) => "Xoá".into(),
            (Str::ConsoleDropped(count), Language::English) => {
                format!("{count} older lines dropped").into()
            }
            (Str::ConsoleDropped(count), Language::Vietnamese) => {
                format!("Đã bỏ {count} dòng cũ").into()
            }

            (Str::RunScripts, Language::English) => "Run scripts".into(),
            (Str::RunScripts, Language::Vietnamese) => "Chạy kịch bản".into(),
            (Str::RunScriptsDescription, Language::English) => {
                "Whether the API Explorer runs the scripts a request carries. A script that \
                 arrived in an imported collection is code from someone else."
                    .into()
            }
            (Str::RunScriptsDescription, Language::Vietnamese) => {
                "API Explorer có chạy kịch bản đi kèm yêu cầu hay không. Kịch bản đến từ bộ \
                 sưu tập nhập vào là mã của người khác."
                    .into()
            }
            (Str::RunScriptsNever, Language::English) => "Never".into(),
            (Str::RunScriptsNever, Language::Vietnamese) => "Không bao giờ".into(),
            (Str::RunScriptsAskImported, Language::English) => "Ask for imported".into(),
            (Str::RunScriptsAskImported, Language::Vietnamese) => "Hỏi khi nhập vào".into(),
            (Str::RunScriptsAlways, Language::English) => "Always".into(),
            (Str::RunScriptsAlways, Language::Vietnamese) => "Luôn luôn".into(),
            (Str::ScriptConsentTitle, Language::English) => "Run this imported script?".into(),
            (Str::ScriptConsentTitle, Language::Vietnamese) => {
                "Chạy kịch bản nhập vào này?".into()
            }
            (Str::ScriptConsentExplain, Language::English) => {
                "This script came from an imported collection and has not run before. Read it \
                 before approving: it can change this request and write your variables."
                    .into()
            }
            (Str::ScriptConsentExplain, Language::Vietnamese) => {
                "Kịch bản này đến từ bộ sưu tập nhập vào và chưa từng chạy. Hãy đọc trước khi \
                 duyệt: nó có thể thay đổi yêu cầu này và ghi vào biến của bạn."
                    .into()
            }
            (Str::ScriptConsentRequest(name), Language::English) => {
                format!("Request: {name}").into()
            }
            (Str::ScriptConsentRequest(name), Language::Vietnamese) => {
                format!("Yêu cầu: {name}").into()
            }
            (Str::ScriptConsentRun, Language::English) => "Run script".into(),
            (Str::ScriptConsentRun, Language::Vietnamese) => "Chạy kịch bản".into(),
            (Str::ScriptConsentSkip, Language::English) => "Send without it".into(),
            (Str::ScriptConsentSkip, Language::Vietnamese) => "Gửi mà không chạy".into(),
            (Str::ConsentStoreError(detail), Language::English) => {
                format!("Could not read or write the script approvals: {detail}").into()
            }
            (Str::ConsentStoreError(detail), Language::Vietnamese) => {
                format!("Không đọc hoặc ghi được danh sách kịch bản đã duyệt: {detail}").into()
            }
            (Str::ConsentStoreMissingVersion, Language::English) => {
                "The script approvals file carries no schema version, so it was not read."
                    .into()
            }
            (Str::ConsentStoreMissingVersion, Language::Vietnamese) => {
                "Tệp kịch bản đã duyệt không có phiên bản lược đồ, nên không được đọc.".into()
            }
            (Str::ConsentStoreUnsupportedVersion { found, supported }, Language::English) => {
                format!(
                    "This script approvals file uses schema {found}; this build of dodo reads \
                     {supported}. Every imported script will ask again."
                )
                .into()
            }
            (
                Str::ConsentStoreUnsupportedVersion { found, supported },
                Language::Vietnamese,
            ) => format!(
                "Tệp kịch bản đã duyệt này dùng lược đồ {found}; bản dodo này chỉ đọc \
                 {supported}. Mọi kịch bản nhập vào sẽ hỏi lại."
            )
            .into(),
            (Str::ScriptConsentExplainChanged, Language::English) => {
                "This imported script has changed since you approved it, so the earlier \
                 approval no longer applies. Read it again before approving: it can change \
                 this request and write your variables."
                    .into()
            }
            (Str::ScriptConsentExplainChanged, Language::Vietnamese) => {
                "Kịch bản nhập vào này đã thay đổi kể từ khi bạn duyệt, nên lần duyệt trước \
                 không còn hiệu lực. Hãy đọc lại trước khi duyệt: nó có thể thay đổi yêu cầu \
                 này và ghi vào biến của bạn."
                    .into()
            }

            (Str::ScriptSyntaxError(detail), Language::English) => {
                format!("Syntax error: {detail}").into()
            }
            (Str::ScriptSyntaxError(detail), Language::Vietnamese) => {
                format!("Lỗi cú pháp: {detail}").into()
            }
            (Str::ScriptSyntaxErrorAt { line, detail }, Language::English) => {
                format!("Line {line}: {detail}").into()
            }
            (Str::ScriptSyntaxErrorAt { line, detail }, Language::Vietnamese) => {
                format!("Dòng {line}: {detail}").into()
            }

            (Str::TestScriptFinished { millis }, Language::English) => {
                format!("Post-response script finished in {millis} ms.").into()
            }
            (Str::TestScriptFinished { millis }, Language::Vietnamese) => {
                format!("Kịch bản sau phản hồi chạy xong trong {millis} ms.").into()
            }
            (Str::TestsNone, Language::English) => "This request has no tests".into(),
            (Str::TestsNone, Language::Vietnamese) => "Yêu cầu này chưa có kiểm thử".into(),
            (Str::TestsNoneHint, Language::English) => {
                "A post-response script can assert what came back with pm.test.".into()
            }
            (Str::TestsNoneHint, Language::Vietnamese) => {
                "Kịch bản sau phản hồi có thể kiểm tra kết quả trả về bằng pm.test.".into()
            }
            (Str::TestsAddOne, Language::English) => "Add a test".into(),
            (Str::TestsAddOne, Language::Vietnamese) => "Thêm kiểm thử".into(),
            (Str::TestsScriptDefinedNone, Language::English) => {
                "The script ran and defined no tests".into()
            }
            (Str::TestsScriptDefinedNone, Language::Vietnamese) => {
                "Kịch bản đã chạy và không định nghĩa kiểm thử nào".into()
            }
            (Str::TestsScriptDefinedNoneHint, Language::English) => {
                "Anything it printed is in the Console.".into()
            }
            (Str::TestsScriptDefinedNoneHint, Language::Vietnamese) => {
                "Những gì nó in ra nằm trong Console.".into()
            }
            (Str::TestsNotRun, Language::English) => {
                "This request has a test script, but it did not run".into()
            }
            (Str::TestsNotRun, Language::Vietnamese) => {
                "Yêu cầu này có kịch bản kiểm thử, nhưng nó đã không chạy".into()
            }
            (Str::TestsPassedCount(count), Language::English) => {
                format!("{count} passed").into()
            }
            (Str::TestsPassedCount(count), Language::Vietnamese) => {
                format!("{count} đạt").into()
            }
            (Str::TestsFailedCount(count), Language::English) => {
                format!("{count} failed").into()
            }
            (Str::TestsFailedCount(count), Language::Vietnamese) => {
                format!("{count} không đạt").into()
            }
            (Str::TestsErroredCount(count), Language::English) => {
                format!("{count} errored").into()
            }
            (Str::TestsErroredCount(count), Language::Vietnamese) => {
                format!("{count} lỗi kịch bản").into()
            }
            (Str::TestsDropped(count), Language::English) => {
                format!("{count} more results were dropped").into()
            }
            (Str::TestsDropped(count), Language::Vietnamese) => {
                format!("Đã bỏ thêm {count} kết quả").into()
            }

            (Str::CodeTargetCurl, Language::English) => "cURL".into(),
            (Str::CodeTargetCurl, Language::Vietnamese) => "cURL".into(),
            (Str::CodeTargetFetch, Language::English) => "fetch".into(),
            (Str::CodeTargetFetch, Language::Vietnamese) => "fetch".into(),
            (Str::CodeTargetAxios, Language::English) => "axios".into(),
            (Str::CodeTargetAxios, Language::Vietnamese) => "axios".into(),
            (Str::CodeTargetXhr, Language::English) => "XMLHttpRequest".into(),
            (Str::CodeTargetXhr, Language::Vietnamese) => "XMLHttpRequest".into(),
            (Str::GenerateCodeCarriesValues, Language::English) => {
                "This code carries the request's real values, including any token or \
                 password it uses."
                    .into()
            }
            (Str::GenerateCodeCarriesValues, Language::Vietnamese) => {
                "Đoạn mã này mang đúng các giá trị thật của yêu cầu, kể cả token hay mật \
                 khẩu mà nó dùng."
                    .into()
            }
            (Str::GenerateCodeSecretsWithheld(names), Language::English) => format!(
                "Left as {{{{placeholders}}}}: {names}. Everything else — including a token \
                 or password typed into this request — is in the code below."
            )
            .into(),
            (Str::GenerateCodeSecretsWithheld(names), Language::Vietnamese) => format!(
                "Được giữ nguyên dạng {{{{chỗ trống}}}}: {names}. Mọi thứ còn lại — kể cả \
                 token hay mật khẩu gõ trực tiếp vào yêu cầu này — đều nằm trong đoạn mã \
                 bên dưới."
            )
            .into(),
            (Str::GenerateCodeSecretsRevealed, Language::English) => {
                "This code contains the real value of every secret variable it uses, in \
                 plain text. Anything you paste it into keeps that value."
                    .into()
            }
            (Str::GenerateCodeSecretsRevealed, Language::Vietnamese) => {
                "Đoạn mã này chứa giá trị thật của mọi biến bí mật mà nó dùng, ở dạng văn \
                 bản thuần. Bất cứ nơi nào bạn dán vào cũng giữ lại giá trị đó."
                    .into()
            }
            (Str::GenerateCodeRevealSecrets, Language::English) => {
                "Resolve secret variables".into()
            }
            (Str::GenerateCodeRevealSecrets, Language::Vietnamese) => {
                "Thay thế cả biến bí mật".into()
            }

            (Str::CheckForUpdates, Language::English) => "Check for updates".into(),
            (Str::CheckForUpdates, Language::Vietnamese) => "Kiểm tra cập nhật".into(),
            (Str::SoftwareUpdate, Language::English) => "Software update".into(),
            (Str::SoftwareUpdate, Language::Vietnamese) => "Cập nhật phần mềm".into(),
            (Str::UpdateChecking, Language::English) => "Checking for updates…".into(),
            (Str::UpdateChecking, Language::Vietnamese) => "Đang kiểm tra cập nhật…".into(),
            (Str::UpdateUpToDate, Language::English) => "dodo is up to date.".into(),
            (Str::UpdateUpToDate, Language::Vietnamese) => "dodo đã là bản mới nhất.".into(),
            (Str::UpdateCurrentVersion(version), Language::English) => {
                format!("You are running version {version}.").into()
            }
            (Str::UpdateCurrentVersion(version), Language::Vietnamese) => {
                format!("Bạn đang dùng phiên bản {version}.").into()
            }
            (Str::UpdateAvailableHeadline(version), Language::English) => {
                format!("Version {version} is available.").into()
            }
            (Str::UpdateAvailableHeadline(version), Language::Vietnamese) => {
                format!("Đã có phiên bản {version}.").into()
            }
            (Str::UpdatePublished(when), Language::English) => format!("Published {when}").into(),
            (Str::UpdatePublished(when), Language::Vietnamese) => format!("Phát hành {when}").into(),
            (Str::UpdateDownloadSize(size), Language::English) => {
                format!("Download size {size}").into()
            }
            (Str::UpdateDownloadSize(size), Language::Vietnamese) => {
                format!("Dung lượng tải về {size}").into()
            }
            (Str::UpdateReleaseNotes, Language::English) => "Release notes".into(),
            (Str::UpdateReleaseNotes, Language::Vietnamese) => "Ghi chú phát hành".into(),
            (Str::UpdateDownloadAction, Language::English) => "Download and install".into(),
            (Str::UpdateDownloadAction, Language::Vietnamese) => "Tải về và cài đặt".into(),
            (
                Str::UpdateDownloadProgress {
                    done,
                    total,
                    percent,
                },
                Language::English,
            ) => format!("Downloading… {done} of {total} ({percent}%)").into(),
            (
                Str::UpdateDownloadProgress {
                    done,
                    total,
                    percent,
                },
                Language::Vietnamese,
            ) => format!("Đang tải… {done} trên {total} ({percent}%)").into(),
            (Str::UpdateVerifying, Language::English) => "Verifying the download…".into(),
            (Str::UpdateVerifying, Language::Vietnamese) => "Đang xác minh tệp tải về…".into(),
            (Str::UpdateInstalling, Language::English) => "Installing…".into(),
            (Str::UpdateInstalling, Language::Vietnamese) => "Đang cài đặt…".into(),
            (Str::UpdateInstalledHeadline(version), Language::English) => {
                format!("Version {version} is installed.").into()
            }
            (Str::UpdateInstalledHeadline(version), Language::Vietnamese) => {
                format!("Đã cài đặt phiên bản {version}.").into()
            }
            (Str::UpdateRestartNow, Language::English) => "Restart now".into(),
            (Str::UpdateRestartNow, Language::Vietnamese) => "Khởi động lại ngay".into(),
            (Str::UpdateLater, Language::English) => "Later".into(),
            (Str::UpdateLater, Language::Vietnamese) => "Để sau".into(),
            (Str::UpdateSkipVersion, Language::English) => "Skip this version".into(),
            (Str::UpdateSkipVersion, Language::Vietnamese) => "Bỏ qua phiên bản này".into(),
            (Str::UpdateCancel, Language::English) => "Cancel".into(),
            (Str::UpdateCancel, Language::Vietnamese) => "Huỷ".into(),
            (Str::UpdateRetry, Language::English) => "Try again".into(),
            (Str::UpdateRetry, Language::Vietnamese) => "Thử lại".into(),
            (Str::UpdateCheckAutomatically, Language::English) => {
                "Check for updates automatically".into()
            }
            (Str::UpdateCheckAutomatically, Language::Vietnamese) => {
                "Tự động kiểm tra cập nhật".into()
            }
            (Str::UpdateManualInstall(path), Language::English) => format!(
                "The update was downloaded and verified, but dodo cannot replace itself where \
                 it is installed. The archive is at {path}."
            )
            .into(),
            (Str::UpdateManualInstall(path), Language::Vietnamese) => format!(
                "Bản cập nhật đã được tải về và xác minh, nhưng dodo không thể tự thay thế ở \
                 vị trí đang cài. Tệp nén nằm tại {path}."
            )
            .into(),
            (Str::UpdateManualNotABundle, Language::English) => {
                "dodo is running as a plain executable rather than from an app bundle.".into()
            }
            (Str::UpdateManualNotABundle, Language::Vietnamese) => {
                "dodo đang chạy dưới dạng tệp thực thi đơn lẻ, không phải từ gói ứng dụng.".into()
            }
            (Str::UpdateManualNotWritable, Language::English) => {
                "The folder dodo is installed in cannot be written to.".into()
            }
            (Str::UpdateManualNotWritable, Language::Vietnamese) => {
                "Không thể ghi vào thư mục đang cài dodo.".into()
            }
            (Str::UpdateManualReadOnly, Language::English) => {
                "dodo is running from a read-only location.".into()
            }
            (Str::UpdateManualReadOnly, Language::Vietnamese) => {
                "dodo đang chạy từ một vị trí chỉ đọc.".into()
            }
            (Str::UpdateFailedHeadline, Language::English) => {
                "The update could not be completed.".into()
            }
            (Str::UpdateFailedHeadline, Language::Vietnamese) => {
                "Không thể hoàn tất bản cập nhật.".into()
            }
            (Str::UpdateErrorNetwork(detail), Language::English) => {
                format!("Could not reach the update server: {detail}").into()
            }
            (Str::UpdateErrorNetwork(detail), Language::Vietnamese) => {
                format!("Không kết nối được máy chủ cập nhật: {detail}").into()
            }
            (Str::UpdateErrorManifestMalformed(detail), Language::English) => {
                format!("The update manifest could not be read: {detail}").into()
            }
            (Str::UpdateErrorManifestMalformed(detail), Language::Vietnamese) => {
                format!("Không đọc được tệp kê khai cập nhật: {detail}").into()
            }
            (Str::UpdateErrorManifestMissingVersion, Language::English) => {
                "The update manifest carries no version, so dodo cannot tell how to read it."
                    .into()
            }
            (Str::UpdateErrorManifestMissingVersion, Language::Vietnamese) => {
                "Tệp kê khai cập nhật không ghi phiên bản, nên dodo không biết cách đọc nó.".into()
            }
            (
                Str::UpdateErrorManifestUnsupportedVersion { found, supported },
                Language::English,
            ) => format!(
                "The update manifest is version {found}; this dodo understands version \
                 {supported}. Update dodo by hand."
            )
            .into(),
            (
                Str::UpdateErrorManifestUnsupportedVersion { found, supported },
                Language::Vietnamese,
            ) => format!(
                "Tệp kê khai cập nhật ở phiên bản {found}; dodo này chỉ hiểu phiên bản \
                 {supported}. Hãy cập nhật dodo thủ công."
            )
            .into(),
            (Str::UpdateErrorManifestUnreadableVersion(text), Language::English) => {
                format!("The update manifest names a version dodo cannot read: {text}").into()
            }
            (Str::UpdateErrorManifestUnreadableVersion(text), Language::Vietnamese) => {
                format!("Tệp kê khai cập nhật ghi một phiên bản dodo không đọc được: {text}").into()
            }
            (Str::UpdateErrorManifestInvalidFile { platform, detail }, language) => format!(
                "{}: {}",
                match language {
                    Language::English =>
                        format!("The update manifest's {platform} entry is unusable"),
                    Language::Vietnamese =>
                        format!("Mục {platform} trong tệp kê khai cập nhật không dùng được"),
                },
                detail.text(language)
            )
            .into(),
            (Str::UpdateErrorManifestBadDigest(digest), Language::English) => {
                format!("{digest} is not a SHA-256 checksum").into()
            }
            (Str::UpdateErrorManifestBadDigest(digest), Language::Vietnamese) => {
                format!("{digest} không phải là mã băm SHA-256").into()
            }
            (Str::UpdateErrorManifestZeroSize, Language::English) => {
                "the download size is zero".into()
            }
            (Str::UpdateErrorManifestZeroSize, Language::Vietnamese) => {
                "dung lượng tải về bằng không".into()
            }
            (Str::UpdateErrorManifestInsecureUrl(url), Language::English) => {
                format!("the download address does not use https: {url}").into()
            }
            (Str::UpdateErrorManifestInsecureUrl(url), Language::Vietnamese) => {
                format!("địa chỉ tải về không dùng https: {url}").into()
            }
            (Str::UpdateErrorPlatformMissing(key), Language::English) => {
                format!("This release publishes no download for {key}.").into()
            }
            (Str::UpdateErrorPlatformMissing(key), Language::Vietnamese) => {
                format!("Bản phát hành này không có tệp tải về cho {key}.").into()
            }
            (Str::UpdateErrorDownload(detail), Language::English) => {
                format!("The download failed: {detail}").into()
            }
            (Str::UpdateErrorDownload(detail), Language::Vietnamese) => {
                format!("Tải về thất bại: {detail}").into()
            }
            (Str::UpdateErrorChecksum { expected, actual }, Language::English) => format!(
                "The download does not match the checksum this release published — expected \
                 {expected}, got {actual}. It has been discarded and nothing was installed."
            )
            .into(),
            (Str::UpdateErrorChecksum { expected, actual }, Language::Vietnamese) => format!(
                "Tệp tải về không khớp mã băm mà bản phát hành công bố — cần {expected}, nhận \
                 được {actual}. Tệp đã bị xoá và không có gì được cài đặt."
            )
            .into(),
            (Str::UpdateErrorSize { expected, actual }, Language::English) => format!(
                "The download is {actual} bytes; this release says {expected}. It has been \
                 discarded and nothing was installed."
            )
            .into(),
            (Str::UpdateErrorSize { expected, actual }, Language::Vietnamese) => format!(
                "Tệp tải về có {actual} byte; bản phát hành ghi {expected}. Tệp đã bị xoá và \
                 không có gì được cài đặt."
            )
            .into(),
            (Str::UpdateErrorInstall(detail), Language::English) => {
                format!("The update could not be installed: {detail}").into()
            }
            (Str::UpdateErrorInstall(detail), Language::Vietnamese) => {
                format!("Không thể cài đặt bản cập nhật: {detail}").into()
            }
            (Str::UpdateErrorIo(detail), Language::English) => {
                format!("A file could not be written: {detail}").into()
            }
            (Str::UpdateErrorIo(detail), Language::Vietnamese) => {
                format!("Không thể ghi tệp: {detail}").into()
            }

            (Str::DatabaseTitle, Language::English) => "Database".into(),
            (Str::DatabaseTitle, Language::Vietnamese) => "Cơ sở dữ liệu".into(),
            (Str::DbConnections, Language::English) => "Connections".into(),
            (Str::DbConnections, Language::Vietnamese) => "Các kết nối".into(),
            (Str::DbNewConnection, Language::English) => "New connection".into(),
            (Str::DbNewConnection, Language::Vietnamese) => "Kết nối mới".into(),
            (Str::DbNoConnections, Language::English) => "No connections yet".into(),
            (Str::DbNoConnections, Language::Vietnamese) => "Chưa có kết nối nào".into(),
            (Str::DbNoConnectionsHint, Language::English) => {
                "Add one to browse a database and run queries.".into()
            }
            (Str::DbNoConnectionsHint, Language::Vietnamese) => {
                "Thêm một kết nối để duyệt cơ sở dữ liệu và chạy truy vấn.".into()
            }
            (Str::DbConnect, Language::English) => "Connect".into(),
            (Str::DbConnect, Language::Vietnamese) => "Kết nối".into(),
            (Str::DbDisconnect, Language::English) => "Disconnect".into(),
            (Str::DbDisconnect, Language::Vietnamese) => "Ngắt kết nối".into(),
            (Str::DbReconnect, Language::English) => "Reconnect".into(),
            (Str::DbReconnect, Language::Vietnamese) => "Kết nối lại".into(),
            (Str::DbEditConnection, Language::English) => "Edit".into(),
            (Str::DbEditConnection, Language::Vietnamese) => "Sửa".into(),
            (Str::DbEditConnectionTitle, Language::English) => "Edit connection".into(),
            (Str::DbEditConnectionTitle, Language::Vietnamese) => "Sửa kết nối".into(),
            (Str::DbDuplicateConnection, Language::English) => "Duplicate".into(),
            (Str::DbDuplicateConnection, Language::Vietnamese) => "Nhân bản".into(),
            (Str::DbDeleteConnection, Language::English) => "Delete".into(),
            (Str::DbDeleteConnection, Language::Vietnamese) => "Xoá".into(),
            (Str::DbCopySuffix, Language::English) => "copy".into(),
            (Str::DbCopySuffix, Language::Vietnamese) => "bản sao".into(),
            (Str::DbStatusConnected, Language::English) => "Connected".into(),
            (Str::DbStatusConnected, Language::Vietnamese) => "Đã kết nối".into(),
            (Str::DbStatusConnecting, Language::English) => "Connecting…".into(),
            (Str::DbStatusConnecting, Language::Vietnamese) => "Đang kết nối…".into(),
            (Str::DbStatusDisconnected, Language::English) => "Disconnected".into(),
            (Str::DbStatusDisconnected, Language::Vietnamese) => "Chưa kết nối".into(),
            (Str::DbStatusError, Language::English) => "Error".into(),
            (Str::DbStatusError, Language::Vietnamese) => "Lỗi".into(),
            (Str::DbDeleteConnectionTitle, Language::English) => "Delete connection?".into(),
            (Str::DbDeleteConnectionTitle, Language::Vietnamese) => "Xoá kết nối?".into(),
            (Str::DbDeleteConnectionMessage(name), Language::English) => {
                format!("\"{name}\" will be removed from this list. The database itself is left alone.")
                    .into()
            }
            (Str::DbDeleteConnectionMessage(name), Language::Vietnamese) => format!(
                "“{name}” sẽ bị xoá khỏi danh sách này. Bản thân cơ sở dữ liệu không bị đụng tới."
            )
            .into(),
            (Str::DbCancel, Language::English) => "Cancel".into(),
            (Str::DbCancel, Language::Vietnamese) => "Huỷ".into(),
            (Str::DbSave, Language::English) => "Save".into(),
            (Str::DbSave, Language::Vietnamese) => "Lưu".into(),
            (Str::DbFieldName, Language::English) => "Name".into(),
            (Str::DbFieldName, Language::Vietnamese) => "Tên".into(),
            (Str::DbFieldNamePlaceholder, Language::English) => "Optional".into(),
            (Str::DbFieldNamePlaceholder, Language::Vietnamese) => "Không bắt buộc".into(),
            (Str::DbFieldEngine, Language::English) => "Type".into(),
            (Str::DbFieldEngine, Language::Vietnamese) => "Loại".into(),
            (Str::DbFieldHost, Language::English) => "Host".into(),
            (Str::DbFieldHost, Language::Vietnamese) => "Máy chủ".into(),
            (Str::DbFieldPort, Language::English) => "Port".into(),
            (Str::DbFieldPort, Language::Vietnamese) => "Cổng".into(),
            (Str::DbFieldDatabase, Language::English) => "Database".into(),
            (Str::DbFieldDatabase, Language::Vietnamese) => "Cơ sở dữ liệu".into(),
            (Str::DbFieldUser, Language::English) => "User".into(),
            (Str::DbFieldUser, Language::Vietnamese) => "Người dùng".into(),
            // The same three letters in every language, like `DbFieldSsl`.
            (Str::DbFieldUrl, _) => "URL".into(),
            (Str::DbFieldPassword, Language::English) => "Password".into(),
            (Str::DbFieldPassword, Language::Vietnamese) => "Mật khẩu".into(),
            (Str::DbFieldFile, Language::English) => "File".into(),
            (Str::DbFieldFile, Language::Vietnamese) => "Tệp".into(),
            (Str::DbFieldFilePlaceholder, Language::English) => {
                "Path to the database file".into()
            }
            (Str::DbFieldFilePlaceholder, Language::Vietnamese) => {
                "Đường dẫn tới tệp cơ sở dữ liệu".into()
            }
            // The protocol's name, the same three letters in both languages.
            (Str::DbFieldSsl, _) => "TLS".into(),
            (Str::DbSslDisable, Language::English) => "Disable".into(),
            (Str::DbSslDisable, Language::Vietnamese) => "Tắt".into(),
            (Str::DbSslPrefer, Language::English) => "Prefer".into(),
            (Str::DbSslPrefer, Language::Vietnamese) => "Ưu tiên".into(),
            (Str::DbSslRequire, Language::English) => "Require".into(),
            (Str::DbSslRequire, Language::Vietnamese) => "Bắt buộc".into(),
            (Str::DbPasswordStorageNotice, Language::English) => {
                "Saved passwords are stored unencrypted in dodo's data folder, like the API \
                 Explorer's secret variables. Anyone who can read that folder can read them."
                    .into()
            }
            (Str::DbPasswordStorageNotice, Language::Vietnamese) => {
                "Mật khẩu đã lưu được giữ ở dạng không mã hoá trong thư mục dữ liệu của dodo, \
                 giống các biến bí mật của API Explorer. Ai đọc được thư mục đó thì đọc được \
                 mật khẩu."
                    .into()
            }
            (Str::DbRevealPassword, Language::English) => "Show password".into(),
            (Str::DbRevealPassword, Language::Vietnamese) => "Hiện mật khẩu".into(),
            (Str::DbHidePassword, Language::English) => "Hide password".into(),
            (Str::DbHidePassword, Language::Vietnamese) => "Ẩn mật khẩu".into(),
            (Str::DbTestConnection, Language::English) => "Test connection".into(),
            (Str::DbTestConnection, Language::Vietnamese) => "Thử kết nối".into(),
            (Str::DbTesting, Language::English) => "Testing…".into(),
            (Str::DbTesting, Language::Vietnamese) => "Đang thử…".into(),
            (Str::DbTestSucceeded, Language::English) => "The connection works.".into(),
            (Str::DbTestSucceeded, Language::Vietnamese) => "Kết nối hoạt động tốt.".into(),
            (Str::DbProfileHostMissing, Language::English) => "Enter a host.".into(),
            (Str::DbProfileHostMissing, Language::Vietnamese) => "Hãy nhập máy chủ.".into(),
            (Str::DbProfilePortMissing, Language::English) => "Enter a port.".into(),
            (Str::DbProfilePortMissing, Language::Vietnamese) => "Hãy nhập cổng.".into(),
            (Str::DbProfileDatabaseMissing, Language::English) => {
                "Enter a database name.".into()
            }
            (Str::DbProfileDatabaseMissing, Language::Vietnamese) => {
                "Hãy nhập tên cơ sở dữ liệu.".into()
            }
            (Str::DbProfileFileMissing, Language::English) => "Choose a database file.".into(),
            (Str::DbProfileFileMissing, Language::Vietnamese) => {
                "Hãy chọn tệp cơ sở dữ liệu.".into()
            }
            (Str::DbGroupTables, Language::English) => "Tables".into(),
            (Str::DbGroupTables, Language::Vietnamese) => "Bảng".into(),
            (Str::DbGroupViews, Language::English) => "Views".into(),
            (Str::DbGroupViews, Language::Vietnamese) => "Khung nhìn".into(),
            (Str::DbGroupColumns, Language::English) => "Columns".into(),
            (Str::DbGroupColumns, Language::Vietnamese) => "Cột".into(),
            (Str::DbGroupIndexes, Language::English) => "Indexes".into(),
            (Str::DbGroupIndexes, Language::Vietnamese) => "Chỉ mục".into(),
            (Str::DbGroupConstraints, Language::English) => "Constraints".into(),
            (Str::DbGroupConstraints, Language::Vietnamese) => "Ràng buộc".into(),
            (Str::DbTreeLoading, Language::English) => "Loading…".into(),
            (Str::DbTreeLoading, Language::Vietnamese) => "Đang tải…".into(),
            (Str::DbTreeEmpty, Language::English) => "Nothing here".into(),
            (Str::DbTreeEmpty, Language::Vietnamese) => "Không có gì".into(),
            (Str::DbTreeNotConnected, Language::English) => "Not connected".into(),
            (Str::DbTreeNotConnected, Language::Vietnamese) => "Chưa kết nối".into(),
            (Str::DbRefreshTree, Language::English) => "Refresh".into(),
            (Str::DbRefreshTree, Language::Vietnamese) => "Tải lại".into(),
            (Str::DbQuery, Language::English) => "Query".into(),
            (Str::DbQuery, Language::Vietnamese) => "Truy vấn".into(),
            (Str::DbQueryPlaceholder, Language::English) => {
                "Write SQL here, then press Execute.".into()
            }
            (Str::DbQueryPlaceholder, Language::Vietnamese) => {
                "Viết SQL ở đây rồi nhấn Chạy.".into()
            }
            (Str::DbExecute, Language::English) => "Execute".into(),
            (Str::DbExecute, Language::Vietnamese) => "Chạy".into(),
            (Str::DbFormat, Language::English) => "Format".into(),
            (Str::DbFormat, Language::Vietnamese) => "Định dạng".into(),
            (Str::DbRunning, Language::English) => "Running…".into(),
            (Str::DbRunning, Language::Vietnamese) => "Đang chạy…".into(),
            (Str::DbNoStatement, Language::English) => "There is nothing to run.".into(),
            (Str::DbNoStatement, Language::Vietnamese) => "Không có gì để chạy.".into(),
            (Str::DbResult, Language::English) => "Result".into(),
            (Str::DbResult, Language::Vietnamese) => "Kết quả".into(),
            (Str::DbNoResultYet, Language::English) => "No result yet".into(),
            (Str::DbNoResultYet, Language::Vietnamese) => "Chưa có kết quả".into(),
            (Str::DbNoResultYetHint, Language::English) => {
                "Run a statement to see its rows here.".into()
            }
            (Str::DbNoResultYetHint, Language::Vietnamese) => {
                "Chạy một câu lệnh để xem các dòng của nó ở đây.".into()
            }
            (Str::DbNoRows, Language::English) => "The statement returned no rows.".into(),
            (Str::DbNoRows, Language::Vietnamese) => "Câu lệnh không trả về dòng nào.".into(),
            (Str::DbFooterRows(count), Language::English) => match count {
                1 => "1 row".into(),
                other => format!("{other} rows").into(),
            },
            (Str::DbFooterRows(count), Language::Vietnamese) => format!("{count} dòng").into(),
            (Str::DbFooterRowsAffected(count), Language::English) => match count {
                1 => "1 row affected".into(),
                other => format!("{other} rows affected").into(),
            },
            (Str::DbFooterRowsAffected(count), Language::Vietnamese) => {
                format!("{count} dòng bị ảnh hưởng").into()
            }
            (Str::DbFooterElapsed(elapsed), Language::English) => format!("in {elapsed}").into(),
            (Str::DbFooterElapsed(elapsed), Language::Vietnamese) => {
                format!("trong {elapsed}").into()
            }
            (Str::DbFooterTruncated(shown), Language::English) => {
                format!("showing the first {shown} — the statement returned more").into()
            }
            (Str::DbFooterTruncated(shown), Language::Vietnamese) => {
                format!("chỉ hiện {shown} dòng đầu — câu lệnh trả về nhiều hơn").into()
            }
            (Str::DbFooterCapped(count), Language::English) => {
                format!("{count} large values shortened").into()
            }
            (Str::DbFooterCapped(count), Language::Vietnamese) => {
                format!("{count} giá trị lớn đã được rút gọn").into()
            }
            (Str::DbStatementLabel, Language::English) => "Statement".into(),
            (Str::DbStatementLabel, Language::Vietnamese) => "Câu lệnh".into(),
            // SQL's own word for "no value". The same four letters in both
            // languages; translating it would make a cell unreadable.
            (Str::DbColumnNull, _) => "NULL".into(),
            (Str::DbSelectConnection, Language::English) => "Select a connection".into(),
            (Str::DbSelectConnection, Language::Vietnamese) => "Chọn một kết nối".into(),
            (Str::DbSelectConnectionHint, Language::English) => {
                "Choose one on the left to browse it and run queries.".into()
            }
            (Str::DbSelectConnectionHint, Language::Vietnamese) => {
                "Chọn một kết nối ở bên trái để duyệt và chạy truy vấn.".into()
            }
            (Str::DbConnectionStoreError(detail), Language::English) => {
                format!("Connections could not be saved: {detail}").into()
            }
            (Str::DbConnectionStoreError(detail), Language::Vietnamese) => {
                format!("Không thể lưu các kết nối: {detail}").into()
            }
            (Str::DbConnectionStoreMissingVersion, Language::English) => {
                "The saved connections file carries no schema version, so it cannot be read.".into()
            }
            (Str::DbConnectionStoreMissingVersion, Language::Vietnamese) => {
                "Tệp kết nối đã lưu không có phiên bản lược đồ nên không thể đọc được.".into()
            }
            (
                Str::DbConnectionStoreUnsupportedVersion { found, supported },
                Language::English,
            ) => format!(
                "The saved connections were written by a newer dodo (version {found}; this build \
                 understands {supported}). Update dodo to open them."
            )
            .into(),
            (
                Str::DbConnectionStoreUnsupportedVersion { found, supported },
                Language::Vietnamese,
            ) => format!(
                "Các kết nối đã lưu được ghi bởi một bản dodo mới hơn (phiên bản {found}; bản này \
                 hiểu {supported}). Hãy cập nhật dodo để mở chúng."
            )
            .into(),
            (Str::DbUnreachable(detail), Language::English) => {
                format!("The database could not be reached: {detail}").into()
            }
            (Str::DbUnreachable(detail), Language::Vietnamese) => {
                format!("Không thể kết nối tới cơ sở dữ liệu: {detail}").into()
            }
            (Str::DbServerError(detail), Language::English) => {
                format!("The server rejected the statement: {detail}").into()
            }
            (Str::DbServerError(detail), Language::Vietnamese) => {
                format!("Máy chủ từ chối câu lệnh: {detail}").into()
            }
            (Str::DbServerErrorCoded { code, detail }, Language::English) => {
                format!("The server rejected the statement ({code}): {detail}").into()
            }
            (Str::DbServerErrorCoded { code, detail }, Language::Vietnamese) => {
                format!("Máy chủ từ chối câu lệnh ({code}): {detail}").into()
            }

            (Str::DbQueryTabTitle(number), Language::English) => format!("Query {number}").into(),
            (Str::DbQueryTabTitle(number), Language::Vietnamese) => {
                format!("Truy vấn {number}").into()
            }
            (Str::DbNewQueryTab, Language::English) => "New query".into(),
            (Str::DbNewQueryTab, Language::Vietnamese) => "Truy vấn mới".into(),
            (Str::DbCloseQueryTab, Language::English) => "Close query".into(),
            (Str::DbCloseQueryTab, Language::Vietnamese) => "Đóng truy vấn".into(),

            (Str::DbCancelQuery, Language::English) => "Cancel".into(),
            (Str::DbCancelQuery, Language::Vietnamese) => "Huỷ".into(),
            (Str::DbCancelledMessage, Language::English) => {
                "The server stopped the statement because you cancelled it.".into()
            }
            (Str::DbCancelledMessage, Language::Vietnamese) => {
                "Máy chủ đã dừng câu lệnh vì bạn huỷ nó.".into()
            }
            (Str::DbCancelledTitle, Language::English) => "Cancelled".into(),
            (Str::DbCancelledTitle, Language::Vietnamese) => "Đã huỷ".into(),
            (Str::DbCancelledHint, Language::English) => {
                "The server confirmed it stopped, so nothing is still running there.".into()
            }
            (Str::DbCancelledHint, Language::Vietnamese) => {
                "Máy chủ xác nhận đã dừng, nên không còn gì đang chạy ở đó.".into()
            }
            (Str::DbCancelFailed(detail), Language::English) => format!(
                "dodo could not reach the server to cancel, so the statement may still be \
                 running: {detail}"
            )
            .into(),
            (Str::DbCancelFailed(detail), Language::Vietnamese) => format!(
                "Dodo không liên hệ được máy chủ để huỷ, nên câu lệnh có thể vẫn đang chạy: \
                 {detail}"
            )
            .into(),

            (Str::DbExplain, Language::English) => "Explain".into(),
            (Str::DbExplain, Language::Vietnamese) => "Giải thích".into(),

            (Str::DbCopyCell, Language::English) => "Copy cell".into(),
            (Str::DbCopyCell, Language::Vietnamese) => "Sao chép ô".into(),
            (Str::DbCopyRow, Language::English) => "Copy row".into(),
            (Str::DbCopyRow, Language::Vietnamese) => "Sao chép dòng".into(),

            (Str::DbExportCsv, Language::English) => "Export CSV".into(),
            (Str::DbExportCsv, Language::Vietnamese) => "Xuất CSV".into(),
            (Str::DbExportJson, Language::English) => "Export JSON".into(),
            (Str::DbExportJson, Language::Vietnamese) => "Xuất JSON".into(),
            (Str::DbExportSucceeded { rows, path }, Language::English) => {
                format!("Exported {rows} rows to {path}.").into()
            }
            (Str::DbExportSucceeded { rows, path }, Language::Vietnamese) => {
                format!("Đã xuất {rows} dòng vào {path}.").into()
            }
            (Str::DbExportCancelled, Language::English) => "Export cancelled.".into(),
            (Str::DbExportCancelled, Language::Vietnamese) => "Đã huỷ xuất dữ liệu.".into(),
            (Str::DbExportFailed(detail), Language::English) => {
                format!("The result could not be exported: {detail}").into()
            }
            (Str::DbExportFailed(detail), Language::Vietnamese) => {
                format!("Không thể xuất kết quả: {detail}").into()
            }

            (Str::DbHistory, Language::English) => "History".into(),
            (Str::DbHistory, Language::Vietnamese) => "Lịch sử".into(),
            (Str::DbHistorySearch, Language::English) => "Search query history…".into(),
            (Str::DbHistorySearch, Language::Vietnamese) => "Tìm trong lịch sử truy vấn…".into(),
            (Str::DbHistoryEmpty, Language::English) => "No queries have run yet.".into(),
            (Str::DbHistoryEmpty, Language::Vietnamese) => "Chưa có truy vấn nào được chạy.".into(),
            (Str::DbHistoryNoMatches, Language::English) => "No matching queries.".into(),
            (Str::DbHistoryNoMatches, Language::Vietnamese) => "Không có truy vấn phù hợp.".into(),

            (Str::DbDetailData, Language::English) => "Data".into(),
            (Str::DbDetailData, Language::Vietnamese) => "Dữ liệu".into(),
            (Str::DbDetailDdl, _) => "DDL".into(),
            (Str::DbDetailFieldNullable, Language::English) => "Nullable".into(),
            (Str::DbDetailFieldNullable, Language::Vietnamese) => "Cho phép NULL".into(),
            (Str::DbDetailFieldNotNull, Language::English) => "Not null".into(),
            (Str::DbDetailFieldNotNull, Language::Vietnamese) => "Không NULL".into(),
            (Str::DbDetailFieldDefault, Language::English) => "Default".into(),
            (Str::DbDetailFieldDefault, Language::Vietnamese) => "Mặc định".into(),
            (Str::DbDetailFieldUnique, Language::English) => "Unique".into(),
            (Str::DbDetailFieldUnique, Language::Vietnamese) => "Duy nhất".into(),
            (Str::DbDetailFieldPrimary, Language::English) => "Primary".into(),
            (Str::DbDetailFieldPrimary, Language::Vietnamese) => "Chính".into(),
            (Str::DbDetailFieldDefinition, Language::English) => "Definition".into(),
            (Str::DbDetailFieldDefinition, Language::Vietnamese) => "Định nghĩa".into(),
            (Str::DbDetailClose, Language::English) => "Close object detail".into(),
            (Str::DbDetailClose, Language::Vietnamese) => "Đóng chi tiết đối tượng".into(),
            (Str::DbDetailUnavailable, Language::English) => {
                "This detail is not available for this object.".into()
            }
            (Str::DbDetailUnavailable, Language::Vietnamese) => {
                "Chi tiết này không có sẵn cho đối tượng này.".into()
            }
            (Str::DbDetailNoRows, Language::English) => "This object has no rows.".into(),
            (Str::DbDetailNoRows, Language::Vietnamese) => {
                "Đối tượng này không có dòng nào.".into()
            }
            (Str::DbDetailNoMetadata, Language::English) => "No metadata was reported.".into(),
            (Str::DbDetailNoMetadata, Language::Vietnamese) => {
                "Không có siêu dữ liệu nào được báo cáo.".into()
            }
            (Str::DbDetailPrevious, Language::English) => "Previous".into(),
            (Str::DbDetailPrevious, Language::Vietnamese) => "Trước".into(),
            (Str::DbDetailNext, Language::English) => "Next".into(),
            (Str::DbDetailNext, Language::Vietnamese) => "Tiếp".into(),
            (Str::DbDetailPage(page), Language::English) => format!("Page {page}").into(),
            (Str::DbDetailPage(page), Language::Vietnamese) => format!("Trang {page}").into(),
            (Str::DbDetailRowsRange { first, last }, Language::English) => {
                format!("Rows {first}–{last}").into()
            }
            (Str::DbDetailRowsRange { first, last }, Language::Vietnamese) => {
                format!("Dòng {first}–{last}").into()
            }
            (Str::DbDetailDdlReconstructed, Language::English) => {
                "Reconstructed from PostgreSQL catalog metadata; partitioning, inheritance, \
                 storage settings, comments and ownership may be omitted."
                    .into()
            }
            (Str::DbDetailDdlReconstructed, Language::Vietnamese) => {
                "Được dựng lại từ siêu dữ liệu danh mục PostgreSQL; có thể thiếu phân vùng, kế \
                 thừa, thiết lập lưu trữ, chú thích và quyền sở hữu."
                    .into()
            }
            (Str::DbDetailConstraintsPartial, Language::English) => {
                "SQLite does not expose CHECK constraints as catalog rows. See the stored DDL for \
                 the complete definition."
                    .into()
            }
            (Str::DbDetailConstraintsPartial, Language::Vietnamese) => {
                "SQLite không cung cấp ràng buộc CHECK dưới dạng dòng danh mục. Xem DDL đã lưu \
                 để biết định nghĩa đầy đủ."
                    .into()
            }
            (Str::DbDetailCopyDdl, Language::English) => "Copy DDL".into(),
            (Str::DbDetailCopyDdl, Language::Vietnamese) => "Sao chép DDL".into(),
            (Str::DbDetailMetadataTruncated(count), Language::English) => {
                format!("Showing the first {count} metadata rows.").into()
            }
            (Str::DbDetailMetadataTruncated(count), Language::Vietnamese) => {
                format!("Đang hiện {count} dòng siêu dữ liệu đầu tiên.").into()
            }
            (Str::DbProfileRedisDatabaseInvalid, Language::English) => {
                "Enter a non-negative logical database number.".into()
            }
            (Str::DbProfileRedisDatabaseInvalid, Language::Vietnamese) => {
                "Hãy nhập số cơ sở dữ liệu logic không âm.".into()
            }
            (Str::DbGroupMore, Language::English) => "More…".into(),
            (Str::DbGroupMore, Language::Vietnamese) => "Thêm…".into(),
            (Str::DbCommandPlaceholder, Language::English) => {
                "Enter one Redis command per line.".into()
            }
            (Str::DbCommandPlaceholder, Language::Vietnamese) => {
                "Nhập một lệnh Redis trên mỗi dòng.".into()
            }
            (Str::DbEditUnsupported, Language::English) => {
                "This result is read-only: this database does not support safe table editing.".into()
            }
            (Str::DbEditUnsupported, Language::Vietnamese) => {
                "Kết quả này chỉ đọc: cơ sở dữ liệu này không hỗ trợ chỉnh sửa bảng an toàn.".into()
            }
            (Str::DbEditNoColumns, Language::English) => {
                "This result is read-only because it has no columns.".into()
            }
            (Str::DbEditNoColumns, Language::Vietnamese) => {
                "Kết quả này chỉ đọc vì không có cột.".into()
            }
            (Str::DbEditMissingOrigin(column), Language::English) => format!(
                "This result is read-only: column {column} does not come from one base table."
            )
            .into(),
            (Str::DbEditMissingOrigin(column), Language::Vietnamese) => format!(
                "Kết quả này chỉ đọc: cột {column} không đến từ một bảng cơ sở."
            )
            .into(),
            (Str::DbEditMultipleTables, Language::English) => {
                "This result is read-only because it combines several tables.".into()
            }
            (Str::DbEditMultipleTables, Language::Vietnamese) => {
                "Kết quả này chỉ đọc vì kết hợp nhiều bảng.".into()
            }
            (Str::DbEditDuplicateColumn(column), Language::English) => format!(
                "This result is read-only because base column {column} appears more than once."
            )
            .into(),
            (Str::DbEditDuplicateColumn(column), Language::Vietnamese) => format!(
                "Kết quả này chỉ đọc vì cột cơ sở {column} xuất hiện nhiều lần."
            )
            .into(),
            (Str::DbEditNoUniqueIdentity(table), Language::English) => format!(
                "This result is read-only: {table} has no primary key or non-null unique index."
            )
            .into(),
            (Str::DbEditNoUniqueIdentity(table), Language::Vietnamese) => format!(
                "Kết quả này chỉ đọc: {table} không có khóa chính hoặc chỉ mục duy nhất không NULL."
            )
            .into(),
            (
                Str::DbEditMissingIdentityColumns { table, columns },
                Language::English,
            ) => format!(
                "This result is read-only: identity column(s) {columns} from {table} are not in the result."
            )
            .into(),
            (
                Str::DbEditMissingIdentityColumns { table, columns },
                Language::Vietnamese,
            ) => format!(
                "Kết quả này chỉ đọc: (các) cột định danh {columns} của {table} không có trong kết quả."
            )
            .into(),
            (Str::DbEditMetadataFailed(detail), Language::English) => {
                format!("This result is read-only because identity metadata could not be loaded: {detail}").into()
            }
            (Str::DbEditMetadataFailed(detail), Language::Vietnamese) => {
                format!("Kết quả này chỉ đọc vì không thể tải siêu dữ liệu định danh: {detail}").into()
            }
            (Str::DbEditCell, Language::English) => "Edit cell".into(),
            (Str::DbEditCell, Language::Vietnamese) => "Sửa ô".into(),
            (Str::DbAddRow, Language::English) => "Add row".into(),
            (Str::DbAddRow, Language::Vietnamese) => "Thêm dòng".into(),
            (Str::DbDeleteRow, Language::English) => "Delete row".into(),
            (Str::DbDeleteRow, Language::Vietnamese) => "Xóa dòng".into(),
            (Str::DbDuplicateRow, Language::English) => "Duplicate row".into(),
            (Str::DbDuplicateRow, Language::Vietnamese) => "Nhân đôi dòng".into(),
            (Str::DbCommit, Language::English) => "Commit".into(),
            (Str::DbCommit, Language::Vietnamese) => "Ghi thay đổi".into(),
            (Str::DbRollback, Language::English) => "Rollback".into(),
            (Str::DbRollback, Language::Vietnamese) => "Hoàn tác".into(),
            (Str::DbEditSelectRow, Language::English) => "Select a row first.".into(),
            (Str::DbEditSelectRow, Language::Vietnamese) => "Trước tiên hãy chọn một dòng.".into(),
            (Str::DbEditIdentityColumn, Language::English) => {
                "Identity columns cannot be edited in place.".into()
            }
            (Str::DbEditIdentityColumn, Language::Vietnamese) => {
                "Không thể sửa trực tiếp cột định danh.".into()
            }
            (Str::DbEditIdentityUnavailable, Language::English) => {
                "This row cannot be changed because its complete identity value is unavailable.".into()
            }
            (Str::DbEditIdentityUnavailable, Language::Vietnamese) => {
                "Không thể thay đổi dòng này vì giá trị định danh đầy đủ không có sẵn.".into()
            }
            (Str::DbEditUnsupportedCell, Language::English) => {
                "This cell cannot be edited safely in this result.".into()
            }
            (Str::DbEditUnsupportedCell, Language::Vietnamese) => {
                "Không thể chỉnh sửa ô này một cách an toàn trong kết quả này.".into()
            }
            (Str::DbEditNoPending, Language::English) => "There are no pending changes.".into(),
            (Str::DbEditNoPending, Language::Vietnamese) => "Không có thay đổi đang chờ.".into(),
            (Str::DbPendingChanges(count), Language::English) => {
                format!("{count} pending row change(s)").into()
            }
            (Str::DbPendingChanges(count), Language::Vietnamese) => {
                format!("{count} thay đổi dòng đang chờ").into()
            }
            (Str::DbEditCellTitle(column), Language::English) => {
                format!("Edit {column}").into()
            }
            (Str::DbEditCellTitle(column), Language::Vietnamese) => {
                format!("Sửa {column}").into()
            }
            (Str::DbAddRowTitle, Language::English) => "Add row".into(),
            (Str::DbAddRowTitle, Language::Vietnamese) => "Thêm dòng".into(),
            (Str::DbDuplicateRowTitle, Language::English) => "Duplicate row".into(),
            (Str::DbDuplicateRowTitle, Language::Vietnamese) => "Nhân đôi dòng".into(),
            (Str::DbSetNull, Language::English) => "NULL".into(),
            (Str::DbSetNull, Language::Vietnamese) => "NULL".into(),
            (Str::DbIdentityRequired(columns), Language::English) => format!(
                "Enter a new value for non-generated identity column(s): {columns}."
            )
            .into(),
            (Str::DbIdentityRequired(columns), Language::Vietnamese) => format!(
                "Nhập giá trị mới cho (các) cột định danh không tự sinh: {columns}."
            )
            .into(),
            (Str::DbCommitTitle, Language::English) => "Confirm database changes".into(),
            (Str::DbCommitTitle, Language::Vietnamese) => "Xác nhận thay đổi cơ sở dữ liệu".into(),
            (Str::DbCommitSummary(count), Language::English) => format!(
                "This transaction expects exactly {count} affected row(s). Review every statement before committing."
            )
            .into(),
            (Str::DbCommitSummary(count), Language::Vietnamese) => format!(
                "Giao dịch này dự kiến tác động chính xác {count} dòng. Hãy xem từng câu lệnh trước khi ghi thay đổi."
            )
            .into(),
            (Str::DbCommitExactStatements, Language::English) => "Generated statements".into(),
            (Str::DbCommitExactStatements, Language::Vietnamese) => "Các câu lệnh đã tạo".into(),
            (Str::DbCommitParameters, Language::English) => "Bound parameters".into(),
            (Str::DbCommitParameters, Language::Vietnamese) => "Tham số liên kết".into(),
            (Str::DbCommitLostUpdateNotice, Language::English) => {
                "Concurrent changes are not detected in this version; committing may overwrite a newer value from another client.".into()
            }
            (Str::DbCommitLostUpdateNotice, Language::Vietnamese) => {
                "Phiên bản này không phát hiện thay đổi đồng thời; ghi thay đổi có thể ghi đè giá trị mới hơn từ máy khách khác.".into()
            }
            (Str::DbCommitRunning, Language::English) => "Committing changes…".into(),
            (Str::DbCommitRunning, Language::Vietnamese) => "Đang ghi thay đổi…".into(),
            (Str::DbCommitSucceeded(count), Language::English) => {
                format!("Committed {count} row change(s).").into()
            }
            (Str::DbCommitSucceeded(count), Language::Vietnamese) => {
                format!("Đã ghi {count} thay đổi dòng.").into()
            }
            (
                Str::DbCommitAffectedMismatch { statement, actual },
                Language::English,
            ) => format!(
                "Statement {statement} matched {actual} rows instead of exactly 1. The whole transaction was rolled back."
            )
            .into(),
            (
                Str::DbCommitAffectedMismatch { statement, actual },
                Language::Vietnamese,
            ) => format!(
                "Câu lệnh {statement} khớp {actual} dòng thay vì chính xác 1. Toàn bộ giao dịch đã được hoàn tác."
            )
            .into(),
            (Str::DbCommitFailed { statement, detail }, Language::English) => format!(
                "Statement {statement} failed: {detail}. The whole transaction was rolled back."
            )
            .into(),
            (Str::DbCommitFailed { statement, detail }, Language::Vietnamese) => format!(
                "Câu lệnh {statement} thất bại: {detail}. Toàn bộ giao dịch đã được hoàn tác."
            )
            .into(),
            (Str::DbCommitTransactionFailed(detail), Language::English) => {
                format!("The transaction could not complete: {detail}").into()
            }
            (Str::DbCommitTransactionFailed(detail), Language::Vietnamese) => {
                format!("Không thể hoàn tất giao dịch: {detail}").into()
            }
            (Str::DbCommitStatementLabel(number), Language::English) => {
                format!("Statement {number}").into()
            }
            (Str::DbCommitStatementLabel(number), Language::Vietnamese) => {
                format!("Câu lệnh {number}").into()
            }
            (Str::DbExpectedOneRow, Language::English) => "Expected affected rows: 1".into(),
            (Str::DbExpectedOneRow, Language::Vietnamese) => "Số dòng dự kiến tác động: 1".into(),
            (Str::DbCommitBuildFailed, Language::English) => {
                "The pending changes could not be generated safely.".into()
            }
            (Str::DbCommitBuildFailed, Language::Vietnamese) => {
                "Không thể tạo các thay đổi đang chờ một cách an toàn.".into()
            }
            (Str::DbResolvePending, Language::English) => {
                "Commit or Rollback the pending changes first.".into()
            }
            (Str::DbResolvePending, Language::Vietnamese) => {
                "Trước tiên hãy Ghi thay đổi hoặc Hoàn tác các thay đổi đang chờ.".into()
            }
            (Str::DbEditDuplicateRows, Language::English) => {
                "This result is read-only because more than one displayed row has the same unique identity.".into()
            }
            (Str::DbEditDuplicateRows, Language::Vietnamese) => {
                "Kết quả này chỉ đọc vì nhiều dòng đang hiển thị có cùng một định danh duy nhất.".into()
            }

            (Str::DbQueryStoreError(detail), Language::English) => {
                format!("Saved queries and history could not be read or written: {detail}").into()
            }
            (Str::DbQueryStoreError(detail), Language::Vietnamese) => {
                format!("Không thể đọc hoặc ghi truy vấn đã lưu và lịch sử: {detail}").into()
            }
            (Str::DbQueryStoreMissingVersion, Language::English) => {
                "The saved-query file has no version and was not loaded.".into()
            }
            (Str::DbQueryStoreMissingVersion, Language::Vietnamese) => {
                "Tệp truy vấn đã lưu không có phiên bản nên chưa được tải.".into()
            }
            (
                Str::DbQueryStoreUnsupportedVersion { found, supported },
                Language::English,
            ) => format!(
                "The saved-query file uses version {found}; this Dodo supports up to {supported}."
            )
            .into(),
            (
                Str::DbQueryStoreUnsupportedVersion { found, supported },
                Language::Vietnamese,
            ) => format!(
                "Tệp truy vấn đã lưu dùng phiên bản {found}; Dodo này chỉ hỗ trợ đến {supported}."
            )
            .into(),
            (Str::DbSavedQueries, Language::English) => "Saved queries".into(),
            (Str::DbSavedQueries, Language::Vietnamese) => "Truy vấn đã lưu".into(),
            (Str::DbSaveQuery, Language::English) => "Save query".into(),
            (Str::DbSaveQuery, Language::Vietnamese) => "Lưu truy vấn".into(),
            (Str::DbSavedQuerySearch, Language::English) => "Search saved queries…".into(),
            (Str::DbSavedQuerySearch, Language::Vietnamese) => "Tìm truy vấn đã lưu…".into(),
            (Str::DbSavedQueryEmpty, Language::English) => "No saved queries yet.".into(),
            (Str::DbSavedQueryEmpty, Language::Vietnamese) => "Chưa có truy vấn nào được lưu.".into(),
            (Str::DbSavedQueryNoMatches, Language::English) => "No matching saved queries.".into(),
            (Str::DbSavedQueryNoMatches, Language::Vietnamese) => "Không có truy vấn đã lưu phù hợp.".into(),
            (Str::DbSavedQueryCreateTitle, Language::English) => "Save query".into(),
            (Str::DbSavedQueryCreateTitle, Language::Vietnamese) => "Lưu truy vấn".into(),
            (Str::DbSavedQueryEditTitle, Language::English) => "Edit saved query".into(),
            (Str::DbSavedQueryEditTitle, Language::Vietnamese) => "Sửa truy vấn đã lưu".into(),
            (Str::DbSavedQueryName, Language::English) => "Name".into(),
            (Str::DbSavedQueryName, Language::Vietnamese) => "Tên".into(),
            (Str::DbSavedQueryNamePlaceholder, Language::English) => "e.g. Recent orders".into(),
            (Str::DbSavedQueryNamePlaceholder, Language::Vietnamese) => "ví dụ: Đơn hàng gần đây".into(),
            (Str::DbSavedQueryStatement, Language::English) => "Query".into(),
            (Str::DbSavedQueryStatement, Language::Vietnamese) => "Truy vấn".into(),
            (Str::DbSavedQueryScope, Language::English) => "Connection".into(),
            (Str::DbSavedQueryScope, Language::Vietnamese) => "Kết nối".into(),
            (Str::DbSavedQueryPlaintextNotice, Language::English) => {
                "Saved queries are stored as plain text on this device. Remove passwords and other secrets before saving."
                    .into()
            }
            (Str::DbSavedQueryPlaintextNotice, Language::Vietnamese) => {
                "Truy vấn được lưu dưới dạng văn bản thuần trên thiết bị này. Hãy xóa mật khẩu và bí mật khác trước khi lưu."
                    .into()
            }
            (Str::DbSavedQueryNameRequired, Language::English) => "Enter a name for this query.".into(),
            (Str::DbSavedQueryNameRequired, Language::Vietnamese) => "Hãy nhập tên cho truy vấn này.".into(),
            (Str::DbSavedQueryStatementRequired, Language::English) => "Enter query text to save.".into(),
            (Str::DbSavedQueryStatementRequired, Language::Vietnamese) => "Hãy nhập nội dung truy vấn để lưu.".into(),
            (Str::DbSavedQueryEdit, Language::English) => "Edit saved query".into(),
            (Str::DbSavedQueryEdit, Language::Vietnamese) => "Sửa truy vấn đã lưu".into(),
            (Str::DbSavedQueryDelete, Language::English) => "Delete saved query".into(),
            (Str::DbSavedQueryDelete, Language::Vietnamese) => "Xóa truy vấn đã lưu".into(),
            (Str::DbSavedQueryDeleteTitle, Language::English) => "Delete saved query?".into(),
            (Str::DbSavedQueryDeleteTitle, Language::Vietnamese) => "Xóa truy vấn đã lưu?".into(),
            (Str::DbSavedQueryDeleteMessage(name), Language::English) => {
                format!("Delete “{name}”? This cannot be undone.").into()
            }
            (Str::DbSavedQueryDeleteMessage(name), Language::Vietnamese) => {
                format!("Xóa “{name}”? Không thể hoàn tác thao tác này.").into()
            }
            (Str::DbSavedQueryScopeMismatch(name), Language::English) => format!(
                "Opened as text only because its saved connection “{name}” is missing or now points elsewhere. Select the intended connection before running it."
            )
            .into(),
            (Str::DbSavedQueryScopeMismatch(name), Language::Vietnamese) => format!(
                "Chỉ mở dưới dạng văn bản vì kết nối đã lưu “{name}” không còn hoặc hiện trỏ đến nơi khác. Hãy chọn đúng kết nối trước khi chạy."
            )
            .into(),
            (Str::DbHistoryClear, Language::English) => "Clear history".into(),
            (Str::DbHistoryClear, Language::Vietnamese) => "Xóa lịch sử".into(),
            (Str::DbHistoryClearTitle, Language::English) => "Clear query history?".into(),
            (Str::DbHistoryClearTitle, Language::Vietnamese) => "Xóa lịch sử truy vấn?".into(),
            (Str::DbHistoryClearMessage, Language::English) => {
                "Delete all persisted query history? Saved queries are not affected.".into()
            }
            (Str::DbHistoryClearMessage, Language::Vietnamese) => {
                "Xóa toàn bộ lịch sử truy vấn đã lưu? Các truy vấn đã lưu sẽ không bị ảnh hưởng.".into()
            }
            (Str::DbHistorySucceeded, Language::English) => "Succeeded".into(),
            (Str::DbHistorySucceeded, Language::Vietnamese) => "Thành công".into(),
            (Str::DbHistoryFailed, Language::English) => "Failed".into(),
            (Str::DbHistoryFailed, Language::Vietnamese) => "Thất bại".into(),
            (Str::DbHistoryJustNow, Language::English) => "Just now".into(),
            (Str::DbHistoryJustNow, Language::Vietnamese) => "Vừa xong".into(),
            (Str::DbHistoryMinutesAgo(minutes), Language::English) => format!("{minutes}m ago").into(),
            (Str::DbHistoryMinutesAgo(minutes), Language::Vietnamese) => format!("{minutes} phút trước").into(),
            (Str::DbHistoryHoursAgo(hours), Language::English) => format!("{hours}h ago").into(),
            (Str::DbHistoryHoursAgo(hours), Language::Vietnamese) => format!("{hours} giờ trước").into(),
            (Str::DbHistoryDaysAgo(days), Language::English) => format!("{days}d ago").into(),
            (Str::DbHistoryDaysAgo(days), Language::Vietnamese) => format!("{days} ngày trước").into(),

            (Str::DbCatalogSearch, Language::English) => "Search catalogs".into(),
            (Str::DbCatalogSearch, Language::Vietnamese) => "Tìm trong danh mục".into(),
            (Str::DbCatalogSearchPlaceholder, Language::English) => "Search catalog objects…".into(),
            (Str::DbCatalogSearchPlaceholder, Language::Vietnamese) => "Tìm đối tượng danh mục…".into(),
            (Str::DbCatalogSearchLoading, Language::English) => "Loading connected catalogs…".into(),
            (Str::DbCatalogSearchLoading, Language::Vietnamese) => "Đang tải danh mục đã kết nối…".into(),
            (Str::DbCatalogSearchEmpty, Language::English) => "No catalog objects were found.".into(),
            (Str::DbCatalogSearchEmpty, Language::Vietnamese) => "Không tìm thấy đối tượng danh mục nào.".into(),
            (Str::DbCatalogSearchNoMatches, Language::English) => "No matching catalog objects.".into(),
            (Str::DbCatalogSearchNoMatches, Language::Vietnamese) => "Không có đối tượng danh mục phù hợp.".into(),
            (Str::DbCatalogSearchConnectedOnly, Language::English) => {
                "Search covers connected databases and builds one bounded in-memory catalog cache."
                    .into()
            }
            (Str::DbCatalogSearchConnectedOnly, Language::Vietnamese) => {
                "Tìm kiếm bao gồm các cơ sở dữ liệu đã kết nối và tạo một bộ nhớ đệm danh mục trong bộ nhớ có giới hạn."
                    .into()
            }
            (Str::DbCatalogSearchTruncated(count), Language::English) => {
                format!("Search stopped at the catalog limit after indexing {count} objects.").into()
            }
            (Str::DbCatalogSearchTruncated(count), Language::Vietnamese) => {
                format!("Tìm kiếm dừng ở giới hạn danh mục sau khi lập chỉ mục {count} đối tượng.").into()
            }
            (Str::DbCatalogSearchPartial(count), Language::English) => {
                format!("{count} catalog branch(es) could not be searched.").into()
            }
            (Str::DbCatalogSearchPartial(count), Language::Vietnamese) => {
                format!("Không thể tìm trong {count} nhánh danh mục.").into()
            }
            (Str::DbCatalogSearchConnectionUnavailable(name), Language::English) => format!(
                "The catalog result cannot be opened because connection “{name}” is no longer connected or now points elsewhere."
            )
            .into(),
            (Str::DbCatalogSearchConnectionUnavailable(name), Language::Vietnamese) => format!(
                "Không thể mở kết quả danh mục vì kết nối “{name}” không còn được kết nối hoặc hiện trỏ đến nơi khác."
            )
            .into(),
            (Str::DbCatalogKindDatabase, Language::English) => "Database".into(),
            (Str::DbCatalogKindDatabase, Language::Vietnamese) => "Cơ sở dữ liệu".into(),
            (Str::DbCatalogKindSchema, Language::English) => "Schema".into(),
            (Str::DbCatalogKindSchema, Language::Vietnamese) => "Lược đồ".into(),
            (Str::DbCatalogKindTable, Language::English) => "Table".into(),
            (Str::DbCatalogKindTable, Language::Vietnamese) => "Bảng".into(),
            (Str::DbCatalogKindView, Language::English) => "View".into(),
            (Str::DbCatalogKindView, Language::Vietnamese) => "Khung nhìn".into(),
            (Str::DbCatalogKindColumn, Language::English) => "Column".into(),
            (Str::DbCatalogKindColumn, Language::Vietnamese) => "Cột".into(),
            (Str::DbCatalogKindIndex, Language::English) => "Index".into(),
            (Str::DbCatalogKindIndex, Language::Vietnamese) => "Chỉ mục".into(),
            (Str::DbCatalogKindConstraint, Language::English) => "Constraint".into(),
            (Str::DbCatalogKindConstraint, Language::Vietnamese) => "Ràng buộc".into(),
            (Str::DbCatalogKindNamespace, Language::English) => "Namespace".into(),
            (Str::DbCatalogKindNamespace, Language::Vietnamese) => "Không gian tên".into(),
            (Str::DbCatalogKindKey, Language::English) => "Key".into(),
            (Str::DbCatalogKindKey, Language::Vietnamese) => "Khóa".into(),
            (Str::DbCatalogKindObject, Language::English) => "Object".into(),
            (Str::DbCatalogKindObject, Language::Vietnamese) => "Đối tượng".into(),
            (Str::DbFieldUri, Language::English) => "Connection URI".into(),
            (Str::DbFieldUri, Language::Vietnamese) => "URI kết nối".into(),
            // An example URI, not prose: the same in every language, and
            // written per-language anyway so a new language still has to add
            // its row rather than fall into a catch-all.
            (Str::DbFieldUriPlaceholder, Language::English) => {
                "postgresql://user:password@host:5432/database".into()
            }
            (Str::DbFieldUriPlaceholder, Language::Vietnamese) => {
                "postgresql://user:password@host:5432/database".into()
            }
            (Str::DbFillFromUri, Language::English) => "Fill from URI".into(),
            (Str::DbFillFromUri, Language::Vietnamese) => "Điền từ URI".into(),
            (Str::DbUriFilled, Language::English) => {
                "Filled in from the URI. Check the fields before saving.".into()
            }
            (Str::DbUriFilled, Language::Vietnamese) => {
                "Đã điền từ URI. Hãy kiểm tra các trường trước khi lưu.".into()
            }
            (Str::DbUriIgnored(parts), Language::English) => {
                format!("Read but not applied: {parts}").into()
            }
            (Str::DbUriIgnored(parts), Language::Vietnamese) => {
                format!("Đã đọc nhưng không áp dụng: {parts}").into()
            }
            (Str::DbUriTlsNotApplied, Language::English) => {
                "This URI asks for TLS, but dodo's Redis client connects without it.".into()
            }
            (Str::DbUriTlsNotApplied, Language::Vietnamese) => {
                "URI này yêu cầu TLS, nhưng ứng dụng khách Redis của dodo kết nối mà không dùng \
                 TLS."
                    .into()
            }
            (Str::DbUriEmpty, Language::English) => "Paste a connection URI first.".into(),
            (Str::DbUriEmpty, Language::Vietnamese) => "Hãy dán một URI kết nối trước.".into(),
            (Str::DbUriNoScheme, Language::English) => {
                "This has no scheme, so there is nothing to say which database it is. Start it \
                 with postgresql://, mysql://, sqlite:// or redis://."
                    .into()
            }
            (Str::DbUriNoScheme, Language::Vietnamese) => {
                "Chuỗi này không có lược đồ nên không biết đây là cơ sở dữ liệu nào. Hãy bắt đầu \
                 bằng postgresql://, mysql://, sqlite:// hoặc redis://."
                    .into()
            }
            (Str::DbUriUnknownScheme(scheme), Language::English) => format!(
                "dodo cannot connect to \"{scheme}\". Use postgresql, mysql, sqlite or redis."
            )
            .into(),
            (Str::DbUriUnknownScheme(scheme), Language::Vietnamese) => format!(
                "dodo không kết nối được tới \"{scheme}\". Hãy dùng postgresql, mysql, sqlite \
                 hoặc redis."
            )
            .into(),
            (Str::DbUriInvalidPort(port), Language::English) => {
                format!("\"{port}\" is not a port number.").into()
            }
            (Str::DbUriInvalidPort(port), Language::Vietnamese) => {
                format!("\"{port}\" không phải là số cổng.").into()
            }
            (Str::DbUriMissingFile, Language::English) => {
                "This URI names no database file.".into()
            }
            (Str::DbUriMissingFile, Language::Vietnamese) => {
                "URI này không nêu tệp cơ sở dữ liệu nào.".into()
            }
            (Str::DbUriInvalidEscape, Language::English) => {
                "A percent-escape in this URI is not valid UTF-8.".into()
            }
            (Str::DbUriInvalidEscape, Language::Vietnamese) => {
                "Một chuỗi thoát phần trăm trong URI này không phải UTF-8 hợp lệ.".into()
            }

            (Str::QuickNavigation, Language::English) => "Quick navigation".into(),
            (Str::QuickNavigation, Language::Vietnamese) => "Điều hướng nhanh".into(),
            (Str::QuickNavEnabled, Language::English) => "Paste to navigate".into(),
            (Str::QuickNavEnabled, Language::Vietnamese) => "Dán để điều hướng".into(),
            (Str::QuickNavEnabledDescription, Language::English) => {
                "With no input focused, Cmd+V, Ctrl+V or p reads the clipboard and opens the tool \
                 that can handle it. Press Esc inside an input to leave it first."
                    .into()
            }
            (Str::QuickNavEnabledDescription, Language::Vietnamese) => {
                "Khi không có ô nhập nào đang được chọn, Cmd+V, Ctrl+V hoặc p sẽ đọc bảng nhớ tạm \
                 và mở công cụ xử lý được nội dung đó. Nhấn Esc trong ô nhập để rời khỏi nó trước."
                    .into()
            }
            (Str::QuickNavCurlPattern, Language::English) => "cURL pattern".into(),
            (Str::QuickNavCurlPattern, Language::Vietnamese) => "Mẫu cURL".into(),
            (Str::QuickNavDatabasePattern, Language::English) => "Database URI pattern".into(),
            (Str::QuickNavDatabasePattern, Language::Vietnamese) => {
                "Mẫu URI cơ sở dữ liệu".into()
            }
            (Str::QuickNavJwtPattern, Language::English) => "JWT pattern".into(),
            (Str::QuickNavJwtPattern, Language::Vietnamese) => "Mẫu JWT".into(),
            (Str::QuickNavJsonPattern, Language::English) => "JSON pattern".into(),
            (Str::QuickNavJsonPattern, Language::Vietnamese) => "Mẫu JSON".into(),
            (Str::QuickNavBase64Pattern, Language::English) => "Base64 pattern".into(),
            (Str::QuickNavBase64Pattern, Language::Vietnamese) => "Mẫu Base64".into(),
            (Str::QuickNavGateDescription, Language::English) => {
                "Optional. dodo already has a real parser for this format and uses it; a pattern \
                 here only narrows what is offered to it. Leave it empty to try the parser on \
                 everything."
                    .into()
            }
            (Str::QuickNavGateDescription, Language::Vietnamese) => {
                "Tùy chọn. dodo đã có bộ phân tích thật cho định dạng này và luôn dùng nó; mẫu ở \
                 đây chỉ thu hẹp phần được đưa vào bộ phân tích. Để trống để thử bộ phân tích với \
                 mọi nội dung."
                    .into()
            }
            (Str::QuickNavShapeDescription, Language::English) => {
                "The shape a candidate must have. Leave it empty for the built-in one; either way \
                 the text still has to decode before dodo will jump."
                    .into()
            }
            (Str::QuickNavShapeDescription, Language::Vietnamese) => {
                "Hình dạng mà một ứng viên phải có. Để trống để dùng mẫu dựng sẵn; dù thế nào thì \
                 nội dung vẫn phải giải mã được thì dodo mới chuyển sang công cụ."
                    .into()
            }
            (Str::QuickNavPatternInvalid(detail), Language::English) => {
                format!("This pattern is not valid, so the built-in one is being used: {detail}")
                    .into()
            }
            (Str::QuickNavPatternInvalid(detail), Language::Vietnamese) => format!(
                "Mẫu này không hợp lệ nên dodo đang dùng mẫu dựng sẵn: {detail}"
            )
            .into(),
            (Str::QuickNavPatternTooLong { length, limit }, Language::English) => format!(
                "This pattern is {length} characters long; the limit is {limit}. The built-in one \
                 is being used."
            )
            .into(),
            (Str::QuickNavPatternTooLong { length, limit }, Language::Vietnamese) => format!(
                "Mẫu này dài {length} ký tự, vượt giới hạn {limit}. dodo đang dùng mẫu dựng sẵn."
            )
            .into(),
            (Str::QuickNavStorageProblem, Language::English) => "Saved settings".into(),
            (Str::QuickNavStorageProblem, Language::Vietnamese) => "Cài đặt đã lưu".into(),
            (Str::QuickNavStoreError(detail), Language::English) => {
                format!("quick-nav.json could not be read or written: {detail}").into()
            }
            (Str::QuickNavStoreError(detail), Language::Vietnamese) => {
                format!("Không đọc hoặc ghi được quick-nav.json: {detail}").into()
            }
            (Str::QuickNavStoreMissingVersion, Language::English) => {
                "quick-nav.json carries no version, so it was not written by dodo. It is being \
                 left alone and the defaults are in use."
                    .into()
            }
            (Str::QuickNavStoreMissingVersion, Language::Vietnamese) => {
                "quick-nav.json không có trường version nên không phải do dodo ghi. dodo giữ \
                 nguyên tệp và dùng giá trị mặc định."
                    .into()
            }
            (
                Str::QuickNavStoreUnsupportedVersion { found, understood },
                Language::English,
            ) => format!(
                "quick-nav.json is version {found}; this dodo understands {understood}. The \
                 defaults are in use and the file is being left alone."
            )
            .into(),
            (
                Str::QuickNavStoreUnsupportedVersion { found, understood },
                Language::Vietnamese,
            ) => format!(
                "quick-nav.json là phiên bản {found}; bản dodo này hiểu phiên bản {understood}. \
                 dodo dùng giá trị mặc định và giữ nguyên tệp."
            )
            .into(),
            (Str::QuickNavOpenedConnection(name), Language::English) => {
                format!("Opened the saved connection \"{name}\".").into()
            }
            (Str::QuickNavOpenedConnection(name), Language::Vietnamese) => {
                format!("Đã mở kết nối đã lưu \"{name}\".").into()
            }
            (Str::QuickNavKeptStoredPassword(name), Language::English) => format!(
                "Opened the saved connection \"{name}\". Its stored password was kept; the pasted \
                 one was not used."
            )
            .into(),
            (Str::QuickNavKeptStoredPassword(name), Language::Vietnamese) => format!(
                "Đã mở kết nối đã lưu \"{name}\". dodo giữ mật khẩu đã lưu; mật khẩu vừa dán không \
                 được dùng."
            )
            .into(),
            (Str::QuickNavCreatedConnection(name), Language::English) => {
                format!("Created the connection \"{name}\" from the pasted URI.").into()
            }
            (Str::QuickNavCreatedConnection(name), Language::Vietnamese) => {
                format!("Đã tạo kết nối \"{name}\" từ URI vừa dán.").into()
            }
            (Str::QuickNavConnectionsLoading, Language::English) => {
                "The saved connections are still loading, so nothing was created. Paste the URI \
                 again in a moment."
                    .into()
            }
            (Str::QuickNavConnectionsLoading, Language::Vietnamese) => {
                "Các kết nối đã lưu vẫn đang được tải nên chưa tạo gì cả. Hãy dán lại URI sau giây \
                 lát."
                    .into()
            }

            (Str::SessionStorageProblem, Language::English) => "Saved session".into(),
            (Str::SessionStorageProblem, Language::Vietnamese) => "Phiên đã lưu".into(),
            (Str::SessionStoreError(detail), Language::English) => {
                format!("session.json could not be read or written: {detail}").into()
            }
            (Str::SessionStoreError(detail), Language::Vietnamese) => {
                format!("Không đọc hoặc ghi được session.json: {detail}").into()
            }
            (Str::SessionStoreMissingVersion, Language::English) => {
                "session.json carries no version, so it was not written by dodo. It is being left \
                 alone and nothing is being saved this run."
                    .into()
            }
            (Str::SessionStoreMissingVersion, Language::Vietnamese) => {
                "session.json không có trường version nên không phải do dodo ghi. dodo giữ nguyên \
                 tệp và không lưu gì trong lần chạy này."
                    .into()
            }
            (Str::SessionStoreUnsupportedVersion { found, understood }, Language::English) => {
                format!(
                    "session.json is version {found}; this dodo understands {understood}. It is \
                     being left alone and nothing is being saved this run."
                )
                .into()
            }
            (Str::SessionStoreUnsupportedVersion { found, understood }, Language::Vietnamese) => {
                format!(
                    "session.json là phiên bản {found}; bản dodo này hiểu phiên bản {understood}. \
                     dodo giữ nguyên tệp và không lưu gì trong lần chạy này."
                )
                .into()
            }

            (Str::Features, Language::English) => "Features".into(),
            (Str::Features, Language::Vietnamese) => "Tính năng".into(),
            (Str::FeaturesDescription, Language::English) => {
                "Choose which tools the sidebar lists, and in what order. Drag a row by its \
                 handle, or use the arrows."
                    .into()
            }
            (Str::FeaturesDescription, Language::Vietnamese) => {
                "Chọn những công cụ hiện trong thanh bên và thứ tự của chúng. Kéo một dòng bằng \
                 tay cầm, hoặc dùng các mũi tên."
                    .into()
            }
            (Str::FeatureShowInSidebar, Language::English) => "Show in the sidebar".into(),
            (Str::FeatureShowInSidebar, Language::Vietnamese) => "Hiện trong thanh bên".into(),
            (Str::FeatureDragToReorder, Language::English) => "Drag to reorder".into(),
            (Str::FeatureDragToReorder, Language::Vietnamese) => "Kéo để sắp xếp lại".into(),
            (Str::FeatureMoveUp, Language::English) => "Move up".into(),
            (Str::FeatureMoveUp, Language::Vietnamese) => "Chuyển lên".into(),
            (Str::FeatureMoveDown, Language::English) => "Move down".into(),
            (Str::FeatureMoveDown, Language::Vietnamese) => "Chuyển xuống".into(),
            (Str::FeatureLastVisibleTool, Language::English) => {
                "At least one tool has to stay in the sidebar.".into()
            }
            (Str::FeatureLastVisibleTool, Language::Vietnamese) => {
                "Thanh bên phải giữ lại ít nhất một công cụ.".into()
            }

            (Str::TrayOpenDodo, Language::English) => "Open Dodo".into(),
            (Str::TrayOpenDodo, Language::Vietnamese) => "Mở Dodo".into(),
            // Deliberately not "Language": in a status menu that word reads as
            // dodo's *interface* language, which is the Settings dialog's and a
            // different setting entirely. See `tray::input_language`.
            (Str::TrayKeyboardInput, Language::English) => "Keyboard Input".into(),
            (Str::TrayKeyboardInput, Language::Vietnamese) => "Bàn phím nhập".into(),
            (Str::TrayQuitDodo, Language::English) => "Quit Dodo".into(),
            (Str::TrayQuitDodo, Language::Vietnamese) => "Thoát Dodo".into(),

            (Str::InputMethod, Language::English) => "Input method".into(),
            (Str::InputMethod, Language::Vietnamese) => "Bộ gõ".into(),
            (Str::InputMethodDescription, Language::English) => {
                "Choose Dodo's Vietnamese input method. Native Input Method works after Dodo \
                 closes; Event Tap asks macOS for Accessibility permission and works while Dodo is open."
                    .into()
            }
            (Str::InputMethodDescription, Language::Vietnamese) => {
                "Chọn bộ gõ tiếng Việt của Dodo. Bộ gõ gốc hoạt động khi Dodo đã đóng; Chặn sự \
                 kiện yêu cầu macOS cấp quyền Trợ năng và chỉ hoạt động khi Dodo đang mở."
                    .into()
            }
            (Str::InputMethodBackend, Language::English) => "Backend".into(),
            (Str::InputMethodBackend, Language::Vietnamese) => "Phụ trợ".into(),
            (Str::InputMethodBackendDescription, Language::English) => {
                "Only one backend transforms keys at a time.".into()
            }
            (Str::InputMethodBackendDescription, Language::Vietnamese) => {
                "Chỉ một phụ trợ chuyển đổi phím tại một thời điểm.".into()
            }
            (Str::InputMethodNative, Language::English) => "Native Input Method".into(),
            (Str::InputMethodNative, Language::Vietnamese) => "Bộ gõ gốc".into(),
            (Str::InputMethodEventTap, Language::English) => "Event Tap".into(),
            (Str::InputMethodEventTap, Language::Vietnamese) => "Chặn sự kiện".into(),
            (Str::InputMethodEventTapStatus, Language::English) => "Event Tap status".into(),
            (Str::InputMethodEventTapStatus, Language::Vietnamese) => {
                "Trạng thái chặn sự kiện".into()
            }
            (Str::InputMethodEventTapInactive, Language::English) => {
                "Select Vietnamese from Dodo's Keyboard Input menu to start Event Tap.".into()
            }
            (Str::InputMethodEventTapInactive, Language::Vietnamese) => {
                "Chọn Tiếng Việt trong menu Bàn phím nhập của Dodo để khởi động chặn sự kiện."
                    .into()
            }
            (Str::InputMethodEventTapWaitingForNative, Language::English) => {
                "Waiting for Native Input Method to apply this selection.".into()
            }
            (Str::InputMethodEventTapWaitingForNative, Language::Vietnamese) => {
                "Đang chờ Bộ gõ gốc áp dụng lựa chọn này.".into()
            }
            (Str::InputMethodEventTapNeedsAccessibility, Language::English) => {
                "macOS needs you to enable Dodo in System Settings → Privacy & Security → Accessibility. Keys are passing through unchanged."
                    .into()
            }
            (Str::InputMethodEventTapNeedsAccessibility, Language::Vietnamese) => {
                "macOS cần bạn bật Dodo trong Cài đặt hệ thống → Quyền riêng tư và bảo mật → Trợ năng. Các phím được chuyển qua không thay đổi."
                    .into()
            }
            (Str::InputMethodEventTapRunning, Language::English) => {
                "Event Tap is active while Dodo is open. It never stores or sends what you type."
                    .into()
            }
            (Str::InputMethodEventTapRunning, Language::Vietnamese) => {
                "Chặn sự kiện hoạt động khi Dodo đang mở. Dodo không bao giờ lưu hoặc gửi nội dung bạn gõ."
                    .into()
            }
            (Str::InputMethodEventTapFailed, Language::English) => {
                "Event Tap could not start. Keys are passing through unchanged.".into()
            }
            (Str::InputMethodEventTapFailed, Language::Vietnamese) => {
                "Không thể khởi động chặn sự kiện. Các phím được chuyển qua không thay đổi."
                    .into()
            }
            (Str::InputMethodWindowsDescription, Language::English) => {
                "Choose Dodo's Vietnamese input method. Native TSF works after Dodo closes and requires installation; Keyboard Hook needs Dodo to remain open."
                    .into()
            }
            (Str::InputMethodWindowsDescription, Language::Vietnamese) => {
                "Chọn bộ gõ tiếng Việt của Dodo. TSF gốc hoạt động khi Dodo đã đóng và cần cài đặt; Keyboard Hook cần Dodo tiếp tục chạy."
                    .into()
            }
            (Str::InputMethodWindowsLanguageDescription, Language::English) => {
                "Select Vietnamese before either Windows backend transforms input.".into()
            }
            (Str::InputMethodWindowsLanguageDescription, Language::Vietnamese) => {
                "Chọn Tiếng Việt trước khi một trong hai phụ trợ Windows chuyển đổi đầu vào.".into()
            }
            (Str::InputMethodNativeTsf, Language::English) => "Native TSF".into(),
            (Str::InputMethodNativeTsf, Language::Vietnamese) => "TSF gốc".into(),
            (Str::InputMethodWindowsTsfStatus, Language::English) => "Native TSF status".into(),
            (Str::InputMethodWindowsTsfStatus, Language::Vietnamese) => "Trạng thái TSF gốc".into(),
            (Str::InputMethodWindowsTsfNotInstalled, Language::English) => {
                "Not installed. Install Native TSF to type when Dodo is closed.".into()
            }
            (Str::InputMethodWindowsTsfNotInstalled, Language::Vietnamese) => {
                "Chưa cài. Hãy cài TSF gốc để gõ khi Dodo đã đóng.".into()
            }
            (Str::InputMethodWindowsTsfInstalled, Language::English) => {
                "Installed for this Windows account. Select Dodo Vietnamese from Windows input methods.".into()
            }
            (Str::InputMethodWindowsTsfInstalled, Language::Vietnamese) => {
                "Đã cài cho tài khoản Windows này. Chọn Dodo Vietnamese trong bộ gõ của Windows.".into()
            }
            (Str::InputMethodWindowsTsfRemoved, Language::English) => {
                "Native TSF was removed for this Windows account.".into()
            }
            (Str::InputMethodWindowsTsfRemoved, Language::Vietnamese) => {
                "TSF gốc đã được gỡ cho tài khoản Windows này.".into()
            }
            (Str::InputMethodWindowsTsfNoDll, Language::English) => {
                "This build carries no Windows TSF DLL to install.".into()
            }
            (Str::InputMethodWindowsTsfNoDll, Language::Vietnamese) => {
                "Bản dựng này không kèm DLL TSF Windows để cài.".into()
            }
            (Str::InputMethodWindowsTsfRegisterFailed(detail), Language::English) => {
                format!("Windows could not register Native TSF: {detail}").into()
            }
            (Str::InputMethodWindowsTsfRegisterFailed(detail), Language::Vietnamese) => {
                format!("Windows không thể đăng ký TSF gốc: {detail}").into()
            }
            (Str::InputMethodWindowsTsfUnregisterFailed(detail), Language::English) => {
                format!("Windows could not remove Native TSF: {detail}").into()
            }
            (Str::InputMethodWindowsTsfUnregisterFailed(detail), Language::Vietnamese) => {
                format!("Windows không thể gỡ TSF gốc: {detail}").into()
            }
            (Str::InputMethodKeyboardHook, Language::English) => "Keyboard Hook".into(),
            (Str::InputMethodKeyboardHook, Language::Vietnamese) => "Keyboard Hook".into(),
            (Str::InputMethodKeyboardHookStatus, Language::English) => "Keyboard Hook status".into(),
            (Str::InputMethodKeyboardHookStatus, Language::Vietnamese) => {
                "Trạng thái Keyboard Hook".into()
            }
            (Str::InputMethodKeyboardHookInactive, Language::English) => {
                "Select Vietnamese from Dodo's Keyboard Input menu to start Keyboard Hook.".into()
            }
            (Str::InputMethodKeyboardHookInactive, Language::Vietnamese) => {
                "Chọn Tiếng Việt trong menu Bàn phím nhập của Dodo để khởi động Keyboard Hook.".into()
            }
            (Str::InputMethodKeyboardHookRunning, Language::English) => {
                "Keyboard Hook is active only while Dodo is open. It never stores or sends what you type.".into()
            }
            (Str::InputMethodKeyboardHookRunning, Language::Vietnamese) => {
                "Keyboard Hook chỉ hoạt động khi Dodo đang mở. Dodo không bao giờ lưu hoặc gửi nội dung bạn gõ.".into()
            }
            (Str::InputMethodKeyboardHookFailed, Language::English) => {
                "Keyboard Hook could not start. Keys are passing through unchanged.".into()
            }
            (Str::InputMethodKeyboardHookFailed, Language::Vietnamese) => {
                "Không thể khởi động Keyboard Hook. Các phím được chuyển qua không thay đổi.".into()
            }
            (Str::InputMethodUninstall, Language::English) => "Uninstall".into(),
            (Str::InputMethodUninstall, Language::Vietnamese) => "Gỡ cài đặt".into(),
            (Str::InputMethodUninstalling, Language::English) => "Uninstalling…".into(),
            (Str::InputMethodUninstalling, Language::Vietnamese) => "Đang gỡ cài đặt…".into(),
            (Str::InputMethodInstall, Language::English) => "Install".into(),
            (Str::InputMethodInstall, Language::Vietnamese) => "Cài đặt".into(),
            (Str::InputMethodReinstall, Language::English) => "Reinstall".into(),
            (Str::InputMethodReinstall, Language::Vietnamese) => "Cài lại".into(),
            (Str::InputMethodInstalling, Language::English) => "Installing…".into(),
            (Str::InputMethodInstalling, Language::Vietnamese) => "Đang cài…".into(),
            (Str::InputMethodInstalled, Language::English) => {
                "Installed, and macOS switched to it. Type Vietnamese anywhere.".into()
            }
            (Str::InputMethodInstalled, Language::Vietnamese) => {
                "Đã cài và macOS đã chuyển sang bộ gõ. Gõ tiếng Việt ở mọi nơi.".into()
            }
            // The `-50` case, which on some machines is every input source's
            // answer including Apple's own — see `docs/macos-input-method.md` §5.
            // So the wording says what to do next rather than what went wrong.
            (Str::InputMethodInstalledNotActive(status), Language::English) => format!(
                "Installed, but macOS would not switch to it (error {status}). Turn it on in \
                 System Settings → Keyboard → Input Sources."
            )
            .into(),
            (Str::InputMethodInstalledNotActive(status), Language::Vietnamese) => format!(
                "Đã cài, nhưng macOS không chuyển sang bộ gõ (lỗi {status}). Hãy bật trong Cài \
                 đặt hệ thống → Bàn phím → Nguồn nhập."
            )
            .into(),
            (Str::InputMethodNoBundle, Language::English) => {
                "This build of Dodo carries no input method to install.".into()
            }
            (Str::InputMethodNoBundle, Language::Vietnamese) => {
                "Bản Dodo này không kèm bộ gõ để cài.".into()
            }
            (Str::InputMethodCopyFailed(detail), Language::English) => {
                format!("The input method could not be copied: {detail}").into()
            }
            (Str::InputMethodCopyFailed(detail), Language::Vietnamese) => {
                format!("Không thể sao chép bộ gõ: {detail}").into()
            }
            (Str::InputMethodInvalidSignature(detail), Language::English) => {
                format!("The input method has an invalid code signature: {detail}").into()
            }
            (Str::InputMethodInvalidSignature(detail), Language::Vietnamese) => {
                format!("Bộ gõ có chữ ký mã không hợp lệ: {detail}").into()
            }
            (Str::InputMethodNeverAppeared(attempts), Language::English) => format!(
                "macOS accepted the input method but never listed it, after {attempts} attempts."
            )
            .into(),
            (Str::InputMethodNeverAppeared(attempts), Language::Vietnamese) => format!(
                "macOS đã nhận bộ gõ nhưng không liệt kê nó, sau {attempts} lần thử."
            )
            .into(),
            (Str::InputMethodStatus, Language::English) => "Status".into(),
            (Str::InputMethodStatus, Language::Vietnamese) => "Trạng thái".into(),
            (Str::InputMethodNotInstalled, Language::English) => "Not installed.".into(),
            (Str::InputMethodNotInstalled, Language::Vietnamese) => "Chưa cài.".into(),
            (Str::InputMethodRunning(version), Language::English) => {
                format!("Running, version {version}.").into()
            }
            (Str::InputMethodRunning(version), Language::Vietnamese) => {
                format!("Đang chạy, phiên bản {version}.").into()
            }
            (Str::InputMethodInstalledIdle, Language::English) => {
                "Installed. macOS starts it when you switch to it.".into()
            }
            (Str::InputMethodInstalledIdle, Language::Vietnamese) => {
                "Đã cài. macOS sẽ khởi động bộ gõ khi bạn chuyển sang nó.".into()
            }
            (Str::InputMethodSettingsPending, Language::English) => {
                "The input method has not picked these settings up yet.".into()
            }
            (Str::InputMethodSettingsPending, Language::Vietnamese) => {
                "Bộ gõ chưa nhận các thiết lập này.".into()
            }
            (Str::InputMethodStorageProblem, Language::English) => "Settings file".into(),
            (Str::InputMethodStorageProblem, Language::Vietnamese) => "Tệp thiết lập".into(),
            (Str::InputMethodStoreError(detail), Language::English) => {
                format!("The input method's settings could not be read or saved: {detail}").into()
            }
            (Str::InputMethodStoreError(detail), Language::Vietnamese) => {
                format!("Không thể đọc hoặc lưu thiết lập bộ gõ: {detail}").into()
            }
            (Str::InputMethodStoreMissingVersion, Language::English) => {
                "The input method's settings file carries no schema version, so it cannot be \
                 read safely."
                    .into()
            }
            (Str::InputMethodStoreMissingVersion, Language::Vietnamese) => {
                "Tệp thiết lập bộ gõ không ghi phiên bản lược đồ nên không thể đọc an toàn.".into()
            }
            (
                Str::InputMethodStoreUnsupportedVersion { found, supported },
                Language::English,
            ) => format!(
                "The input method's settings file uses schema {found}; this build of dodo reads \
                 {supported}. Update dodo rather than risk misreading it."
            )
            .into(),
            (
                Str::InputMethodStoreUnsupportedVersion { found, supported },
                Language::Vietnamese,
            ) => format!(
                "Tệp thiết lập bộ gõ dùng lược đồ {found}; bản dodo này chỉ đọc {supported}. Hãy \
                 cập nhật dodo thay vì đọc sai tệp."
            )
            .into(),
            (Str::InputMethodScheme, Language::English) => "Input scheme".into(),
            (Str::InputMethodScheme, Language::Vietnamese) => "Kiểu gõ".into(),
            (Str::InputMethodSchemeDescription, Language::English) => {
                "Telex marks tones with letters (aa, ow, s, f); VNI marks them with digits \
                 (a6, o7, 1, 2)."
                    .into()
            }
            (Str::InputMethodSchemeDescription, Language::Vietnamese) => {
                "Telex bỏ dấu bằng chữ (aa, ow, s, f); VNI bỏ dấu bằng số (a6, o7, 1, 2).".into()
            }
            (Str::InputMethodTelex, _) => "Telex".into(),
            (Str::InputMethodVni, _) => "VNI".into(),
            (Str::InputMethodTonePlacement, Language::English) => "Tone mark placement".into(),
            (Str::InputMethodTonePlacement, Language::Vietnamese) => "Vị trí dấu thanh".into(),
            (Str::InputMethodTonePlacementDescription, Language::English) => {
                "Modern puts the mark on the main vowel (hoà); traditional puts it on the first \
                 (hòa)."
                    .into()
            }
            (Str::InputMethodTonePlacementDescription, Language::Vietnamese) => {
                "Kiểu mới đặt dấu trên nguyên âm chính (hoà); kiểu cũ đặt trên nguyên âm đầu \
                 (hòa)."
                    .into()
            }
            (Str::InputMethodToneModern, Language::English) => "Modern".into(),
            (Str::InputMethodToneModern, Language::Vietnamese) => "Kiểu mới".into(),
            (Str::InputMethodToneTraditional, Language::English) => "Traditional".into(),
            (Str::InputMethodToneTraditional, Language::Vietnamese) => "Kiểu cũ".into(),
            (Str::InputMethodSpellCheck, Language::English) => "Spell check".into(),
            (Str::InputMethodSpellCheck, Language::Vietnamese) => "Kiểm tra chính tả".into(),
            (Str::InputMethodSpellCheckDescription, Language::English) => {
                "Hand back the keys as typed when the result is not a Vietnamese syllable, so \
                 English words survive."
                    .into()
            }
            (Str::InputMethodSpellCheckDescription, Language::Vietnamese) => {
                "Trả lại đúng các ký tự đã gõ khi kết quả không phải âm tiết tiếng Việt, để từ \
                 tiếng Anh không bị đổi."
                    .into()
            }
            (Str::InputMethodBracketShortcuts, Language::English) => "Bracket shortcuts".into(),
            (Str::InputMethodBracketShortcuts, Language::Vietnamese) => "Phím ngoặc".into(),
            (Str::InputMethodBracketShortcutsDescription, Language::English) => {
                "In Telex, [ and ] type ơ and ư — the only way to type uơ (thuở, huơ).".into()
            }
            (Str::InputMethodBracketShortcutsDescription, Language::Vietnamese) => {
                "Trong Telex, [ và ] gõ ơ và ư — cách duy nhất để gõ uơ (thuở, huơ).".into()
            }
            (Str::CleanerScanDescription, Language::English) => {
                "Scan this category for files that can be safely removed.".into()
            }
            (Str::CleanerScanDescription, Language::Vietnamese) => {
                "Quét mục này để tìm các tệp có thể xoá an toàn.".into()
            }
            (Str::CleanerEntriesScannedCount(count), Language::English) => {
                format!("{count} entries scanned").into()
            }
            (Str::CleanerEntriesScannedCount(count), Language::Vietnamese) => {
                format!("Đã quét {count} mục").into()
            }
            (Str::CleanerBytesDiscovered(size), Language::English) => {
                format!("{size} discovered").into()
            }
            (Str::CleanerBytesDiscovered(size), Language::Vietnamese) => {
                format!("Đã tìm thấy {size}").into()
            }
            (Str::CleanerReclaimableAmount(size), Language::English) => {
                format!("{size} reclaimable").into()
            }
            (Str::CleanerReclaimableAmount(size), Language::Vietnamese) => {
                format!("Có thể giải phóng {size}").into()
            }
            (Str::CleanerItemsFound(count), Language::English) => format!("{count} items").into(),
            (Str::CleanerItemsFound(count), Language::Vietnamese) => format!("{count} mục").into(),
            (Str::CleanerSafeItemsCount(count), Language::English) => {
                format!("{count} safe").into()
            }
            (Str::CleanerSafeItemsCount(count), Language::Vietnamese) => {
                format!("{count} an toàn").into()
            }
            (Str::CleanerWarningCount(count), Language::English) => {
                let word = if count == 1 { "warning" } else { "warnings" };
                format!("{count} {word}").into()
            }
            (Str::CleanerWarningCount(count), Language::Vietnamese) => {
                format!("{count} cảnh báo").into()
            }
            (Str::CleanerSelectedSummary { count, size }, Language::English) => {
                format!("{count} selected · {size}").into()
            }
            (Str::CleanerSelectedSummary { count, size }, Language::Vietnamese) => {
                format!("Đã chọn {count} · {size}").into()
            }
            (Str::CleanerCleanCount { count, size }, Language::English) => {
                format!("Clean {count} items · {size}").into()
            }
            (Str::CleanerCleanCount { count, size }, Language::Vietnamese) => {
                format!("Xoá {count} mục · {size}").into()
            }
            (Str::CleanerScanWarningsSummary(count), Language::English) => {
                let word = if count == 1 { "location" } else { "locations" };
                format!("{count} {word} could not be scanned").into()
            }
            (Str::CleanerScanWarningsSummary(count), Language::Vietnamese) => {
                format!("{count} vị trí không thể quét").into()
            }
            (Str::CleanerScanWarningsShowDetails, Language::English) => "Show details".into(),
            (Str::CleanerScanWarningsShowDetails, Language::Vietnamese) => {
                "Xem chi tiết".into()
            }
            (Str::CleanerScanWarningsHideDetails, Language::English) => "Hide details".into(),
            (Str::CleanerScanWarningsHideDetails, Language::Vietnamese) => "Ẩn chi tiết".into(),
            (Str::CleanerRescan, Language::English) => "Rescan".into(),
            (Str::CleanerRescan, Language::Vietnamese) => "Quét lại".into(),
            (Str::CleanerSelectAll, Language::English) => "Select all".into(),
            (Str::CleanerSelectAll, Language::Vietnamese) => "Chọn tất cả".into(),
            (Str::CleanerDeselectAll, Language::English) => "Deselect all".into(),
            (Str::CleanerDeselectAll, Language::Vietnamese) => "Bỏ chọn tất cả".into(),
            (Str::CleanerPermissionNotNow, Language::English) => "Not now".into(),
            (Str::CleanerPermissionNotNow, Language::Vietnamese) => {
                "Không phải lúc này".into()
            }
            (Str::CleanerStatusCompletedWithWarnings, Language::English) => {
                "Completed with warnings".into()
            }
            (Str::CleanerStatusCompletedWithWarnings, Language::Vietnamese) => {
                "Hoàn tất có cảnh báo".into()
            }
            (Str::CleanerStatusCancelled, Language::English) => "Cancelled".into(),
            (Str::CleanerStatusCancelled, Language::Vietnamese) => "Đã hủy".into(),

            (Str::StartWithOs, Language::English) => "Start with OS".into(),
            (Str::StartWithOs, Language::Vietnamese) => "Khởi động cùng hệ điều hành".into(),
            (Str::StartWithOsDescription, Language::English) => {
                "Start Dodo in the tray when you sign in. macOS requires macOS 13+ and a bundled Dodo.app; Windows adds a per-user Startup Apps entry.".into()
            }
            (Str::StartWithOsDescription, Language::Vietnamese) => {
                "Khởi động Dodo trong khay khi bạn đăng nhập. macOS cần macOS 13 trở lên và Dodo.app đã đóng gói; Windows thêm mục Khởi động cho người dùng hiện tại.".into()
            }
        }
    }
}

/// Translates `str` into the active language.
pub fn t(str: Str, cx: &App) -> SharedString {
    match str.text(Language::current(cx)) {
        Cow::Borrowed(text) => SharedString::new_static(text),
        Cow::Owned(text) => SharedString::from(text),
    }
}

/// What these tests protect
/// ------------------------
///
/// The `match` in [`Str::text`] already makes a *missing* language a compile
/// error. Three things it cannot catch, and that these tests do:
///
/// 1. A language arm that is present but empty, or whitespace only.
/// 2. A parameterized arm that forgot its `{placeholder}`, so the runtime value
///    (a line number, a parser's message) silently never reaches the screen.
/// 3. A language arm that was filled in by pasting the English text. Asserting
///    "every language differs" would be false — `Hex`, `Header` and `Payload`
///    are the same word in both languages by design — so every variant declares
///    which it is via [`Expect`], and the test holds it to that declaration in
///    *both* directions.
///
/// Adding a `Str` variant is a compile error in `position` below until it is
/// given a slot, and the slot then has to line up with a real entry in
/// `samples`. (The one thing that slips through is deliberately reusing another
/// variant's index; nothing here can detect that.)
#[cfg(test)]
mod tests {
    use super::{JwtPart, Language, Str};

    /// Stands in for a third-party parser's own message. Deliberately unlike
    /// any word in the catalogue so `contains` cannot match by accident.
    const DETAIL: &str = "<<detail-sentinel>>";
    /// Ditto for numeric values: no catalogue string contains this digit run.
    const NUMBER: usize = 4242;
    const NUMBER_TEXT: &str = "4242";

    /// Whether a variant is expected to read differently in each language.
    #[derive(Clone, Copy)]
    enum Expect {
        /// Prose. Every language must produce its own wording.
        Translated,
        /// A term of art that is the same word in every language we ship.
        /// Asserted as equality, so "translating" one later fails here and
        /// forces the declaration to be updated rather than quietly diverging.
        SameEverywhere,
    }

    struct Sample {
        str: Str,
        /// Runtime values the rendered text must surface, in every language.
        parts: &'static [&'static str],
        expect: Expect,
    }

    fn plain(str: Str) -> Sample {
        Sample {
            str,
            parts: &[],
            expect: Expect::Translated,
        }
    }

    fn term(str: Str) -> Sample {
        Sample {
            str,
            parts: &[],
            expect: Expect::SameEverywhere,
        }
    }

    fn with(str: Str, parts: &'static [&'static str]) -> Sample {
        Sample {
            str,
            parts,
            expect: Expect::Translated,
        }
    }

    /// One entry per `Str` variant, in `position` order.
    fn samples() -> Vec<Sample> {
        vec![
            plain(Str::Settings),
            plain(Str::General),
            plain(Str::Appearance),
            plain(Str::Language),
            plain(Str::LanguageDescription),
            plain(Str::Theme),
            plain(Str::ThemeDescription),
            plain(Str::FontSize),
            plain(Str::FontSizeDescription),
            plain(Str::BorderRadius),
            plain(Str::BorderRadiusDescription),
            plain(Str::Large),
            plain(Str::Medium),
            plain(Str::Small),
            plain(Str::SearchSettingsPlaceholder),
            plain(Str::NoSettingsMatch),
            plain(Str::Tools),
            plain(Str::JsonFormatterTitle),
            plain(Str::EncoderDecoderTitle),
            plain(Str::JsonPlaceholder),
            plain(Str::FormatButton),
            plain(Str::IndentLabel),
            with(Str::IndentSpaces(NUMBER), &[NUMBER_TEXT]),
            with(
                Str::InvalidJson {
                    line: NUMBER,
                    column: 77,
                    detail: DETAIL.into(),
                },
                &[NUMBER_TEXT, "77", DETAIL],
            ),
            plain(Str::FormatLabel),
            plain(Str::EncodeButton),
            plain(Str::DecodeButton),
            plain(Str::DecodeJwtButton),
            plain(Str::InputLabel),
            plain(Str::OutputLabel),
            term(Str::JwtHeaderLabel),
            term(Str::JwtPayloadLabel),
            plain(Str::JwtSignatureLabel),
            plain(Str::EncoderInputPlaceholder),
            plain(Str::EncoderOutputPlaceholder),
            plain(Str::FormatBase64),
            plain(Str::FormatBase64UrlSafe),
            plain(Str::FormatUrl),
            term(Str::FormatHex),
            plain(Str::FormatJwt),
            plain(Str::JwtEncodeUnsupported),
            with(Str::InvalidHexOddLength(NUMBER), &[NUMBER_TEXT]),
            with(
                Str::InvalidHexDigit {
                    digit: 'Z',
                    position: NUMBER,
                },
                &["Z", NUMBER_TEXT],
            ),
            with(Str::InvalidBase64(DETAIL.into()), &[DETAIL]),
            with(Str::InvalidPercentAt(NUMBER), &[NUMBER_TEXT]),
            with(Str::InvalidPercentEncoding(DETAIL.into()), &[DETAIL]),
            with(Str::NotUtf8(DETAIL.into()), &[DETAIL]),
            plain(Str::JwtEmpty),
            with(Str::JwtPartCount(NUMBER), &[NUMBER_TEXT]),
            // The part name is checked separately: it is language-dependent, so
            // it cannot be a fixed fragment here.
            with(
                Str::JwtPartNotBase64 {
                    part: JwtPart::Header,
                    detail: DETAIL.into(),
                },
                &[DETAIL],
            ),
            with(
                Str::JwtPartNotJson {
                    part: JwtPart::Payload,
                    detail: DETAIL.into(),
                },
                &[DETAIL],
            ),
            with(
                Str::JwtPartNotRenderable {
                    part: JwtPart::Header,
                    detail: DETAIL.into(),
                },
                &[DETAIL],
            ),
            // API Explorer. Appended rather than slotted in beside the strings
            // they read next to, so that adding a tool does not renumber every
            // existing entry.
            plain(Str::ApiExplorerTitle),
            plain(Str::Collections),
            plain(Str::NoCollections),
            plain(Str::NoCollectionsHint),
            plain(Str::UrlPlaceholder),
            plain(Str::Send),
            plain(Str::NewRequest),
            plain(Str::CloseRequest),
            plain(Str::NameRequest),
            plain(Str::NameRequestPlaceholder),
            plain(Str::SaveName),
            plain(Str::GenerateCode),
            plain(Str::RequestTabParams),
            plain(Str::RequestTabHeaders),
            plain(Str::RequestTabBody),
            plain(Str::RequestTabAuth),
            plain(Str::RequestTabScripts),
            plain(Str::ColumnKey),
            plain(Str::ColumnValue),
            plain(Str::Add),
            plain(Str::AddParameter),
            plain(Str::AddHeader),
            plain(Str::DeleteRow),
            plain(Str::NoActiveParams),
            with(Str::ActiveParams(NUMBER), &[NUMBER_TEXT]),
            plain(Str::NoActiveHeaders),
            with(Str::ActiveHeaders(NUMBER), &[NUMBER_TEXT]),
            plain(Str::ParamKeyPlaceholder),
            plain(Str::ParamValuePlaceholder),
            plain(Str::HeaderKeyPlaceholder),
            plain(Str::HeaderValuePlaceholder),
            plain(Str::ColumnDescription),
            plain(Str::DescriptionPlaceholder),
            plain(Str::DuplicateRow),
            plain(Str::MoveRowUp),
            plain(Str::MoveRowDown),
            plain(Str::AddField),
            plain(Str::NoActiveFields),
            with(Str::ActiveFields(NUMBER), &[NUMBER_TEXT]),
            plain(Str::FieldKeyPlaceholder),
            plain(Str::FieldValuePlaceholder),
            plain(Str::BodyTypeNone),
            term(Str::BodyTypeJson),
            plain(Str::BodyTypeText),
            term(Str::BodyTypeXml),
            term(Str::BodyTypeHtml),
            plain(Str::BodyTypeFormData),
            // The wire spelling of the media type, in both languages.
            term(Str::BodyTypeUrlEncoded),
            plain(Str::BodyTypeBinary),
            plain(Str::BodyPlaceholder),
            plain(Str::NoBodyTitle),
            plain(Str::NoBodyHint),
            plain(Str::BinaryBodyHint),
            with(Str::MethodSendsNoBody("GET".into()), &["GET"]),
            plain(Str::AuthTypeLabel),
            plain(Str::AuthTypeNone),
            term(Str::AuthTypeBearer),
            term(Str::AuthTypeBasic),
            term(Str::AuthTypeApiKey),
            term(Str::AuthTypeOAuth2),
            plain(Str::OAuth2Later),
            plain(Str::NoAuthTitle),
            plain(Str::NoAuthHint),
            term(Str::AuthTokenLabel),
            plain(Str::AuthTokenPlaceholder),
            plain(Str::AuthUsernameLabel),
            plain(Str::AuthUsernamePlaceholder),
            plain(Str::AuthPasswordLabel),
            plain(Str::AuthPasswordPlaceholder),
            plain(Str::ApiKeyNameLabel),
            plain(Str::ApiKeyNamePlaceholder),
            plain(Str::ApiKeyValueLabel),
            plain(Str::ApiKeyValuePlaceholder),
            plain(Str::ApiKeySendAs),
            term(Str::ApiKeyInHeader),
            plain(Str::ApiKeyInQuery),
            plain(Str::ScriptsSandboxNotice),
            plain(Str::PreRequestScriptLabel),
            plain(Str::PreRequestScriptPlaceholder),
            plain(Str::PostResponseScriptLabel),
            plain(Str::PostResponseScriptPlaceholder),
            plain(Str::ResponseTabBody),
            plain(Str::ResponseTabHeaders),
            plain(Str::ResponseTabCookies),
            plain(Str::ResponseTabTests),
            plain(Str::ResponseTabConsole),
            plain(Str::NoResponseYet),
            plain(Str::NoResponseHint),
            plain(Str::Sending),
            plain(Str::RequestFailed),
            plain(Str::CollapseResponse),
            plain(Str::ExpandResponse),
            plain(Str::BodyPretty),
            plain(Str::BodyRaw),
            plain(Str::Copy),
            plain(Str::LoadMoreLines),
            plain(Str::BodyTruncated),
            with(
                Str::LineRange {
                    shown: NUMBER,
                    total: 77,
                },
                &[NUMBER_TEXT, "77"],
            ),
            plain(Str::StatusClassInfo),
            plain(Str::StatusClassSuccess),
            plain(Str::StatusClassRedirect),
            plain(Str::StatusClassClientError),
            plain(Str::StatusClassServerError),
            plain(Str::StatusClassUnknown),
            with(Str::HttpInvalidUrl(DETAIL.into()), &[DETAIL]),
            with(Str::HttpUnsupportedScheme(DETAIL.into()), &[DETAIL]),
            with(Str::HttpInvalidHeader(DETAIL.into()), &[DETAIL]),
            with(Str::HttpTimeout(NUMBER as u64), &[NUMBER_TEXT]),
            with(Str::HttpDnsFailure(DETAIL.into()), &[DETAIL]),
            with(Str::HttpConnectFailure(DETAIL.into()), &[DETAIL]),
            with(Str::HttpTlsFailure(DETAIL.into()), &[DETAIL]),
            with(Str::HttpBodyNotText(DETAIL.into()), &[DETAIL]),
            with(Str::HttpUnexpected(DETAIL.into()), &[DETAIL]),
            // Phase 3.
            plain(Str::ImportCollection),
            plain(Str::NewCollection),
            plain(Str::NewFolder),
            plain(Str::SearchCollectionsPlaceholder),
            plain(Str::Rename),
            plain(Str::Delete),
            plain(Str::Duplicate),
            plain(Str::Open),
            plain(Str::MoreActions),
            plain(Str::NamePlaceholder),
            plain(Str::DefaultCollectionName),
            plain(Str::DefaultFolderName),
            plain(Str::SaveToCollectionNote),
            with(Str::CollectionStoreError(DETAIL.into()), &[DETAIL]),
            with(Str::CollectionImportError(DETAIL.into()), &[DETAIL]),
            plain(Str::History),
            plain(Str::NoHistory),
            plain(Str::NoHistoryHint),
            plain(Str::HistoryReopen),
            plain(Str::HistoryResend),
            plain(Str::HistoryClearAll),
            plain(Str::HistoryJustNow),
            with(Str::HistoryMinutesAgo(NUMBER as u64), &[NUMBER_TEXT]),
            with(Str::HistoryHoursAgo(NUMBER as u64), &[NUMBER_TEXT]),
            with(Str::HistoryDaysAgo(NUMBER as u64), &[NUMBER_TEXT]),
            plain(Str::BodyPreview),
            plain(Str::BodyTree),
            plain(Str::SaveToFile),
            with(Str::JsonTreeTruncated(NUMBER), &[NUMBER_TEXT]),
            plain(Str::HtmlPreviewNote),
            plain(Str::NoCookies),
            plain(Str::NoCookiesHint),
            plain(Str::ToggleAllRows),
            plain(Str::EditModeTable),
            plain(Str::EditModeBulk),
            plain(Str::BulkEditPlaceholder),
            plain(Str::InsertTemplate),
            plain(Str::TemplateSetHeader),
            plain(Str::TemplateSetBearerToken),
            plain(Str::TemplateSetTimestamp),
            plain(Str::TemplateAssertStatus),
            plain(Str::TemplateLogResponse),
            plain(Str::TemplateExtractField),
            // Docker module.
            term(Str::Docker),
            term(Str::Containers),
            term(Str::Images),
            term(Str::Volumes),
            term(Str::Networks),
            plain(Str::DockerSearchPlaceholder),
            plain(Str::DockerRefresh),
            plain(Str::DockerFilter),
            plain(Str::DockerCreate),
            plain(Str::DockerColumnName),
            term(Str::DockerColumnImage),
            plain(Str::DockerColumnStatus),
            term(Str::DockerColumnCpu),
            plain(Str::DockerColumnPorts),
            plain(Str::DockerColumnLastStarted),
            plain(Str::DockerColumnActions),
            plain(Str::DockerStatusRunning),
            plain(Str::DockerStatusExited),
            plain(Str::DockerStatusCreated),
            plain(Str::DockerStatusRestarting),
            plain(Str::DockerStatusPaused),
            plain(Str::DockerStatusDead),
            plain(Str::DockerStatusRemoving),
            plain(Str::DockerStatusStopping),
            plain(Str::DockerStatusUnknown),
            plain(Str::DockerStart),
            plain(Str::DockerStop),
            plain(Str::DockerRestart),
            plain(Str::DockerDeleteTitle),
            with(Str::DockerDeleteMessage(DETAIL.into()), &[DETAIL]),
            plain(Str::DockerCancel),
            plain(Str::NoContainers),
            plain(Str::NoContainersHint),
            plain(Str::DockerRetry),
            with(Str::DockerConnectionError(DETAIL.into()), &[DETAIL]),
            with(Str::DockerOperationError(DETAIL.into()), &[DETAIL]),
            plain(Str::DockerSelectAll),
            plain(Str::DockerSelectRow),
            plain(Str::DockerRelNever),
            plain(Str::DockerRelJustNow),
            with(Str::DockerRelSecondsAgo(NUMBER as u64), &[NUMBER_TEXT]),
            with(Str::DockerRelMinutesAgo(NUMBER as u64), &[NUMBER_TEXT]),
            with(Str::DockerRelHoursAgo(NUMBER as u64), &[NUMBER_TEXT]),
            with(Str::DockerRelDaysAgo(NUMBER as u64), &[NUMBER_TEXT]),
            with(Str::DockerRelWeeksAgo(NUMBER as u64), &[NUMBER_TEXT]),
            with(Str::DockerRelMonthsAgo(NUMBER as u64), &[NUMBER_TEXT]),
            with(Str::DockerRelYearsAgo(NUMBER as u64), &[NUMBER_TEXT]),
            plain(Str::DockerUnreachableTitle),
            // Round 2 — grouping, filters, bulk actions.
            plain(Str::DockerUngrouped),
            with(Str::DockerGroupContainers(NUMBER), &[NUMBER_TEXT]),
            with(Str::DockerGroupRunning(NUMBER), &[NUMBER_TEXT]),
            with(Str::DockerFilterWithCount(NUMBER), &[NUMBER_TEXT]),
            plain(Str::DockerFilterTitle),
            plain(Str::DockerFilterProject),
            plain(Str::DockerFilterPublishedPorts),
            plain(Str::DockerFilterFavorites),
            plain(Str::DockerFilterClear),
            with(Str::DockerBulkSelected(NUMBER), &[NUMBER_TEXT]),
            plain(Str::DockerBulkStart),
            plain(Str::DockerBulkStop),
            plain(Str::DockerBulkDelete),
            plain(Str::DockerBulkClear),
            plain(Str::DockerBulkDeleteTitle),
            with(Str::DockerBulkDeleteMessage(NUMBER), &[NUMBER_TEXT]),
            with(Str::DockerBulkFailures(NUMBER), &[NUMBER_TEXT]),
            // Round 3 — Images, Volumes and Networks pages.
            plain(Str::DockerColumnRepository),
            plain(Str::DockerColumnTag),
            plain(Str::DockerColumnImageId),
            plain(Str::DockerColumnSize),
            plain(Str::DockerColumnCreated),
            plain(Str::DockerColumnContainersUsing),
            plain(Str::DockerColumnDriver),
            plain(Str::DockerColumnMountPoint),
            plain(Str::DockerColumnScope),
            plain(Str::DockerSearchImages),
            plain(Str::DockerSearchVolumes),
            plain(Str::DockerSearchNetworks),
            plain(Str::NoImages),
            plain(Str::NoImagesHint),
            plain(Str::NoVolumes),
            plain(Str::NoVolumesHint),
            plain(Str::NoNetworks),
            plain(Str::NoNetworksHint),
            term(Str::DockerNotAvailable),
            term(Str::DockerNone),
            plain(Str::DockerInspect),
            plain(Str::DockerNetworkPredefined),
            // Round 4 — context-menu placeholders.
            plain(Str::DockerViewLogs),
            plain(Str::DockerOpenTerminal),
            plain(Str::DockerComingSoonLabel),
            // Round 5 — the Inspect panel, the Logs viewer and their field labels.
            plain(Str::DockerDetails),
            plain(Str::DockerRawJson),
            plain(Str::DockerDetailErrorTitle),
            plain(Str::DockerNoLogs),
            plain(Str::DockerNoLogsHint),
            with(Str::DockerLogsTail(NUMBER), &[NUMBER_TEXT]),
            plain(Str::DockerYes),
            plain(Str::DockerNo),
            term(Str::DockerFieldId),
            plain(Str::DockerFieldCommand),
            plain(Str::DockerFieldStarted),
            plain(Str::DockerFieldExitCode),
            plain(Str::DockerFieldRestartPolicy),
            plain(Str::DockerFieldIpAddress),
            plain(Str::DockerFieldMounts),
            plain(Str::DockerFieldTags),
            term(Str::DockerFieldDigest),
            plain(Str::DockerFieldArchitecture),
            plain(Str::DockerFieldOs),
            plain(Str::DockerFieldLayers),
            plain(Str::DockerFieldLabels),
            plain(Str::DockerFieldOptions),
            plain(Str::DockerFieldInternal),
            plain(Str::DockerFieldAttachable),
            plain(Str::DockerFieldSubnet),
            term(Str::DockerFieldGateway),
            plain(Str::DockerPull),
            plain(Str::DockerBuild),
            plain(Str::DockerStats),
            plain(Str::DockerOpenDetails),
            plain(Str::UntitledRequest),
            plain(Str::ColumnType),
            plain(Str::FieldKindText),
            plain(Str::FieldKindFile),
            plain(Str::ChooseFile),
            plain(Str::ReplaceFile),
            plain(Str::ClearFile),
            plain(Str::NoFileSelected),
            with(Str::IncompleteFileFields(NUMBER), &[NUMBER_TEXT]),
            with(
                Str::HttpFileUnreadable {
                    path: "/tmp/a.png".into(),
                    detail: DETAIL.into(),
                },
                &["/tmp/a.png", DETAIL],
            ),
            with(
                Str::HttpFileTooLarge {
                    path: "/tmp/a.png".into(),
                    limit_mb: NUMBER as u64,
                },
                &["/tmp/a.png", NUMBER_TEXT],
            ),
            plain(Str::NoEnvironment),
            plain(Str::SelectEnvironment),
            plain(Str::ManageEnvironments),
            plain(Str::Environments),
            plain(Str::NewEnvironment),
            plain(Str::DefaultEnvironmentName),
            plain(Str::EnvironmentCopySuffix),
            plain(Str::DuplicateEnvironment),
            plain(Str::DeleteEnvironment),
            plain(Str::ImportEnvironment),
            plain(Str::CollectionVariables),
            plain(Str::EnvironmentVariables),
            plain(Str::CollectionVariablesNote),
            plain(Str::NoEnvironmentsYet),
            plain(Str::NoEnvironmentsYetHint),
            plain(Str::ColumnSecret),
            plain(Str::AddVariable),
            plain(Str::NoActiveVariables),
            with(Str::ActiveVariables(NUMBER), &[NUMBER_TEXT]),
            // The placeholder is an example variable name, not prose: it reads
            // the same in every language on purpose.
            term(Str::VariableKeyPlaceholder),
            plain(Str::VariableValuePlaceholder),
            plain(Str::MarkSecret),
            plain(Str::RevealSecret),
            plain(Str::HideSecret),
            plain(Str::SecretStorageWarning),
            plain(Str::ResolvedUrlLabel),
            with(Str::UnresolvedVariablePreview(DETAIL.into()), &[DETAIL]),
            with(
                Str::ResolvesFrom {
                    name: DETAIL.into(),
                    scope: "<<scope-sentinel>>".into(),
                },
                &[DETAIL, "<<scope-sentinel>>"],
            ),
            with(Str::HttpUnresolvedVariable(DETAIL.into()), &[DETAIL]),
            with(Str::HttpRecursiveVariable(DETAIL.into()), &[DETAIL]),
            with(Str::VariableStoreError(DETAIL.into()), &[DETAIL]),
            plain(Str::VariableStoreMissingVersion),
            with(
                Str::VariableStoreUnsupportedVersion {
                    found: NUMBER as u64,
                    supported: 7,
                },
                &[NUMBER_TEXT, "7"],
            ),
            with(Str::EnvironmentImportError(DETAIL.into()), &[DETAIL]),
            plain(Str::ScriptVariables),
            with(Str::ScriptThrew(DETAIL.into()), &[DETAIL]),
            with(Str::ScriptDeadline(NUMBER as u64), &[NUMBER_TEXT]),
            plain(Str::ScriptOutOfMemory),
            with(Str::ScriptUnsupported(DETAIL.into()), &[DETAIL]),
            plain(Str::ScriptNoEngine),
            plain(Str::ScriptSkippedByPolicy),
            plain(Str::ScriptSkippedByConsent),
            with(
                Str::ScriptFinished {
                    millis: NUMBER as u64,
                },
                &[NUMBER_TEXT],
            ),
            with(Str::ScriptWroteVariables(NUMBER), &[NUMBER_TEXT]),
            with(Str::ScriptUnknownMethod(DETAIL.into()), &[DETAIL]),
            plain(Str::ConsoleLevelDebug),
            plain(Str::ConsoleLevelLog),
            plain(Str::ConsoleLevelWarn),
            plain(Str::ConsoleLevelError),
            with(
                Str::ConsoleRunSeparator {
                    run: NUMBER,
                    summary: DETAIL.into(),
                },
                &[NUMBER_TEXT, DETAIL],
            ),
            with(Str::ConsoleRunTruncated(NUMBER), &[NUMBER_TEXT]),
            plain(Str::ConsoleEmpty),
            plain(Str::ConsoleEmptyHint),
            plain(Str::ConsoleClear),
            with(Str::ConsoleDropped(NUMBER), &[NUMBER_TEXT]),
            plain(Str::RunScripts),
            plain(Str::RunScriptsDescription),
            plain(Str::RunScriptsNever),
            plain(Str::RunScriptsAskImported),
            plain(Str::RunScriptsAlways),
            plain(Str::ScriptConsentTitle),
            plain(Str::ScriptConsentExplain),
            with(Str::ScriptConsentRequest(DETAIL.into()), &[DETAIL]),
            plain(Str::ScriptConsentRun),
            plain(Str::ScriptConsentSkip),
            with(Str::ConsentStoreError(DETAIL.into()), &[DETAIL]),
            plain(Str::ConsentStoreMissingVersion),
            with(
                Str::ConsentStoreUnsupportedVersion {
                    found: NUMBER as u64,
                    supported: 7,
                },
                &[NUMBER_TEXT, "7"],
            ),
            plain(Str::ScriptConsentExplainChanged),
            with(Str::ScriptSyntaxError(DETAIL.into()), &[DETAIL]),
            with(
                Str::ScriptSyntaxErrorAt {
                    line: NUMBER,
                    detail: DETAIL.into(),
                },
                &[NUMBER_TEXT, DETAIL],
            ),
            with(
                Str::TestScriptFinished {
                    millis: NUMBER as u64,
                },
                &[NUMBER_TEXT],
            ),
            plain(Str::TestsNone),
            plain(Str::TestsNoneHint),
            plain(Str::TestsAddOne),
            plain(Str::TestsScriptDefinedNone),
            plain(Str::TestsScriptDefinedNoneHint),
            plain(Str::TestsNotRun),
            with(Str::TestsPassedCount(NUMBER), &[NUMBER_TEXT]),
            with(Str::TestsFailedCount(NUMBER), &[NUMBER_TEXT]),
            with(Str::TestsErroredCount(NUMBER), &[NUMBER_TEXT]),
            with(Str::TestsDropped(NUMBER), &[NUMBER_TEXT]),
            term(Str::CodeTargetCurl),
            term(Str::CodeTargetFetch),
            term(Str::CodeTargetAxios),
            term(Str::CodeTargetXhr),
            plain(Str::GenerateCodeCarriesValues),
            with(Str::GenerateCodeSecretsWithheld(DETAIL.into()), &[DETAIL]),
            plain(Str::GenerateCodeSecretsRevealed),
            plain(Str::GenerateCodeRevealSecrets),
            // The in-app updater.
            plain(Str::CheckForUpdates),
            plain(Str::SoftwareUpdate),
            plain(Str::UpdateChecking),
            plain(Str::UpdateUpToDate),
            with(Str::UpdateCurrentVersion(DETAIL.into()), &[DETAIL]),
            with(Str::UpdateAvailableHeadline(DETAIL.into()), &[DETAIL]),
            with(Str::UpdatePublished(DETAIL.into()), &[DETAIL]),
            with(Str::UpdateDownloadSize(DETAIL.into()), &[DETAIL]),
            plain(Str::UpdateReleaseNotes),
            plain(Str::UpdateDownloadAction),
            with(
                Str::UpdateDownloadProgress {
                    done: DETAIL.into(),
                    total: NUMBER_TEXT.into(),
                    percent: 42,
                },
                &[DETAIL, NUMBER_TEXT, "42"],
            ),
            plain(Str::UpdateVerifying),
            plain(Str::UpdateInstalling),
            with(Str::UpdateInstalledHeadline(DETAIL.into()), &[DETAIL]),
            plain(Str::UpdateRestartNow),
            plain(Str::UpdateLater),
            plain(Str::UpdateSkipVersion),
            plain(Str::UpdateCancel),
            plain(Str::UpdateRetry),
            plain(Str::UpdateCheckAutomatically),
            with(Str::UpdateManualInstall(DETAIL.into()), &[DETAIL]),
            plain(Str::UpdateManualNotABundle),
            plain(Str::UpdateManualNotWritable),
            plain(Str::UpdateManualReadOnly),
            plain(Str::UpdateFailedHeadline),
            with(Str::UpdateErrorNetwork(DETAIL.into()), &[DETAIL]),
            with(Str::UpdateErrorManifestMalformed(DETAIL.into()), &[DETAIL]),
            plain(Str::UpdateErrorManifestMissingVersion),
            with(
                Str::UpdateErrorManifestUnsupportedVersion {
                    found: NUMBER as u64,
                    supported: 77,
                },
                &[NUMBER_TEXT, "77"],
            ),
            with(
                Str::UpdateErrorManifestUnreadableVersion(DETAIL.into()),
                &[DETAIL],
            ),
            // The framed reason is language-dependent, so only the platform key
            // — which is a wire identifier and never translated — is asserted.
            with(
                Str::UpdateErrorManifestInvalidFile {
                    platform: DETAIL.into(),
                    detail: Box::new(Str::UpdateErrorManifestZeroSize),
                },
                &[DETAIL],
            ),
            with(Str::UpdateErrorManifestBadDigest(DETAIL.into()), &[DETAIL]),
            plain(Str::UpdateErrorManifestZeroSize),
            with(
                Str::UpdateErrorManifestInsecureUrl(DETAIL.into()),
                &[DETAIL],
            ),
            with(Str::UpdateErrorPlatformMissing(DETAIL.into()), &[DETAIL]),
            with(Str::UpdateErrorDownload(DETAIL.into()), &[DETAIL]),
            with(
                Str::UpdateErrorChecksum {
                    expected: DETAIL.into(),
                    actual: NUMBER_TEXT.into(),
                },
                &[DETAIL, NUMBER_TEXT],
            ),
            with(
                Str::UpdateErrorSize {
                    expected: NUMBER as u64,
                    actual: 77,
                },
                &[NUMBER_TEXT, "77"],
            ),
            with(Str::UpdateErrorInstall(DETAIL.into()), &[DETAIL]),
            with(Str::UpdateErrorIo(DETAIL.into()), &[DETAIL]),
            plain(Str::DatabaseTitle),
            plain(Str::DbConnections),
            plain(Str::DbNewConnection),
            plain(Str::DbNoConnections),
            plain(Str::DbNoConnectionsHint),
            plain(Str::DbConnect),
            plain(Str::DbDisconnect),
            plain(Str::DbReconnect),
            plain(Str::DbEditConnection),
            plain(Str::DbEditConnectionTitle),
            plain(Str::DbDuplicateConnection),
            plain(Str::DbDeleteConnection),
            plain(Str::DbCopySuffix),
            plain(Str::DbStatusConnected),
            plain(Str::DbStatusConnecting),
            plain(Str::DbStatusDisconnected),
            plain(Str::DbStatusError),
            plain(Str::DbDeleteConnectionTitle),
            with(Str::DbDeleteConnectionMessage(DETAIL.into()), &[DETAIL]),
            plain(Str::DbCancel),
            plain(Str::DbSave),
            plain(Str::DbFieldName),
            plain(Str::DbFieldNamePlaceholder),
            plain(Str::DbFieldEngine),
            plain(Str::DbFieldHost),
            plain(Str::DbFieldPort),
            plain(Str::DbFieldDatabase),
            plain(Str::DbFieldUser),
            term(Str::DbFieldUrl),
            plain(Str::DbFieldPassword),
            plain(Str::DbFieldFile),
            plain(Str::DbFieldFilePlaceholder),
            term(Str::DbFieldSsl),
            plain(Str::DbSslDisable),
            plain(Str::DbSslPrefer),
            plain(Str::DbSslRequire),
            plain(Str::DbPasswordStorageNotice),
            plain(Str::DbRevealPassword),
            plain(Str::DbHidePassword),
            plain(Str::DbTestConnection),
            plain(Str::DbTesting),
            plain(Str::DbTestSucceeded),
            plain(Str::DbProfileHostMissing),
            plain(Str::DbProfilePortMissing),
            plain(Str::DbProfileDatabaseMissing),
            plain(Str::DbProfileFileMissing),
            plain(Str::DbGroupTables),
            plain(Str::DbGroupViews),
            plain(Str::DbGroupColumns),
            plain(Str::DbGroupIndexes),
            plain(Str::DbGroupConstraints),
            plain(Str::DbTreeLoading),
            plain(Str::DbTreeEmpty),
            plain(Str::DbTreeNotConnected),
            plain(Str::DbRefreshTree),
            plain(Str::DbQuery),
            plain(Str::DbQueryPlaceholder),
            plain(Str::DbExecute),
            plain(Str::DbFormat),
            plain(Str::DbRunning),
            plain(Str::DbNoStatement),
            plain(Str::DbResult),
            plain(Str::DbNoResultYet),
            plain(Str::DbNoResultYetHint),
            plain(Str::DbNoRows),
            with(Str::DbFooterRows(NUMBER), &[NUMBER_TEXT]),
            with(Str::DbFooterRowsAffected(NUMBER as u64), &[NUMBER_TEXT]),
            with(Str::DbFooterElapsed(DETAIL.into()), &[DETAIL]),
            with(Str::DbFooterTruncated(NUMBER), &[NUMBER_TEXT]),
            with(Str::DbFooterCapped(NUMBER), &[NUMBER_TEXT]),
            plain(Str::DbStatementLabel),
            term(Str::DbColumnNull),
            plain(Str::DbSelectConnection),
            plain(Str::DbSelectConnectionHint),
            with(Str::DbConnectionStoreError(DETAIL.into()), &[DETAIL]),
            plain(Str::DbConnectionStoreMissingVersion),
            with(
                Str::DbConnectionStoreUnsupportedVersion {
                    found: NUMBER as u64,
                    supported: 77,
                },
                &[NUMBER_TEXT, "77"],
            ),
            with(Str::DbUnreachable(DETAIL.into()), &[DETAIL]),
            with(Str::DbServerError(DETAIL.into()), &[DETAIL]),
            with(
                Str::DbServerErrorCoded {
                    code: "42P01".into(),
                    detail: DETAIL.into(),
                },
                &["42P01", DETAIL],
            ),
            with(Str::DbQueryTabTitle(NUMBER), &[NUMBER_TEXT]),
            plain(Str::DbNewQueryTab),
            plain(Str::DbCloseQueryTab),
            plain(Str::DbCancelQuery),
            plain(Str::DbCancelledMessage),
            plain(Str::DbCancelledTitle),
            plain(Str::DbCancelledHint),
            with(Str::DbCancelFailed(DETAIL.into()), &[DETAIL]),
            plain(Str::DbExplain),
            plain(Str::DbCopyCell),
            plain(Str::DbCopyRow),
            plain(Str::DbExportCsv),
            plain(Str::DbExportJson),
            with(
                Str::DbExportSucceeded {
                    rows: NUMBER,
                    path: DETAIL.into(),
                },
                &[NUMBER_TEXT, DETAIL],
            ),
            plain(Str::DbExportCancelled),
            with(Str::DbExportFailed(DETAIL.into()), &[DETAIL]),
            plain(Str::DbHistory),
            plain(Str::DbHistorySearch),
            plain(Str::DbHistoryEmpty),
            plain(Str::DbHistoryNoMatches),
            plain(Str::DbDetailData),
            term(Str::DbDetailDdl),
            plain(Str::DbDetailFieldNullable),
            plain(Str::DbDetailFieldDefault),
            plain(Str::DbDetailFieldUnique),
            plain(Str::DbDetailFieldPrimary),
            plain(Str::DbDetailFieldDefinition),
            plain(Str::DbDetailClose),
            plain(Str::DbDetailUnavailable),
            plain(Str::DbDetailNoRows),
            plain(Str::DbDetailNoMetadata),
            plain(Str::DbDetailPrevious),
            plain(Str::DbDetailNext),
            with(Str::DbDetailPage(NUMBER), &[NUMBER_TEXT]),
            with(
                Str::DbDetailRowsRange {
                    first: NUMBER as u64,
                    last: 77,
                },
                &[NUMBER_TEXT, "77"],
            ),
            plain(Str::DbDetailDdlReconstructed),
            plain(Str::DbDetailConstraintsPartial),
            plain(Str::DbDetailCopyDdl),
            plain(Str::DbDetailFieldNotNull),
            with(Str::DbDetailMetadataTruncated(NUMBER), &[NUMBER_TEXT]),
            plain(Str::DbProfileRedisDatabaseInvalid),
            plain(Str::DbGroupMore),
            plain(Str::DbCommandPlaceholder),
            plain(Str::DbEditUnsupported),
            plain(Str::DbEditNoColumns),
            with(Str::DbEditMissingOrigin(DETAIL.into()), &[DETAIL]),
            plain(Str::DbEditMultipleTables),
            with(Str::DbEditDuplicateColumn(DETAIL.into()), &[DETAIL]),
            with(Str::DbEditNoUniqueIdentity(DETAIL.into()), &[DETAIL]),
            with(
                Str::DbEditMissingIdentityColumns {
                    table: DETAIL.into(),
                    columns: "sentinel-columns".into(),
                },
                &[DETAIL, "sentinel-columns"],
            ),
            with(Str::DbEditMetadataFailed(DETAIL.into()), &[DETAIL]),
            plain(Str::DbEditCell),
            plain(Str::DbAddRow),
            plain(Str::DbDeleteRow),
            plain(Str::DbDuplicateRow),
            plain(Str::DbCommit),
            plain(Str::DbRollback),
            plain(Str::DbEditSelectRow),
            plain(Str::DbEditIdentityColumn),
            plain(Str::DbEditIdentityUnavailable),
            plain(Str::DbEditUnsupportedCell),
            plain(Str::DbEditNoPending),
            with(Str::DbPendingChanges(NUMBER), &[NUMBER_TEXT]),
            with(Str::DbEditCellTitle(DETAIL.into()), &[DETAIL]),
            plain(Str::DbAddRowTitle),
            plain(Str::DbDuplicateRowTitle),
            term(Str::DbSetNull),
            with(Str::DbIdentityRequired(DETAIL.into()), &[DETAIL]),
            plain(Str::DbCommitTitle),
            with(Str::DbCommitSummary(NUMBER), &[NUMBER_TEXT]),
            plain(Str::DbCommitExactStatements),
            plain(Str::DbCommitParameters),
            plain(Str::DbCommitLostUpdateNotice),
            plain(Str::DbCommitRunning),
            with(Str::DbCommitSucceeded(NUMBER), &[NUMBER_TEXT]),
            with(
                Str::DbCommitAffectedMismatch {
                    statement: NUMBER,
                    actual: 77,
                },
                &[NUMBER_TEXT, "77"],
            ),
            with(
                Str::DbCommitFailed {
                    statement: NUMBER,
                    detail: DETAIL.into(),
                },
                &[NUMBER_TEXT, DETAIL],
            ),
            with(Str::DbCommitTransactionFailed(DETAIL.into()), &[DETAIL]),
            with(Str::DbCommitStatementLabel(NUMBER), &[NUMBER_TEXT]),
            plain(Str::DbExpectedOneRow),
            plain(Str::DbCommitBuildFailed),
            plain(Str::DbResolvePending),
            plain(Str::DbEditDuplicateRows),
            with(Str::DbQueryStoreError(DETAIL.into()), &[DETAIL]),
            plain(Str::DbQueryStoreMissingVersion),
            with(
                Str::DbQueryStoreUnsupportedVersion {
                    found: NUMBER as u64,
                    supported: 77,
                },
                &[NUMBER_TEXT, "77"],
            ),
            plain(Str::DbSavedQueries),
            plain(Str::DbSaveQuery),
            plain(Str::DbSavedQuerySearch),
            plain(Str::DbSavedQueryEmpty),
            plain(Str::DbSavedQueryNoMatches),
            plain(Str::DbSavedQueryCreateTitle),
            plain(Str::DbSavedQueryEditTitle),
            plain(Str::DbSavedQueryName),
            plain(Str::DbSavedQueryNamePlaceholder),
            plain(Str::DbSavedQueryStatement),
            plain(Str::DbSavedQueryPlaintextNotice),
            plain(Str::DbSavedQueryNameRequired),
            plain(Str::DbSavedQueryStatementRequired),
            plain(Str::DbSavedQueryEdit),
            plain(Str::DbSavedQueryDelete),
            plain(Str::DbSavedQueryDeleteTitle),
            with(Str::DbSavedQueryDeleteMessage(DETAIL.into()), &[DETAIL]),
            with(Str::DbSavedQueryScopeMismatch(DETAIL.into()), &[DETAIL]),
            plain(Str::DbHistoryClear),
            plain(Str::DbHistoryClearTitle),
            plain(Str::DbHistoryClearMessage),
            plain(Str::DbHistorySucceeded),
            plain(Str::DbHistoryFailed),
            plain(Str::DbHistoryJustNow),
            with(Str::DbHistoryMinutesAgo(NUMBER as u64), &[NUMBER_TEXT]),
            with(Str::DbHistoryHoursAgo(NUMBER as u64), &[NUMBER_TEXT]),
            with(Str::DbHistoryDaysAgo(NUMBER as u64), &[NUMBER_TEXT]),
            plain(Str::DbCatalogSearch),
            plain(Str::DbCatalogSearchPlaceholder),
            plain(Str::DbCatalogSearchLoading),
            plain(Str::DbCatalogSearchEmpty),
            plain(Str::DbCatalogSearchNoMatches),
            plain(Str::DbCatalogSearchConnectedOnly),
            with(Str::DbCatalogSearchTruncated(NUMBER), &[NUMBER_TEXT]),
            with(Str::DbCatalogSearchPartial(NUMBER), &[NUMBER_TEXT]),
            with(
                Str::DbCatalogSearchConnectionUnavailable(DETAIL.into()),
                &[DETAIL],
            ),
            plain(Str::DbCatalogKindDatabase),
            plain(Str::DbCatalogKindSchema),
            plain(Str::DbCatalogKindTable),
            plain(Str::DbCatalogKindView),
            plain(Str::DbCatalogKindColumn),
            plain(Str::DbCatalogKindIndex),
            plain(Str::DbCatalogKindConstraint),
            plain(Str::DbCatalogKindNamespace),
            plain(Str::DbCatalogKindKey),
            plain(Str::DbCatalogKindObject),
            plain(Str::DbSavedQueryScope),
            plain(Str::DbFieldUri),
            term(Str::DbFieldUriPlaceholder),
            plain(Str::DbFillFromUri),
            plain(Str::DbUriFilled),
            with(Str::DbUriIgnored(DETAIL.into()), &[DETAIL]),
            plain(Str::DbUriTlsNotApplied),
            plain(Str::DbUriEmpty),
            plain(Str::DbUriNoScheme),
            with(Str::DbUriUnknownScheme(DETAIL.into()), &[DETAIL]),
            with(Str::DbUriInvalidPort(DETAIL.into()), &[DETAIL]),
            plain(Str::DbUriMissingFile),
            plain(Str::DbUriInvalidEscape),
            plain(Str::QuickNavigation),
            plain(Str::QuickNavEnabled),
            plain(Str::QuickNavEnabledDescription),
            plain(Str::QuickNavCurlPattern),
            plain(Str::QuickNavDatabasePattern),
            plain(Str::QuickNavJwtPattern),
            plain(Str::QuickNavJsonPattern),
            plain(Str::QuickNavBase64Pattern),
            plain(Str::QuickNavGateDescription),
            plain(Str::QuickNavShapeDescription),
            with(Str::QuickNavPatternInvalid(DETAIL.into()), &[DETAIL]),
            with(
                Str::QuickNavPatternTooLong {
                    length: NUMBER,
                    limit: 512,
                },
                &[NUMBER_TEXT, "512"],
            ),
            plain(Str::QuickNavStorageProblem),
            with(Str::QuickNavStoreError(DETAIL.into()), &[DETAIL]),
            plain(Str::QuickNavStoreMissingVersion),
            with(
                Str::QuickNavStoreUnsupportedVersion {
                    found: 9,
                    understood: 1,
                },
                &["9", "1"],
            ),
            with(Str::QuickNavOpenedConnection(DETAIL.into()), &[DETAIL]),
            with(Str::QuickNavKeptStoredPassword(DETAIL.into()), &[DETAIL]),
            with(Str::QuickNavCreatedConnection(DETAIL.into()), &[DETAIL]),
            plain(Str::QuickNavConnectionsLoading),
            // Cleaner. Appended for the same reason as the API Explorer block
            // above: a new tool must not renumber every existing entry, even
            // though its `Str` variants sit near the top of the enum.
            plain(Str::CleanerTitle),
            plain(Str::CleanerUnsupportedPlatform),
            plain(Str::CleanerScan),
            plain(Str::CleanerCancelScan),
            plain(Str::CleanerNoResultsYet),
            plain(Str::CleanerStatusScanning),
            plain(Str::CleanerStatusCancelling),
            plain(Str::CleanerStatusPartial),
            plain(Str::CleanerStatusCompleted),
            plain(Str::CleanerStatusCleaning),
            plain(Str::CleanerStatusFailed),
            plain(Str::CleanerSectionCleanup),
            plain(Str::CleanerSectionApplications),
            plain(Str::CleanerSectionAdvanced),
            plain(Str::CleanerCategorySystemJunk),
            plain(Str::CleanerCategoryUserCache),
            plain(Str::CleanerCategoryMailFiles),
            plain(Str::CleanerCategoryTrashBins),
            plain(Str::CleanerCategoryLargeOldFiles),
            plain(Str::CleanerCategoryInstalledApps),
            plain(Str::CleanerCategoryOrphanedFiles),
            plain(Str::CleanerCategoryAiApps),
            plain(Str::CleanerCategoryXcodeJunk),
            plain(Str::CleanerCategoryHomebrewCache),
            plain(Str::CleanerCategoryNodeToolingCache),
            plain(Str::CleanerCategoryDockerCache),
            plain(Str::CleanerCategoryUniversalBinaries),
            plain(Str::CleanerCategoryLanguageFiles),
            plain(Str::CleanerWarnings),
            plain(Str::CleanerPath),
            plain(Str::CleanerExplanation),
            plain(Str::CleanerCopyPath),
            plain(Str::CleanerRevealInFinder),
            plain(Str::CleanerSelectItem),
            plain(Str::CleanerDeselectItem),
            plain(Str::CleanerSelectSafeItems),
            plain(Str::CleanerCleanSelected),
            plain(Str::CleanerCleanupReport),
            plain(Str::CleanerCleanupConfirmTitle),
            with(
                Str::CleanerCleanupConfirmMessage {
                    count: NUMBER,
                    size: DETAIL.into(),
                },
                &[NUMBER_TEXT, DETAIL],
            ),
            with(Str::CleanerCleanupSuccessCount(NUMBER), &[NUMBER_TEXT]),
            with(Str::CleanerCleanupFailureCount(NUMBER), &[NUMBER_TEXT]),
            plain(Str::CleanerPermissionTitle),
            plain(Str::CleanerPermissionExplanation),
            plain(Str::CleanerPermissionOpenSettings),
            plain(Str::CleanerPartialPermissionDenied),
            plain(Str::CleanerPartialRootUnavailable),
            plain(Str::CleanerPartialCancelled),
            plain(Str::CleanerPartialUnsupported),
            plain(Str::CleanerBeginUninstallReview),
            with(
                Str::CleanerUninstallReviewTitle {
                    name: DETAIL.into(),
                },
                &[DETAIL],
            ),
            plain(Str::CleanerUninstallLoading),
            plain(Str::CleanerUninstallRefusedProtected),
            plain(Str::CleanerUninstallRefusedNotApplication),
            plain(Str::CleanerUninstallRelatedFilesHeader),
            plain(Str::CleanerUninstallNoRelatedFiles),
            plain(Str::CleanerUninstallDestinationNote),
            plain(Str::CleanerUninstallScanOnlyBadge),
            plain(Str::CleanerUninstallMoveToTrash),
            plain(Str::CleanerUninstallClose),
            plain(Str::CleanerConfidenceConfirmed),
            plain(Str::CleanerConfidenceHigh),
            plain(Str::CleanerConfidenceMedium),
            plain(Str::CleanerConfidenceLow),
            plain(Str::CleanerConfidenceSharedOrUnsafe),
            plain(Str::CleanerKeepItem),
            with(Str::CleanerIgnoreStoreError(DETAIL.into()), &[DETAIL]),
            plain(Str::CleanerIgnoreStoreMissingVersion),
            with(
                Str::CleanerIgnoreStoreUnsupportedVersion {
                    found: NUMBER as u64,
                    understood: 1,
                },
                &[NUMBER_TEXT, "1"],
            ),
            plain(Str::CleanerDockerCleanupConfirmTitle),
            with(
                Str::CleanerDockerCleanupConfirmMessage {
                    count: NUMBER,
                    size: DETAIL.into(),
                },
                &[NUMBER_TEXT, DETAIL],
            ),
            // Session restoration.
            plain(Str::SessionStorageProblem),
            with(Str::SessionStoreError(DETAIL.into()), &[DETAIL]),
            plain(Str::SessionStoreMissingVersion),
            with(
                Str::SessionStoreUnsupportedVersion {
                    found: 9,
                    understood: 1,
                },
                &["9", "1"],
            ),
            // The Features settings page.
            plain(Str::Features),
            plain(Str::FeaturesDescription),
            plain(Str::FeatureShowInSidebar),
            plain(Str::FeatureDragToReorder),
            plain(Str::FeatureMoveUp),
            plain(Str::FeatureMoveDown),
            plain(Str::FeatureLastVisibleTool),
            // The macOS menu bar item.
            plain(Str::TrayOpenDodo),
            plain(Str::TrayKeyboardInput),
            plain(Str::TrayQuitDodo),
            // The macOS input method.
            plain(Str::InputMethod),
            plain(Str::InputMethodDescription),
            plain(Str::InputMethodInstall),
            plain(Str::InputMethodReinstall),
            plain(Str::InputMethodInstalling),
            plain(Str::InputMethodInstalled),
            with(
                Str::InputMethodInstalledNotActive(NUMBER as i32),
                &[NUMBER_TEXT],
            ),
            plain(Str::InputMethodNoBundle),
            with(Str::InputMethodCopyFailed(DETAIL.into()), &[DETAIL]),
            with(Str::InputMethodInvalidSignature(DETAIL.into()), &[DETAIL]),
            with(Str::InputMethodNeverAppeared(NUMBER as u32), &[NUMBER_TEXT]),
            plain(Str::InputMethodStatus),
            plain(Str::InputMethodNotInstalled),
            with(Str::InputMethodRunning(DETAIL.into()), &[DETAIL]),
            plain(Str::InputMethodInstalledIdle),
            plain(Str::InputMethodSettingsPending),
            plain(Str::InputMethodStorageProblem),
            with(Str::InputMethodStoreError(DETAIL.into()), &[DETAIL]),
            plain(Str::InputMethodStoreMissingVersion),
            with(
                Str::InputMethodStoreUnsupportedVersion {
                    found: NUMBER as u64,
                    supported: 7,
                },
                &[NUMBER_TEXT, "7"],
            ),
            plain(Str::InputMethodScheme),
            plain(Str::InputMethodSchemeDescription),
            // The two scheme names are the same word in both languages, like
            // every other term of art in dodo.
            term(Str::InputMethodTelex),
            term(Str::InputMethodVni),
            plain(Str::InputMethodTonePlacement),
            plain(Str::InputMethodTonePlacementDescription),
            plain(Str::InputMethodToneModern),
            plain(Str::InputMethodToneTraditional),
            plain(Str::InputMethodSpellCheck),
            plain(Str::InputMethodSpellCheckDescription),
            plain(Str::InputMethodBracketShortcuts),
            plain(Str::InputMethodBracketShortcutsDescription),
            plain(Str::InputMethodBackend),
            plain(Str::InputMethodBackendDescription),
            plain(Str::InputMethodNative),
            plain(Str::InputMethodEventTap),
            plain(Str::InputMethodEventTapStatus),
            plain(Str::InputMethodEventTapInactive),
            plain(Str::InputMethodEventTapWaitingForNative),
            plain(Str::InputMethodEventTapNeedsAccessibility),
            plain(Str::InputMethodEventTapRunning),
            plain(Str::InputMethodEventTapFailed),
            plain(Str::InputMethodWindowsDescription),
            plain(Str::InputMethodWindowsLanguageDescription),
            plain(Str::InputMethodNativeTsf),
            plain(Str::InputMethodWindowsTsfStatus),
            plain(Str::InputMethodWindowsTsfNotInstalled),
            plain(Str::InputMethodWindowsTsfInstalled),
            plain(Str::InputMethodWindowsTsfRemoved),
            plain(Str::InputMethodWindowsTsfNoDll),
            with(
                Str::InputMethodWindowsTsfRegisterFailed(DETAIL.into()),
                &[DETAIL],
            ),
            with(
                Str::InputMethodWindowsTsfUnregisterFailed(DETAIL.into()),
                &[DETAIL],
            ),
            term(Str::InputMethodKeyboardHook),
            plain(Str::InputMethodKeyboardHookStatus),
            plain(Str::InputMethodKeyboardHookInactive),
            plain(Str::InputMethodKeyboardHookRunning),
            plain(Str::InputMethodKeyboardHookFailed),
            plain(Str::InputMethodUninstall),
            plain(Str::InputMethodUninstalling),
            plain(Str::CleanerRevealInExplorer),
            plain(Str::CleanerRevealInFileManager),
            plain(Str::CleanerMoreActions),
            plain(Str::CleanerColumnName),
            plain(Str::CleanerColumnRisk),
            plain(Str::CleanerColumnSize),
            plain(Str::CleanerColumnActions),
            plain(Str::CleanerRiskSafe),
            plain(Str::CleanerRiskReview),
            plain(Str::CleanerRiskUserData),
            plain(Str::CleanerRiskAppChange),
            plain(Str::CleanerRiskProtected),
            // Docker module (round 7) — the Runtimes tab.
            term(Str::Runtimes),
            plain(Str::RuntimesDescription),
            term(Str::RuntimePodmanMachine),
            term(Str::RuntimeKubernetes),
            term(Str::RuntimeContainerd),
            plain(Str::RuntimeStatusRunning),
            plain(Str::RuntimeStatusStopped),
            plain(Str::RuntimeStatusNotInstalled),
            plain(Str::RuntimeStatusUnsupported),
            plain(Str::RuntimeStatusUnknown),
            plain(Str::RuntimeManagedExternally),
            plain(Str::RuntimeStarting),
            plain(Str::RuntimeStopping),
            plain(Str::RuntimeBinaryNotFound),
            plain(Str::RuntimeActionUnsupported),
            // Cleaner UX/state refactor — appended for the same reason the
            // block above is: a new string must not renumber every existing
            // one.
            plain(Str::CleanerScanDescription),
            with(
                Str::CleanerEntriesScannedCount(NUMBER as u64),
                &[NUMBER_TEXT],
            ),
            with(Str::CleanerBytesDiscovered(DETAIL.into()), &[DETAIL]),
            with(Str::CleanerReclaimableAmount(DETAIL.into()), &[DETAIL]),
            with(Str::CleanerItemsFound(NUMBER), &[NUMBER_TEXT]),
            with(Str::CleanerSafeItemsCount(NUMBER), &[NUMBER_TEXT]),
            with(Str::CleanerWarningCount(NUMBER), &[NUMBER_TEXT]),
            with(
                Str::CleanerSelectedSummary {
                    count: NUMBER,
                    size: DETAIL.into(),
                },
                &[NUMBER_TEXT, DETAIL],
            ),
            with(
                Str::CleanerCleanCount {
                    count: NUMBER,
                    size: DETAIL.into(),
                },
                &[NUMBER_TEXT, DETAIL],
            ),
            with(Str::CleanerScanWarningsSummary(NUMBER), &[NUMBER_TEXT]),
            plain(Str::CleanerScanWarningsShowDetails),
            plain(Str::CleanerScanWarningsHideDetails),
            plain(Str::CleanerRescan),
            plain(Str::CleanerSelectAll),
            plain(Str::CleanerDeselectAll),
            plain(Str::CleanerPermissionNotNow),
            plain(Str::CleanerStatusCompletedWithWarnings),
            plain(Str::CleanerStatusCancelled),
            // Close-to-tray and OS startup.
            plain(Str::StartWithOs),
            plain(Str::StartWithOsDescription),
        ]
    }

    /// Exhaustive over `Str`: a new variant does not compile until it is given
    /// a position, and `samples` must then have an entry at that position.
    fn position(str: &Str) -> usize {
        match str {
            Str::Settings => 0,
            Str::General => 1,
            Str::Appearance => 2,
            Str::Language => 3,
            Str::LanguageDescription => 4,
            Str::Theme => 5,
            Str::ThemeDescription => 6,
            Str::FontSize => 7,
            Str::FontSizeDescription => 8,
            Str::BorderRadius => 9,
            Str::BorderRadiusDescription => 10,
            Str::Large => 11,
            Str::Medium => 12,
            Str::Small => 13,
            Str::SearchSettingsPlaceholder => 14,
            Str::NoSettingsMatch => 15,
            Str::Tools => 16,
            Str::JsonFormatterTitle => 17,
            Str::EncoderDecoderTitle => 18,
            Str::JsonPlaceholder => 19,
            Str::FormatButton => 20,
            Str::IndentLabel => 21,
            Str::IndentSpaces(_) => 22,
            Str::InvalidJson { .. } => 23,
            Str::FormatLabel => 24,
            Str::EncodeButton => 25,
            Str::DecodeButton => 26,
            Str::DecodeJwtButton => 27,
            Str::InputLabel => 28,
            Str::OutputLabel => 29,
            Str::JwtHeaderLabel => 30,
            Str::JwtPayloadLabel => 31,
            Str::JwtSignatureLabel => 32,
            Str::EncoderInputPlaceholder => 33,
            Str::EncoderOutputPlaceholder => 34,
            Str::FormatBase64 => 35,
            Str::FormatBase64UrlSafe => 36,
            Str::FormatUrl => 37,
            Str::FormatHex => 38,
            Str::FormatJwt => 39,
            Str::JwtEncodeUnsupported => 40,
            Str::InvalidHexOddLength(_) => 41,
            Str::InvalidHexDigit { .. } => 42,
            Str::InvalidBase64(_) => 43,
            Str::InvalidPercentAt(_) => 44,
            Str::InvalidPercentEncoding(_) => 45,
            Str::NotUtf8(_) => 46,
            Str::JwtEmpty => 47,
            Str::JwtPartCount(_) => 48,
            Str::JwtPartNotBase64 { .. } => 49,
            Str::JwtPartNotJson { .. } => 50,
            Str::JwtPartNotRenderable { .. } => 51,
            Str::ApiExplorerTitle => 52,
            Str::Collections => 53,
            Str::NoCollections => 54,
            Str::NoCollectionsHint => 55,
            Str::UrlPlaceholder => 56,
            Str::Send => 57,
            Str::NewRequest => 58,
            Str::CloseRequest => 59,
            Str::NameRequest => 60,
            Str::NameRequestPlaceholder => 61,
            Str::SaveName => 62,
            Str::GenerateCode => 63,
            Str::RequestTabParams => 64,
            Str::RequestTabHeaders => 65,
            Str::RequestTabBody => 66,
            Str::RequestTabAuth => 67,
            Str::RequestTabScripts => 68,
            Str::ColumnKey => 69,
            Str::ColumnValue => 70,
            Str::Add => 71,
            Str::AddParameter => 72,
            Str::AddHeader => 73,
            Str::DeleteRow => 74,
            Str::NoActiveParams => 75,
            Str::ActiveParams(_) => 76,
            Str::NoActiveHeaders => 77,
            Str::ActiveHeaders(_) => 78,
            Str::ParamKeyPlaceholder => 79,
            Str::ParamValuePlaceholder => 80,
            Str::HeaderKeyPlaceholder => 81,
            Str::HeaderValuePlaceholder => 82,
            Str::ColumnDescription => 83,
            Str::DescriptionPlaceholder => 84,
            Str::DuplicateRow => 85,
            Str::MoveRowUp => 86,
            Str::MoveRowDown => 87,
            Str::AddField => 88,
            Str::NoActiveFields => 89,
            Str::ActiveFields(_) => 90,
            Str::FieldKeyPlaceholder => 91,
            Str::FieldValuePlaceholder => 92,
            Str::BodyTypeNone => 93,
            Str::BodyTypeJson => 94,
            Str::BodyTypeText => 95,
            Str::BodyTypeXml => 96,
            Str::BodyTypeHtml => 97,
            Str::BodyTypeFormData => 98,
            Str::BodyTypeUrlEncoded => 99,
            Str::BodyTypeBinary => 100,
            Str::BodyPlaceholder => 101,
            Str::NoBodyTitle => 102,
            Str::NoBodyHint => 103,
            Str::BinaryBodyHint => 104,
            Str::MethodSendsNoBody(_) => 105,
            Str::AuthTypeLabel => 106,
            Str::AuthTypeNone => 107,
            Str::AuthTypeBearer => 108,
            Str::AuthTypeBasic => 109,
            Str::AuthTypeApiKey => 110,
            Str::AuthTypeOAuth2 => 111,
            Str::OAuth2Later => 112,
            Str::NoAuthTitle => 113,
            Str::NoAuthHint => 114,
            Str::AuthTokenLabel => 115,
            Str::AuthTokenPlaceholder => 116,
            Str::AuthUsernameLabel => 117,
            Str::AuthUsernamePlaceholder => 118,
            Str::AuthPasswordLabel => 119,
            Str::AuthPasswordPlaceholder => 120,
            Str::ApiKeyNameLabel => 121,
            Str::ApiKeyNamePlaceholder => 122,
            Str::ApiKeyValueLabel => 123,
            Str::ApiKeyValuePlaceholder => 124,
            Str::ApiKeySendAs => 125,
            Str::ApiKeyInHeader => 126,
            Str::ApiKeyInQuery => 127,
            Str::ScriptsSandboxNotice => 128,
            Str::PreRequestScriptLabel => 129,
            Str::PreRequestScriptPlaceholder => 130,
            Str::PostResponseScriptLabel => 131,
            Str::PostResponseScriptPlaceholder => 132,
            Str::ResponseTabBody => 133,
            Str::ResponseTabHeaders => 134,
            Str::ResponseTabCookies => 135,
            Str::ResponseTabTests => 136,
            Str::ResponseTabConsole => 137,
            Str::NoResponseYet => 138,
            Str::NoResponseHint => 139,
            Str::Sending => 140,
            Str::RequestFailed => 141,
            Str::CollapseResponse => 142,
            Str::ExpandResponse => 143,
            Str::BodyPretty => 144,
            Str::BodyRaw => 145,
            Str::Copy => 146,
            Str::LoadMoreLines => 147,
            Str::BodyTruncated => 148,
            Str::LineRange { .. } => 149,
            Str::StatusClassInfo => 150,
            Str::StatusClassSuccess => 151,
            Str::StatusClassRedirect => 152,
            Str::StatusClassClientError => 153,
            Str::StatusClassServerError => 154,
            Str::StatusClassUnknown => 155,
            Str::HttpInvalidUrl(_) => 156,
            Str::HttpUnsupportedScheme(_) => 157,
            Str::HttpInvalidHeader(_) => 158,
            Str::HttpTimeout(_) => 159,
            Str::HttpDnsFailure(_) => 160,
            Str::HttpConnectFailure(_) => 161,
            Str::HttpTlsFailure(_) => 162,
            Str::HttpBodyNotText(_) => 163,
            Str::HttpUnexpected(_) => 164,
            Str::ImportCollection => 165,
            Str::NewCollection => 166,
            Str::NewFolder => 167,
            Str::SearchCollectionsPlaceholder => 168,
            Str::Rename => 169,
            Str::Delete => 170,
            Str::Duplicate => 171,
            Str::Open => 172,
            Str::MoreActions => 173,
            Str::NamePlaceholder => 174,
            Str::DefaultCollectionName => 175,
            Str::DefaultFolderName => 176,
            Str::SaveToCollectionNote => 177,
            Str::CollectionStoreError(_) => 178,
            Str::CollectionImportError(_) => 179,
            Str::History => 180,
            Str::NoHistory => 181,
            Str::NoHistoryHint => 182,
            Str::HistoryReopen => 183,
            Str::HistoryResend => 184,
            Str::HistoryClearAll => 185,
            Str::HistoryJustNow => 186,
            Str::HistoryMinutesAgo(_) => 187,
            Str::HistoryHoursAgo(_) => 188,
            Str::HistoryDaysAgo(_) => 189,
            Str::BodyPreview => 190,
            Str::BodyTree => 191,
            Str::SaveToFile => 192,
            Str::JsonTreeTruncated(_) => 193,
            Str::HtmlPreviewNote => 194,
            Str::NoCookies => 195,
            Str::NoCookiesHint => 196,
            Str::ToggleAllRows => 197,
            Str::EditModeTable => 198,
            Str::EditModeBulk => 199,
            Str::BulkEditPlaceholder => 200,
            Str::InsertTemplate => 201,
            Str::TemplateSetHeader => 202,
            Str::TemplateSetBearerToken => 203,
            Str::TemplateSetTimestamp => 204,
            Str::TemplateAssertStatus => 205,
            Str::TemplateLogResponse => 206,
            Str::TemplateExtractField => 207,
            Str::Docker => 208,
            Str::Containers => 209,
            Str::Images => 210,
            Str::Volumes => 211,
            Str::Networks => 212,
            Str::DockerSearchPlaceholder => 213,
            Str::DockerRefresh => 214,
            Str::DockerFilter => 215,
            Str::DockerCreate => 216,
            Str::DockerColumnName => 217,
            Str::DockerColumnImage => 218,
            Str::DockerColumnStatus => 219,
            Str::DockerColumnCpu => 220,
            Str::DockerColumnPorts => 221,
            Str::DockerColumnLastStarted => 222,
            Str::DockerColumnActions => 223,
            Str::DockerStatusRunning => 224,
            Str::DockerStatusExited => 225,
            Str::DockerStatusCreated => 226,
            Str::DockerStatusRestarting => 227,
            Str::DockerStatusPaused => 228,
            Str::DockerStatusDead => 229,
            Str::DockerStatusRemoving => 230,
            Str::DockerStatusStopping => 231,
            Str::DockerStatusUnknown => 232,
            Str::DockerStart => 233,
            Str::DockerStop => 234,
            Str::DockerRestart => 235,
            Str::DockerDeleteTitle => 236,
            Str::DockerDeleteMessage(_) => 237,
            Str::DockerCancel => 238,
            Str::NoContainers => 239,
            Str::NoContainersHint => 240,
            Str::DockerRetry => 241,
            Str::DockerConnectionError(_) => 242,
            Str::DockerOperationError(_) => 243,
            Str::DockerSelectAll => 244,
            Str::DockerSelectRow => 245,
            Str::DockerRelNever => 246,
            Str::DockerRelJustNow => 247,
            Str::DockerRelSecondsAgo(_) => 248,
            Str::DockerRelMinutesAgo(_) => 249,
            Str::DockerRelHoursAgo(_) => 250,
            Str::DockerRelDaysAgo(_) => 251,
            Str::DockerRelWeeksAgo(_) => 252,
            Str::DockerRelMonthsAgo(_) => 253,
            Str::DockerRelYearsAgo(_) => 254,
            Str::DockerUnreachableTitle => 255,
            Str::DockerUngrouped => 256,
            Str::DockerGroupContainers(_) => 257,
            Str::DockerGroupRunning(_) => 258,
            Str::DockerFilterWithCount(_) => 259,
            Str::DockerFilterTitle => 260,
            Str::DockerFilterProject => 261,
            Str::DockerFilterPublishedPorts => 262,
            Str::DockerFilterFavorites => 263,
            Str::DockerFilterClear => 264,
            Str::DockerBulkSelected(_) => 265,
            Str::DockerBulkStart => 266,
            Str::DockerBulkStop => 267,
            Str::DockerBulkDelete => 268,
            Str::DockerBulkClear => 269,
            Str::DockerBulkDeleteTitle => 270,
            Str::DockerBulkDeleteMessage(_) => 271,
            Str::DockerBulkFailures(_) => 272,
            Str::DockerColumnRepository => 273,
            Str::DockerColumnTag => 274,
            Str::DockerColumnImageId => 275,
            Str::DockerColumnSize => 276,
            Str::DockerColumnCreated => 277,
            Str::DockerColumnContainersUsing => 278,
            Str::DockerColumnDriver => 279,
            Str::DockerColumnMountPoint => 280,
            Str::DockerColumnScope => 281,
            Str::DockerSearchImages => 282,
            Str::DockerSearchVolumes => 283,
            Str::DockerSearchNetworks => 284,
            Str::NoImages => 285,
            Str::NoImagesHint => 286,
            Str::NoVolumes => 287,
            Str::NoVolumesHint => 288,
            Str::NoNetworks => 289,
            Str::NoNetworksHint => 290,
            Str::DockerNotAvailable => 291,
            Str::DockerNone => 292,
            Str::DockerInspect => 293,
            Str::DockerNetworkPredefined => 294,
            Str::DockerViewLogs => 295,
            Str::DockerOpenTerminal => 296,
            Str::DockerComingSoonLabel => 297,
            Str::DockerDetails => 298,
            Str::DockerRawJson => 299,
            Str::DockerDetailErrorTitle => 300,
            Str::DockerNoLogs => 301,
            Str::DockerNoLogsHint => 302,
            Str::DockerLogsTail(_) => 303,
            Str::DockerYes => 304,
            Str::DockerNo => 305,
            Str::DockerFieldId => 306,
            Str::DockerFieldCommand => 307,
            Str::DockerFieldStarted => 308,
            Str::DockerFieldExitCode => 309,
            Str::DockerFieldRestartPolicy => 310,
            Str::DockerFieldIpAddress => 311,
            Str::DockerFieldMounts => 312,
            Str::DockerFieldTags => 313,
            Str::DockerFieldDigest => 314,
            Str::DockerFieldArchitecture => 315,
            Str::DockerFieldOs => 316,
            Str::DockerFieldLayers => 317,
            Str::DockerFieldLabels => 318,
            Str::DockerFieldOptions => 319,
            Str::DockerFieldInternal => 320,
            Str::DockerFieldAttachable => 321,
            Str::DockerFieldSubnet => 322,
            Str::DockerFieldGateway => 323,
            Str::DockerPull => 324,
            Str::DockerBuild => 325,
            Str::DockerStats => 326,
            Str::DockerOpenDetails => 327,
            Str::UntitledRequest => 328,
            Str::ColumnType => 329,
            Str::FieldKindText => 330,
            Str::FieldKindFile => 331,
            Str::ChooseFile => 332,
            Str::ReplaceFile => 333,
            Str::ClearFile => 334,
            Str::NoFileSelected => 335,
            Str::IncompleteFileFields(_) => 336,
            Str::HttpFileUnreadable { .. } => 337,
            Str::HttpFileTooLarge { .. } => 338,
            Str::NoEnvironment => 339,
            Str::SelectEnvironment => 340,
            Str::ManageEnvironments => 341,
            Str::Environments => 342,
            Str::NewEnvironment => 343,
            Str::DefaultEnvironmentName => 344,
            Str::EnvironmentCopySuffix => 345,
            Str::DuplicateEnvironment => 346,
            Str::DeleteEnvironment => 347,
            Str::ImportEnvironment => 348,
            Str::CollectionVariables => 349,
            Str::EnvironmentVariables => 350,
            Str::CollectionVariablesNote => 351,
            Str::NoEnvironmentsYet => 352,
            Str::NoEnvironmentsYetHint => 353,
            Str::ColumnSecret => 354,
            Str::AddVariable => 355,
            Str::NoActiveVariables => 356,
            Str::ActiveVariables(_) => 357,
            Str::VariableKeyPlaceholder => 358,
            Str::VariableValuePlaceholder => 359,
            Str::MarkSecret => 360,
            Str::RevealSecret => 361,
            Str::HideSecret => 362,
            Str::SecretStorageWarning => 363,
            Str::ResolvedUrlLabel => 364,
            Str::UnresolvedVariablePreview(_) => 365,
            Str::ResolvesFrom { .. } => 366,
            Str::HttpUnresolvedVariable(_) => 367,
            Str::HttpRecursiveVariable(_) => 368,
            Str::VariableStoreError(_) => 369,
            Str::VariableStoreMissingVersion => 370,
            Str::VariableStoreUnsupportedVersion { .. } => 371,
            Str::EnvironmentImportError(_) => 372,
            Str::ScriptVariables => 373,
            Str::ScriptThrew(_) => 374,
            Str::ScriptDeadline(_) => 375,
            Str::ScriptOutOfMemory => 376,
            Str::ScriptUnsupported(_) => 377,
            Str::ScriptNoEngine => 378,
            Str::ScriptSkippedByPolicy => 379,
            Str::ScriptSkippedByConsent => 380,
            Str::ScriptFinished { .. } => 381,
            Str::ScriptWroteVariables(_) => 382,
            Str::ScriptUnknownMethod(_) => 383,
            Str::ConsoleLevelDebug => 384,
            Str::ConsoleLevelLog => 385,
            Str::ConsoleLevelWarn => 386,
            Str::ConsoleLevelError => 387,
            Str::ConsoleRunSeparator { .. } => 388,
            Str::ConsoleRunTruncated(_) => 389,
            Str::ConsoleEmpty => 390,
            Str::ConsoleEmptyHint => 391,
            Str::ConsoleClear => 392,
            Str::ConsoleDropped(_) => 393,
            Str::RunScripts => 394,
            Str::RunScriptsDescription => 395,
            Str::RunScriptsNever => 396,
            Str::RunScriptsAskImported => 397,
            Str::RunScriptsAlways => 398,
            Str::ScriptConsentTitle => 399,
            Str::ScriptConsentExplain => 400,
            Str::ScriptConsentRequest(_) => 401,
            Str::ScriptConsentRun => 402,
            Str::ScriptConsentSkip => 403,
            Str::ConsentStoreError(_) => 404,
            Str::ConsentStoreMissingVersion => 405,
            Str::ConsentStoreUnsupportedVersion { .. } => 406,
            Str::ScriptConsentExplainChanged => 407,
            Str::ScriptSyntaxError(_) => 408,
            Str::ScriptSyntaxErrorAt { .. } => 409,
            Str::TestScriptFinished { .. } => 410,
            Str::TestsNone => 411,
            Str::TestsNoneHint => 412,
            Str::TestsAddOne => 413,
            Str::TestsScriptDefinedNone => 414,
            Str::TestsScriptDefinedNoneHint => 415,
            Str::TestsNotRun => 416,
            Str::TestsPassedCount(_) => 417,
            Str::TestsFailedCount(_) => 418,
            Str::TestsErroredCount(_) => 419,
            Str::TestsDropped(_) => 420,
            Str::CodeTargetCurl => 421,
            Str::CodeTargetFetch => 422,
            Str::CodeTargetAxios => 423,
            Str::CodeTargetXhr => 424,
            Str::GenerateCodeCarriesValues => 425,
            Str::GenerateCodeSecretsWithheld(_) => 426,
            Str::GenerateCodeSecretsRevealed => 427,
            Str::GenerateCodeRevealSecrets => 428,
            Str::CheckForUpdates => 429,
            Str::SoftwareUpdate => 430,
            Str::UpdateChecking => 431,
            Str::UpdateUpToDate => 432,
            Str::UpdateCurrentVersion(_) => 433,
            Str::UpdateAvailableHeadline(_) => 434,
            Str::UpdatePublished(_) => 435,
            Str::UpdateDownloadSize(_) => 436,
            Str::UpdateReleaseNotes => 437,
            Str::UpdateDownloadAction => 438,
            Str::UpdateDownloadProgress { .. } => 439,
            Str::UpdateVerifying => 440,
            Str::UpdateInstalling => 441,
            Str::UpdateInstalledHeadline(_) => 442,
            Str::UpdateRestartNow => 443,
            Str::UpdateLater => 444,
            Str::UpdateSkipVersion => 445,
            Str::UpdateCancel => 446,
            Str::UpdateRetry => 447,
            Str::UpdateCheckAutomatically => 448,
            Str::UpdateManualInstall(_) => 449,
            Str::UpdateManualNotABundle => 450,
            Str::UpdateManualNotWritable => 451,
            Str::UpdateManualReadOnly => 452,
            Str::UpdateFailedHeadline => 453,
            Str::UpdateErrorNetwork(_) => 454,
            Str::UpdateErrorManifestMalformed(_) => 455,
            Str::UpdateErrorManifestMissingVersion => 456,
            Str::UpdateErrorManifestUnsupportedVersion { .. } => 457,
            Str::UpdateErrorManifestUnreadableVersion(_) => 458,
            Str::UpdateErrorManifestInvalidFile { .. } => 459,
            Str::UpdateErrorManifestBadDigest(_) => 460,
            Str::UpdateErrorManifestZeroSize => 461,
            Str::UpdateErrorManifestInsecureUrl(_) => 462,
            Str::UpdateErrorPlatformMissing(_) => 463,
            Str::UpdateErrorDownload(_) => 464,
            Str::UpdateErrorChecksum { .. } => 465,
            Str::UpdateErrorSize { .. } => 466,
            Str::UpdateErrorInstall(_) => 467,
            Str::UpdateErrorIo(_) => 468,
            Str::DatabaseTitle => 469,
            Str::DbConnections => 470,
            Str::DbNewConnection => 471,
            Str::DbNoConnections => 472,
            Str::DbNoConnectionsHint => 473,
            Str::DbConnect => 474,
            Str::DbDisconnect => 475,
            Str::DbReconnect => 476,
            Str::DbEditConnection => 477,
            Str::DbEditConnectionTitle => 478,
            Str::DbDuplicateConnection => 479,
            Str::DbDeleteConnection => 480,
            Str::DbCopySuffix => 481,
            Str::DbStatusConnected => 482,
            Str::DbStatusConnecting => 483,
            Str::DbStatusDisconnected => 484,
            Str::DbStatusError => 485,
            Str::DbDeleteConnectionTitle => 486,
            Str::DbDeleteConnectionMessage(_) => 487,
            Str::DbCancel => 488,
            Str::DbSave => 489,
            Str::DbFieldName => 490,
            Str::DbFieldNamePlaceholder => 491,
            Str::DbFieldEngine => 492,
            Str::DbFieldHost => 493,
            Str::DbFieldPort => 494,
            Str::DbFieldDatabase => 495,
            Str::DbFieldUser => 496,
            Str::DbFieldUrl => 497,
            Str::DbFieldPassword => 498,
            Str::DbFieldFile => 499,
            Str::DbFieldFilePlaceholder => 500,
            Str::DbFieldSsl => 501,
            Str::DbSslDisable => 502,
            Str::DbSslPrefer => 503,
            Str::DbSslRequire => 504,
            Str::DbPasswordStorageNotice => 505,
            Str::DbRevealPassword => 506,
            Str::DbHidePassword => 507,
            Str::DbTestConnection => 508,
            Str::DbTesting => 509,
            Str::DbTestSucceeded => 510,
            Str::DbProfileHostMissing => 511,
            Str::DbProfilePortMissing => 512,
            Str::DbProfileDatabaseMissing => 513,
            Str::DbProfileFileMissing => 514,
            Str::DbGroupTables => 515,
            Str::DbGroupViews => 516,
            Str::DbGroupColumns => 517,
            Str::DbGroupIndexes => 518,
            Str::DbGroupConstraints => 519,
            Str::DbTreeLoading => 520,
            Str::DbTreeEmpty => 521,
            Str::DbTreeNotConnected => 522,
            Str::DbRefreshTree => 523,
            Str::DbQuery => 524,
            Str::DbQueryPlaceholder => 525,
            Str::DbExecute => 526,
            Str::DbFormat => 527,
            Str::DbRunning => 528,
            Str::DbNoStatement => 529,
            Str::DbResult => 530,
            Str::DbNoResultYet => 531,
            Str::DbNoResultYetHint => 532,
            Str::DbNoRows => 533,
            Str::DbFooterRows(_) => 534,
            Str::DbFooterRowsAffected(_) => 535,
            Str::DbFooterElapsed(_) => 536,
            Str::DbFooterTruncated(_) => 537,
            Str::DbFooterCapped(_) => 538,
            Str::DbStatementLabel => 539,
            Str::DbColumnNull => 540,
            Str::DbSelectConnection => 541,
            Str::DbSelectConnectionHint => 542,
            Str::DbConnectionStoreError(_) => 543,
            Str::DbConnectionStoreMissingVersion => 544,
            Str::DbConnectionStoreUnsupportedVersion { .. } => 545,
            Str::DbUnreachable(_) => 546,
            Str::DbServerError(_) => 547,
            Str::DbServerErrorCoded { .. } => 548,
            Str::DbQueryTabTitle(_) => 549,
            Str::DbNewQueryTab => 550,
            Str::DbCloseQueryTab => 551,
            Str::DbCancelQuery => 552,
            Str::DbCancelledMessage => 553,
            Str::DbCancelledTitle => 554,
            Str::DbCancelledHint => 555,
            Str::DbCancelFailed(_) => 556,
            Str::DbExplain => 557,
            Str::DbCopyCell => 558,
            Str::DbCopyRow => 559,
            Str::DbExportCsv => 560,
            Str::DbExportJson => 561,
            Str::DbExportSucceeded { .. } => 562,
            Str::DbExportCancelled => 563,
            Str::DbExportFailed(_) => 564,
            Str::DbHistory => 565,
            Str::DbHistorySearch => 566,
            Str::DbHistoryEmpty => 567,
            Str::DbHistoryNoMatches => 568,
            Str::DbDetailData => 569,
            Str::DbDetailDdl => 570,
            Str::DbDetailFieldNullable => 571,
            Str::DbDetailFieldDefault => 572,
            Str::DbDetailFieldUnique => 573,
            Str::DbDetailFieldPrimary => 574,
            Str::DbDetailFieldDefinition => 575,
            Str::DbDetailClose => 576,
            Str::DbDetailUnavailable => 577,
            Str::DbDetailNoRows => 578,
            Str::DbDetailNoMetadata => 579,
            Str::DbDetailPrevious => 580,
            Str::DbDetailNext => 581,
            Str::DbDetailPage(_) => 582,
            Str::DbDetailRowsRange { .. } => 583,
            Str::DbDetailDdlReconstructed => 584,
            Str::DbDetailConstraintsPartial => 585,
            Str::DbDetailCopyDdl => 586,
            Str::DbDetailFieldNotNull => 587,
            Str::DbDetailMetadataTruncated(_) => 588,
            Str::DbProfileRedisDatabaseInvalid => 589,
            Str::DbGroupMore => 590,
            Str::DbCommandPlaceholder => 591,
            Str::DbEditUnsupported => 592,
            Str::DbEditNoColumns => 593,
            Str::DbEditMissingOrigin(_) => 594,
            Str::DbEditMultipleTables => 595,
            Str::DbEditDuplicateColumn(_) => 596,
            Str::DbEditNoUniqueIdentity(_) => 597,
            Str::DbEditMissingIdentityColumns { .. } => 598,
            Str::DbEditMetadataFailed(_) => 599,
            Str::DbEditCell => 600,
            Str::DbAddRow => 601,
            Str::DbDeleteRow => 602,
            Str::DbDuplicateRow => 603,
            Str::DbCommit => 604,
            Str::DbRollback => 605,
            Str::DbEditSelectRow => 606,
            Str::DbEditIdentityColumn => 607,
            Str::DbEditIdentityUnavailable => 608,
            Str::DbEditUnsupportedCell => 609,
            Str::DbEditNoPending => 610,
            Str::DbPendingChanges(_) => 611,
            Str::DbEditCellTitle(_) => 612,
            Str::DbAddRowTitle => 613,
            Str::DbDuplicateRowTitle => 614,
            Str::DbSetNull => 615,
            Str::DbIdentityRequired(_) => 616,
            Str::DbCommitTitle => 617,
            Str::DbCommitSummary(_) => 618,
            Str::DbCommitExactStatements => 619,
            Str::DbCommitParameters => 620,
            Str::DbCommitLostUpdateNotice => 621,
            Str::DbCommitRunning => 622,
            Str::DbCommitSucceeded(_) => 623,
            Str::DbCommitAffectedMismatch { .. } => 624,
            Str::DbCommitFailed { .. } => 625,
            Str::DbCommitTransactionFailed(_) => 626,
            Str::DbCommitStatementLabel(_) => 627,
            Str::DbExpectedOneRow => 628,
            Str::DbCommitBuildFailed => 629,
            Str::DbResolvePending => 630,
            Str::DbEditDuplicateRows => 631,
            Str::DbQueryStoreError(_) => 632,
            Str::DbQueryStoreMissingVersion => 633,
            Str::DbQueryStoreUnsupportedVersion { .. } => 634,
            Str::DbSavedQueries => 635,
            Str::DbSaveQuery => 636,
            Str::DbSavedQuerySearch => 637,
            Str::DbSavedQueryEmpty => 638,
            Str::DbSavedQueryNoMatches => 639,
            Str::DbSavedQueryCreateTitle => 640,
            Str::DbSavedQueryEditTitle => 641,
            Str::DbSavedQueryName => 642,
            Str::DbSavedQueryNamePlaceholder => 643,
            Str::DbSavedQueryStatement => 644,
            Str::DbSavedQueryPlaintextNotice => 645,
            Str::DbSavedQueryNameRequired => 646,
            Str::DbSavedQueryStatementRequired => 647,
            Str::DbSavedQueryEdit => 648,
            Str::DbSavedQueryDelete => 649,
            Str::DbSavedQueryDeleteTitle => 650,
            Str::DbSavedQueryDeleteMessage(_) => 651,
            Str::DbSavedQueryScopeMismatch(_) => 652,
            Str::DbHistoryClear => 653,
            Str::DbHistoryClearTitle => 654,
            Str::DbHistoryClearMessage => 655,
            Str::DbHistorySucceeded => 656,
            Str::DbHistoryFailed => 657,
            Str::DbHistoryJustNow => 658,
            Str::DbHistoryMinutesAgo(_) => 659,
            Str::DbHistoryHoursAgo(_) => 660,
            Str::DbHistoryDaysAgo(_) => 661,
            Str::DbCatalogSearch => 662,
            Str::DbCatalogSearchPlaceholder => 663,
            Str::DbCatalogSearchLoading => 664,
            Str::DbCatalogSearchEmpty => 665,
            Str::DbCatalogSearchNoMatches => 666,
            Str::DbCatalogSearchConnectedOnly => 667,
            Str::DbCatalogSearchTruncated(_) => 668,
            Str::DbCatalogSearchPartial(_) => 669,
            Str::DbCatalogSearchConnectionUnavailable(_) => 670,
            Str::DbCatalogKindDatabase => 671,
            Str::DbCatalogKindSchema => 672,
            Str::DbCatalogKindTable => 673,
            Str::DbCatalogKindView => 674,
            Str::DbCatalogKindColumn => 675,
            Str::DbCatalogKindIndex => 676,
            Str::DbCatalogKindConstraint => 677,
            Str::DbCatalogKindNamespace => 678,
            Str::DbCatalogKindKey => 679,
            Str::DbCatalogKindObject => 680,
            Str::DbSavedQueryScope => 681,
            Str::DbFieldUri => 682,
            Str::DbFieldUriPlaceholder => 683,
            Str::DbFillFromUri => 684,
            Str::DbUriFilled => 685,
            Str::DbUriIgnored(_) => 686,
            Str::DbUriTlsNotApplied => 687,
            Str::DbUriEmpty => 688,
            Str::DbUriNoScheme => 689,
            Str::DbUriUnknownScheme(_) => 690,
            Str::DbUriInvalidPort(_) => 691,
            Str::DbUriMissingFile => 692,
            Str::DbUriInvalidEscape => 693,
            Str::QuickNavigation => 694,
            Str::QuickNavEnabled => 695,
            Str::QuickNavEnabledDescription => 696,
            Str::QuickNavCurlPattern => 697,
            Str::QuickNavDatabasePattern => 698,
            Str::QuickNavJwtPattern => 699,
            Str::QuickNavJsonPattern => 700,
            Str::QuickNavBase64Pattern => 701,
            Str::QuickNavGateDescription => 702,
            Str::QuickNavShapeDescription => 703,
            Str::QuickNavPatternInvalid(_) => 704,
            Str::QuickNavPatternTooLong { .. } => 705,
            Str::QuickNavStorageProblem => 706,
            Str::QuickNavStoreError(_) => 707,
            Str::QuickNavStoreMissingVersion => 708,
            Str::QuickNavStoreUnsupportedVersion { .. } => 709,
            Str::QuickNavOpenedConnection(_) => 710,
            Str::QuickNavKeptStoredPassword(_) => 711,
            Str::QuickNavCreatedConnection(_) => 712,
            Str::QuickNavConnectionsLoading => 713,
            Str::CleanerTitle => 714,
            Str::CleanerUnsupportedPlatform => 715,
            Str::CleanerScan => 716,
            Str::CleanerCancelScan => 717,
            Str::CleanerNoResultsYet => 718,
            Str::CleanerStatusScanning => 719,
            Str::CleanerStatusCancelling => 720,
            Str::CleanerStatusPartial => 721,
            Str::CleanerStatusCompleted => 722,
            Str::CleanerStatusCleaning => 723,
            Str::CleanerStatusFailed => 724,
            Str::CleanerSectionCleanup => 725,
            Str::CleanerSectionApplications => 726,
            Str::CleanerSectionAdvanced => 727,
            Str::CleanerCategorySystemJunk => 728,
            Str::CleanerCategoryUserCache => 729,
            Str::CleanerCategoryMailFiles => 730,
            Str::CleanerCategoryTrashBins => 731,
            Str::CleanerCategoryLargeOldFiles => 732,
            Str::CleanerCategoryInstalledApps => 733,
            Str::CleanerCategoryOrphanedFiles => 734,
            Str::CleanerCategoryAiApps => 735,
            Str::CleanerCategoryXcodeJunk => 736,
            Str::CleanerCategoryHomebrewCache => 737,
            Str::CleanerCategoryNodeToolingCache => 738,
            Str::CleanerCategoryDockerCache => 739,
            Str::CleanerCategoryUniversalBinaries => 740,
            Str::CleanerCategoryLanguageFiles => 741,
            Str::CleanerWarnings => 742,
            Str::CleanerPath => 743,
            Str::CleanerExplanation => 744,
            Str::CleanerCopyPath => 745,
            Str::CleanerRevealInFinder => 746,
            Str::CleanerSelectItem => 747,
            Str::CleanerDeselectItem => 748,
            Str::CleanerSelectSafeItems => 749,
            Str::CleanerCleanSelected => 750,
            Str::CleanerCleanupReport => 751,
            Str::CleanerCleanupConfirmTitle => 752,
            Str::CleanerCleanupConfirmMessage { .. } => 753,
            Str::CleanerCleanupSuccessCount(_) => 754,
            Str::CleanerCleanupFailureCount(_) => 755,
            Str::CleanerPermissionTitle => 756,
            Str::CleanerPermissionExplanation => 757,
            Str::CleanerPermissionOpenSettings => 758,
            Str::CleanerPartialPermissionDenied => 759,
            Str::CleanerPartialRootUnavailable => 760,
            Str::CleanerPartialCancelled => 761,
            Str::CleanerPartialUnsupported => 762,
            Str::CleanerBeginUninstallReview => 763,
            Str::CleanerUninstallReviewTitle { .. } => 764,
            Str::CleanerUninstallLoading => 765,
            Str::CleanerUninstallRefusedProtected => 766,
            Str::CleanerUninstallRefusedNotApplication => 767,
            Str::CleanerUninstallRelatedFilesHeader => 768,
            Str::CleanerUninstallNoRelatedFiles => 769,
            Str::CleanerUninstallDestinationNote => 770,
            Str::CleanerUninstallScanOnlyBadge => 771,
            Str::CleanerUninstallMoveToTrash => 772,
            Str::CleanerUninstallClose => 773,
            Str::CleanerConfidenceConfirmed => 774,
            Str::CleanerConfidenceHigh => 775,
            Str::CleanerConfidenceMedium => 776,
            Str::CleanerConfidenceLow => 777,
            Str::CleanerConfidenceSharedOrUnsafe => 778,
            Str::CleanerKeepItem => 779,
            Str::CleanerIgnoreStoreError(_) => 780,
            Str::CleanerIgnoreStoreMissingVersion => 781,
            Str::CleanerIgnoreStoreUnsupportedVersion { .. } => 782,
            Str::CleanerDockerCleanupConfirmTitle => 783,
            Str::CleanerDockerCleanupConfirmMessage { .. } => 784,
            Str::SessionStorageProblem => 785,
            Str::SessionStoreError(_) => 786,
            Str::SessionStoreMissingVersion => 787,
            Str::SessionStoreUnsupportedVersion { .. } => 788,
            Str::Features => 789,
            Str::FeaturesDescription => 790,
            Str::FeatureShowInSidebar => 791,
            Str::FeatureDragToReorder => 792,
            Str::FeatureMoveUp => 793,
            Str::FeatureMoveDown => 794,
            Str::FeatureLastVisibleTool => 795,
            Str::TrayOpenDodo => 796,
            Str::TrayKeyboardInput => 797,
            Str::TrayQuitDodo => 798,
            Str::InputMethod => 799,
            Str::InputMethodDescription => 800,
            Str::InputMethodInstall => 801,
            Str::InputMethodReinstall => 802,
            Str::InputMethodInstalling => 803,
            Str::InputMethodInstalled => 804,
            Str::InputMethodInstalledNotActive(_) => 805,
            Str::InputMethodNoBundle => 806,
            Str::InputMethodCopyFailed(_) => 807,
            Str::InputMethodInvalidSignature(_) => 808,
            Str::InputMethodNeverAppeared(_) => 809,
            Str::InputMethodStatus => 810,
            Str::InputMethodNotInstalled => 811,
            Str::InputMethodRunning(_) => 812,
            Str::InputMethodInstalledIdle => 813,
            Str::InputMethodSettingsPending => 814,
            Str::InputMethodStorageProblem => 815,
            Str::InputMethodStoreError(_) => 816,
            Str::InputMethodStoreMissingVersion => 817,
            Str::InputMethodStoreUnsupportedVersion { .. } => 818,
            Str::InputMethodScheme => 819,
            Str::InputMethodSchemeDescription => 820,
            Str::InputMethodTelex => 821,
            Str::InputMethodVni => 822,
            Str::InputMethodTonePlacement => 823,
            Str::InputMethodTonePlacementDescription => 824,
            Str::InputMethodToneModern => 825,
            Str::InputMethodToneTraditional => 826,
            Str::InputMethodSpellCheck => 827,
            Str::InputMethodSpellCheckDescription => 828,
            Str::InputMethodBracketShortcuts => 829,
            Str::InputMethodBracketShortcutsDescription => 830,
            Str::InputMethodBackend => 831,
            Str::InputMethodBackendDescription => 832,
            Str::InputMethodNative => 833,
            Str::InputMethodEventTap => 834,
            Str::InputMethodEventTapStatus => 835,
            Str::InputMethodEventTapInactive => 836,
            Str::InputMethodEventTapWaitingForNative => 837,
            Str::InputMethodEventTapNeedsAccessibility => 838,
            Str::InputMethodEventTapRunning => 839,
            Str::InputMethodEventTapFailed => 840,
            Str::InputMethodWindowsDescription => 841,
            Str::InputMethodWindowsLanguageDescription => 842,
            Str::InputMethodNativeTsf => 843,
            Str::InputMethodWindowsTsfStatus => 844,
            Str::InputMethodWindowsTsfNotInstalled => 845,
            Str::InputMethodWindowsTsfInstalled => 846,
            Str::InputMethodWindowsTsfRemoved => 847,
            Str::InputMethodWindowsTsfNoDll => 848,
            Str::InputMethodWindowsTsfRegisterFailed(_) => 849,
            Str::InputMethodWindowsTsfUnregisterFailed(_) => 850,
            Str::InputMethodKeyboardHook => 851,
            Str::InputMethodKeyboardHookStatus => 852,
            Str::InputMethodKeyboardHookInactive => 853,
            Str::InputMethodKeyboardHookRunning => 854,
            Str::InputMethodKeyboardHookFailed => 855,
            Str::InputMethodUninstall => 856,
            Str::InputMethodUninstalling => 857,
            Str::CleanerRevealInExplorer => 858,
            Str::CleanerRevealInFileManager => 859,
            Str::CleanerMoreActions => 860,
            Str::CleanerColumnName => 861,
            Str::CleanerColumnRisk => 862,
            Str::CleanerColumnSize => 863,
            Str::CleanerColumnActions => 864,
            Str::CleanerRiskSafe => 865,
            Str::CleanerRiskReview => 866,
            Str::CleanerRiskUserData => 867,
            Str::CleanerRiskAppChange => 868,
            Str::CleanerRiskProtected => 869,
            Str::Runtimes => 870,
            Str::RuntimesDescription => 871,
            Str::RuntimePodmanMachine => 872,
            Str::RuntimeKubernetes => 873,
            Str::RuntimeContainerd => 874,
            Str::RuntimeStatusRunning => 875,
            Str::RuntimeStatusStopped => 876,
            Str::RuntimeStatusNotInstalled => 877,
            Str::RuntimeStatusUnsupported => 878,
            Str::RuntimeStatusUnknown => 879,
            Str::RuntimeManagedExternally => 880,
            Str::RuntimeStarting => 881,
            Str::RuntimeStopping => 882,
            Str::RuntimeBinaryNotFound => 883,
            Str::RuntimeActionUnsupported => 884,
            Str::CleanerScanDescription => 885,
            Str::CleanerEntriesScannedCount(_) => 886,
            Str::CleanerBytesDiscovered(_) => 887,
            Str::CleanerReclaimableAmount(_) => 888,
            Str::CleanerItemsFound(_) => 889,
            Str::CleanerSafeItemsCount(_) => 890,
            Str::CleanerWarningCount(_) => 891,
            Str::CleanerSelectedSummary { .. } => 892,
            Str::CleanerCleanCount { .. } => 893,
            Str::CleanerScanWarningsSummary(_) => 894,
            Str::CleanerScanWarningsShowDetails => 895,
            Str::CleanerScanWarningsHideDetails => 896,
            Str::CleanerRescan => 897,
            Str::CleanerSelectAll => 898,
            Str::CleanerDeselectAll => 899,
            Str::CleanerPermissionNotNow => 900,
            Str::CleanerStatusCompletedWithWarnings => 901,
            Str::CleanerStatusCancelled => 902,
            Str::StartWithOs => 903,
            Str::StartWithOsDescription => 904,
        }
    }

    #[test]
    fn every_str_variant_has_a_sample() {
        for (index, sample) in samples().iter().enumerate() {
            assert_eq!(
                position(&sample.str),
                index,
                "samples() is out of step with position() at index {index}: add the \
                 missing entry rather than renumbering position()"
            );
        }
    }

    #[test]
    fn every_language_renders_every_string() {
        for sample in samples() {
            let english = sample.str.clone().text(Language::English).into_owned();

            for language in Language::ALL {
                let text = sample.str.clone().text(language).into_owned();
                let code = language.code();

                assert!(
                    !text.trim().is_empty(),
                    "{code} translation of \"{english}\" is empty"
                );
                for part in sample.parts {
                    assert!(
                        text.contains(part),
                        "{code} translation of \"{english}\" dropped the runtime value \
                         `{part}`; it rendered as \"{text}\""
                    );
                }
            }
        }
    }

    #[test]
    fn translations_match_their_declared_kind() {
        for sample in samples() {
            let english = sample.str.clone().text(Language::English).into_owned();

            for language in Language::ALL {
                if language == Language::English {
                    continue;
                }
                let text = sample.str.clone().text(language).into_owned();
                let code = language.code();

                match sample.expect {
                    Expect::Translated => assert_ne!(
                        text, english,
                        "{code} still shows the English text for \"{english}\" — translate it, \
                         or declare it with term() if it really is the same word"
                    ),
                    Expect::SameEverywhere => assert_eq!(
                        text, english,
                        "\"{english}\" is declared as a term of art that is identical in every \
                         language, but {code} differs — declare it with plain() instead"
                    ),
                }
            }
        }
    }

    #[test]
    fn every_language_names_every_jwt_part() {
        for part in [JwtPart::Header, JwtPart::Payload] {
            for language in Language::ALL {
                assert!(
                    !part.name(language).trim().is_empty(),
                    "{} has no name for a JWT part",
                    language.code()
                );
            }
        }
    }
}
