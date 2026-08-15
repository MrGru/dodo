//! The English column of the API Explorer's request side.

use std::borrow::Cow;

use super::Text;

pub(crate) fn text(text: Text) -> Cow<'static, str> {
    match text {
        Text::UrlPlaceholder => "Enter a URL, then press Send.".into(),
        Text::Send => "Send".into(),
        Text::NewRequest => "New request".into(),
        Text::CloseRequest => "Close request".into(),
        Text::NameRequest => "Name this request".into(),
        Text::NameRequestPlaceholder => "Request name".into(),
        Text::SaveName => "Save name".into(),
        Text::GenerateCode => "Generate code".into(),
        Text::RequestTabParams => "Params".into(),
        Text::RequestTabHeaders => "Headers".into(),
        Text::RequestTabBody => "Body".into(),
        Text::RequestTabAuth => "Auth".into(),
        Text::RequestTabScripts => "Scripts".into(),
        Text::Add => "Add".into(),
        Text::AddParameter => "Add parameter".into(),
        Text::AddHeader => "Add header".into(),
        Text::NoActiveParams => "No active params".into(),
        Text::ActiveParams(count) => format!("{count} active params").into(),
        Text::NoActiveHeaders => "No active headers".into(),
        Text::ActiveHeaders(count) => format!("{count} active headers").into(),
        Text::ParamKeyPlaceholder => "Parameter".into(),
        Text::ParamValuePlaceholder => "Value".into(),
        Text::HeaderKeyPlaceholder => "Header".into(),
        Text::HeaderValuePlaceholder => "Value".into(),
        Text::ColumnDescription => "DESCRIPTION".into(),
        Text::DescriptionPlaceholder => "Description".into(),
        Text::DuplicateRow => "Duplicate row".into(),
        Text::MoveRowUp => "Move row up".into(),
        Text::MoveRowDown => "Move row down".into(),
        Text::AddField => "Add field".into(),
        Text::NoActiveFields => "No active fields".into(),
        Text::ActiveFields(count) => format!("{count} active fields").into(),
        Text::FieldKeyPlaceholder => "Field".into(),
        Text::FieldValuePlaceholder => "Value".into(),
        Text::BodyTypeNone => "None".into(),
        Text::BodyTypeJson => "JSON".into(),
        Text::BodyTypeText => "Raw text".into(),
        Text::BodyTypeXml => "XML".into(),
        Text::BodyTypeHtml => "HTML".into(),
        Text::BodyTypeFormData => "Form data".into(),
        Text::BodyTypeUrlEncoded => "x-www-form-urlencoded".into(),
        Text::BodyTypeBinary => "Binary".into(),
        Text::BodyPlaceholder => "Type or paste the request body here.".into(),
        Text::NoBodyTitle => "No body".into(),
        Text::NoBodyHint => {
            "This request is sent without a body. Choose a type above to add one.".into()
        }
        Text::BinaryBodyHint => "Pick a file to send as the raw request body.".into(),
        Text::MethodSendsNoBody(method) => {
            format!("{method} requests are sent without a body.").into()
        }
        Text::AuthTypeLabel => "Auth type".into(),
        Text::AuthTypeNone => "No auth".into(),
        Text::AuthTypeBearer => "Bearer token".into(),
        Text::AuthTypeBasic => "Basic auth".into(),
        Text::AuthTypeApiKey => "API key".into(),
        Text::AuthTypeOAuth2 => "OAuth 2.0".into(),
        Text::OAuth2Later => {
            "OAuth 2.0 needs a browser redirect and a token store; it arrives in a later step."
                .into()
        }
        Text::NoAuthTitle => "No authorization".into(),
        Text::NoAuthHint => {
            "This request carries no Authorization header. Choose a scheme above to add one.".into()
        }
        Text::AuthTokenLabel => "Token".into(),
        Text::AuthTokenPlaceholder => "Paste the bearer token".into(),
        Text::AuthUsernameLabel => "Username".into(),
        Text::AuthUsernamePlaceholder => "Your username".into(),
        Text::AuthPasswordLabel => "Password".into(),
        Text::AuthPasswordPlaceholder => "Your password".into(),
        Text::ApiKeyNameLabel => "Key".into(),
        Text::ApiKeyNamePlaceholder => "For example X-Api-Key".into(),
        Text::ApiKeyValueLabel => "Value".into(),
        Text::ApiKeyValuePlaceholder => "The key's value".into(),
        Text::ApiKeySendAs => "Send as".into(),
        Text::ApiKeyInHeader => "Header".into(),
        Text::ApiKeyInQuery => "Query parameter".into(),
        Text::PreRequestScriptPlaceholder => "Runs before the request is sent.".into(),
        Text::PostResponseScriptPlaceholder => "Runs after the response arrives.".into(),
        Text::InvalidUrl(detail) => {
            if detail.is_empty() {
                "Enter a URL before sending.".into()
            } else {
                format!("That URL could not be read: {detail}").into()
            }
        }
        Text::UnsupportedScheme(scheme) => {
            format!("This tool can only fetch http and https, not {scheme}.").into()
        }
        Text::InvalidHeader(name) => {
            format!("The header \"{name}\" cannot be sent as written.").into()
        }
        Text::Timeout(seconds) => format!("No response within {seconds} seconds.").into(),
        Text::DnsFailure(host) => format!("The address \"{host}\" could not be found.").into(),
        Text::ConnectFailure(detail) => format!("Could not connect: {detail}").into(),
        Text::TlsFailure(detail) => format!("The secure connection was refused: {detail}").into(),
        Text::BodyNotText(detail) => {
            format!("The response is not text this viewer can show ({detail}).").into()
        }
        Text::Unexpected(detail) => format!("The request failed: {detail}").into(),
        Text::SearchCollectionsPlaceholder => "Search collections".into(),
        Text::DefaultCollectionName => "New collection".into(),
        Text::DefaultFolderName => "New folder".into(),
        Text::SaveToCollectionNote => "Saved into your collections.".into(),
        Text::ToggleAllRows => "Enable or disable all rows".into(),
        Text::EditModeTable => "Table".into(),
        Text::EditModeBulk => "Bulk edit".into(),
        Text::BulkEditPlaceholder => {
            "One entry per line as Key: Value. Begin a line with # to disable it.".into()
        }
        Text::UntitledRequest => "Untitled".into(),
        Text::ColumnType => "TYPE".into(),
        Text::FieldKindText => "Text".into(),
        Text::FieldKindFile => "File".into(),
        Text::ChooseFile => "Choose file…".into(),
        Text::ReplaceFile => "Choose another file".into(),
        Text::ClearFile => "Remove the chosen file".into(),
        Text::NoFileSelected => "No file chosen".into(),
        Text::IncompleteFileFields(count) => if count == 1 {
            "1 file field has no file chosen and will not be sent.".to_string()
        } else {
            format!("{count} file fields have no file chosen and will not be sent.")
        }
        .into(),
        Text::FileUnreadable { path, detail } => format!("Could not read {path}: {detail}").into(),
        Text::FileTooLarge { path, limit_mb } => {
            format!("{path} is larger than the {limit_mb} MB this build can send.").into()
        }
        Text::UnresolvedVariable(name) => format!(
            "No variable named {name} is defined. Add it to an environment, or to \
                         the collection variables, then send again."
        )
        .into(),
        Text::RecursiveVariable(name) => {
            format!("The variable {name} refers back to itself, so it cannot be resolved.").into()
        }
        Text::ScriptFinished { millis } => {
            format!("Pre-request script finished in {millis} ms.").into()
        }
        Text::ScriptWroteVariables(count) => format!("The script wrote {count} variables.").into(),
        Text::ScriptUnknownMethod(method) => format!(
            "The script asked for method {method}, which dodo has no option for; the \
                 method in the editor was kept."
        )
        .into(),
        Text::ConsoleRunTruncated(count) => {
            format!("{count} lines from this run were dropped.").into()
        }
        Text::ScriptSyntaxError(detail) => format!("Syntax error: {detail}").into(),
        Text::TestScriptFinished { millis } => {
            format!("Post-response script finished in {millis} ms.").into()
        }
        Text::CodeTargetCurl => "cURL".into(),
        Text::CodeTargetFetch => "fetch".into(),
        Text::CodeTargetAxios => "axios".into(),
        Text::CodeTargetXhr => "XMLHttpRequest".into(),
        Text::GenerateCodeCarriesValues => {
            "This code carries the request's real values, including any token or \
                 password it uses."
                .into()
        }
        Text::GenerateCodeSecretsWithheld(names) => format!(
            "Left as {{{{placeholders}}}}: {names}. Everything else — including a token \
                 or password typed into this request — is in the code below."
        )
        .into(),
        Text::GenerateCodeSecretsRevealed => {
            "This code contains the real value of every secret variable it uses, in \
                 plain text. Anything you paste it into keeps that value."
                .into()
        }
        Text::GenerateCodeRevealSecrets => "Resolve secret variables".into(),
    }
}
