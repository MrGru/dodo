//! The API Explorer's variables and environments.
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
    // API Explorer — key/value tables.
    ColumnKey,
    ColumnValue,
    DeleteRow,
    NamePlaceholder,

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
    KeyPlaceholder,
    ValuePlaceholder,
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
    /// The environments file could not be read or written. `detail` is
    /// third-party English, kept verbatim inside a translated frame.
    StoreError(String),
    StoreMissingVersion,
    /// "This environments file was written by a newer dodo (schema {found};
    /// this build reads {supported})."
    StoreUnsupportedVersion {
        found: u64,
        supported: u32,
    },
    /// An environment file could not be imported. `detail` as above.
    EnvironmentImportError(String),

    // API Explorer — the script engine.
    /// The precedence layer `pm.variables.set` writes into.
    ScriptVariables,
}
