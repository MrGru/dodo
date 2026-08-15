//! The English column of the API Explorer's variables.

use std::borrow::Cow;

use super::Text;

pub(crate) fn text(text: Text) -> Cow<'static, str> {
    match text {
        Text::ColumnKey => "KEY".into(),
        Text::ColumnValue => "VALUE".into(),
        Text::DeleteRow => "Delete row".into(),
        Text::NamePlaceholder => "Name".into(),
        Text::NoEnvironment => "No environment".into(),
        Text::SelectEnvironment => "Choose the active environment".into(),
        Text::ManageEnvironments => "Manage environments…".into(),
        Text::Environments => "Environments".into(),
        Text::NewEnvironment => "New environment".into(),
        Text::DefaultEnvironmentName => "New environment".into(),
        Text::EnvironmentCopySuffix => "copy".into(),
        Text::DuplicateEnvironment => "Duplicate".into(),
        Text::DeleteEnvironment => "Delete".into(),
        Text::ImportEnvironment => "Import".into(),
        Text::CollectionVariables => "Collection variables".into(),
        Text::EnvironmentVariables => "Environment variables".into(),
        Text::CollectionVariablesNote => {
            "Shared by every request, whichever environment is active. An imported \
                 collection files its own variables here."
                .into()
        }
        Text::NoEnvironmentsYet => "No environments yet".into(),
        Text::NoEnvironmentsYetHint => {
            "Create one to keep a host, a token or an API key in a single place and refer \
                 to it as {{name}}."
                .into()
        }
        Text::ColumnSecret => "SECRET".into(),
        Text::AddVariable => "Add variable".into(),
        Text::NoActiveVariables => "No variables".into(),
        Text::ActiveVariables(count) => format!("{count} active").into(),
        Text::KeyPlaceholder => "baseUrl".into(),
        Text::ValuePlaceholder => "Value".into(),
        Text::MarkSecret => "Mask this value in the editor".into(),
        Text::RevealSecret => "Show the value".into(),
        Text::HideSecret => "Hide the value".into(),
        Text::SecretStorageWarning => {
            "Secret values are masked here, but they are saved to this machine in plain \
                 text, unencrypted, like every other variable."
                .into()
        }
        Text::ResolvedUrlLabel => "Resolves to".into(),
        Text::UnresolvedVariablePreview(name) => format!("{name} is not defined").into(),
        Text::ResolvesFrom { name, scope } => format!("{name} — from {scope}").into(),
        Text::StoreError(detail) => format!("Could not save or load environments: {detail}").into(),
        Text::StoreMissingVersion => {
            "This environments file carries no schema version, so it cannot be read safely.".into()
        }
        Text::StoreUnsupportedVersion { found, supported } => format!(
            "This environments file uses schema {found}; this build of dodo reads {supported}. \
                 Update dodo rather than risk misreading it."
        )
        .into(),
        Text::EnvironmentImportError(detail) => {
            format!("Could not import that environment: {detail}").into()
        }
        Text::ScriptVariables => "Script".into(),
    }
}
