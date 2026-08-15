//! One sample per [`Text`] variant, for the language tests.
//!
//! `samples!` also emits an exhaustive `match` over [`Text`], so a variant
//! with no entry here is a compile error.

use crate::tests::{DETAIL, NUMBER, NUMBER_TEXT, Sample, plain, term, with};

use super::Text;

samples! {
    plain ColumnKey;
    plain ColumnValue;
    plain DeleteRow;
    plain NamePlaceholder;
    plain NoEnvironment;
    plain SelectEnvironment;
    plain ManageEnvironments;
    plain Environments;
    plain NewEnvironment;
    plain DefaultEnvironmentName;
    plain EnvironmentCopySuffix;
    plain DuplicateEnvironment;
    plain DeleteEnvironment;
    plain ImportEnvironment;
    plain CollectionVariables;
    plain EnvironmentVariables;
    plain CollectionVariablesNote;
    plain NoEnvironmentsYet;
    plain NoEnvironmentsYetHint;
    plain ColumnSecret;
    plain AddVariable;
    plain NoActiveVariables;
    with ActiveVariables(NUMBER) [NUMBER_TEXT];
    term KeyPlaceholder;
    plain ValuePlaceholder;
    plain MarkSecret;
    plain RevealSecret;
    plain HideSecret;
    plain SecretStorageWarning;
    plain ResolvedUrlLabel;
    with UnresolvedVariablePreview(DETAIL.into()) [DETAIL];
    with ResolvesFrom { name: DETAIL.into(), scope: "<<scope-sentinel>>".into() } [DETAIL, "<<scope-sentinel>>"];
    with StoreError(DETAIL.into()) [DETAIL];
    plain StoreMissingVersion;
    with StoreUnsupportedVersion { found: NUMBER as u64, supported: 7 } [NUMBER_TEXT, "7"];
    with EnvironmentImportError(DETAIL.into()) [DETAIL];
    plain ScriptVariables;
}
