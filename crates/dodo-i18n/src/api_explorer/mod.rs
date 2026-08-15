//! The API Explorer's request side: the bar, the tabs, the key/value
//! tables, the body and auth editors, the send failures and the code generator.
//!
//! `en` and `vi` each render every variant below; the compiler names any
//! string a language has not been given.

pub(crate) mod en;
pub(crate) mod vi;

#[cfg(test)]
pub(crate) mod samples;

/// The strings this area owns.
#[derive(Clone, Debug, PartialEq, Eq)]
#[allow(
    clippy::enum_variant_names,
    reason = "`BodyTypeText`, `BodyNotText` and `FieldKindText` end in the \
              enum's name by coincidence: `Text` here is the catalogue, and \
              there is the media type. Remove this if they are renamed."
)]
pub enum Text {
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
    Add,
    AddParameter,
    AddHeader,
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
    PreRequestScriptPlaceholder,
    PostResponseScriptPlaceholder,

    // API Explorer — request failures.
    /// The URL parser's message is third-party English and is kept verbatim.
    InvalidUrl(String),
    UnsupportedScheme(String),
    InvalidHeader(String),
    Timeout(u64),
    DnsFailure(String),
    /// The underlying error chain is third-party English and is kept verbatim.
    ConnectFailure(String),
    TlsFailure(String),
    BodyNotText(String),
    Unexpected(String),
    SearchCollectionsPlaceholder,
    DefaultCollectionName,
    DefaultFolderName,
    SaveToCollectionNote,

    // API Explorer — key/value table refinements (phase 4).
    ToggleAllRows,
    EditModeTable,
    EditModeBulk,
    BulkEditPlaceholder,

    // API Explorer (round 7) — typed form rows, the binary body, and the tab
    // title.
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
    FileUnreadable {
        path: String,
        detail: String,
    },
    /// "{path} is larger than the {limit_mb} MB this build will send."
    FileTooLarge {
        path: String,
        limit_mb: u64,
    },
    /// "No variable named {name} is defined in this environment." — the send
    /// failure. Its own wording rather than a shared stem, because it is read
    /// in an error banner rather than beside the URL.
    UnresolvedVariable(String),
    /// "{name} refers to itself."
    RecursiveVariable(String),
    /// "Pre-request script finished in {millis} ms."
    ScriptFinished {
        millis: u64,
    },
    /// "The script wrote {count} variables."
    ScriptWroteVariables(usize),
    /// "The script asked for method {method}, which dodo does not support."
    ScriptUnknownMethod(String),
    /// "{count} lines from this run were dropped."
    ConsoleRunTruncated(usize),

    // API Explorer — the script editors' syntax check.
    /// The wavy-underline message inside the editor. `detail` is QuickJS's own
    /// wording and stays English inside the translated frame.
    ScriptSyntaxError(String),

    // API Explorer — the Tests tab.
    /// The Console line the post-response hook leaves, matching
    /// [`api_scripts::Text::Finished`](crate::api_scripts::Text::Finished) for the other hook.
    TestScriptFinished {
        millis: u64,
    },

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
}
