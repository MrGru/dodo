//! The English column of the Database Explorer's connection form.

use std::borrow::Cow;

use super::Text;

pub(crate) fn text(text: Text) -> Cow<'static, str> {
    match text {
        Text::NewConnection => "New connection".into(),
        Text::EditConnectionTitle => "Edit connection".into(),
        Text::Cancel => "Cancel".into(),
        Text::Save => "Save".into(),
        Text::FieldName => "Name".into(),
        Text::FieldNamePlaceholder => "Optional".into(),
        Text::FieldEngine => "Type".into(),
        Text::FieldHost => "Host".into(),
        Text::FieldPort => "Port".into(),
        Text::FieldDatabase => "Database".into(),
        Text::FieldUser => "User".into(),
        Text::FieldUrl => "URL".into(),
        Text::FieldPassword => "Password".into(),
        Text::FieldFile => "File".into(),
        Text::FieldFilePlaceholder => "Path to the database file".into(),
        Text::FieldSsl => "TLS".into(),
        Text::SslDisable => "Disable".into(),
        Text::SslPrefer => "Prefer".into(),
        Text::SslRequire => "Require".into(),
        Text::PasswordStorageNotice => {
            "Saved passwords are stored unencrypted in dodo's data folder, like the API \
                 Explorer's secret variables. Anyone who can read that folder can read them."
                .into()
        }
        Text::RevealPassword => "Show password".into(),
        Text::HidePassword => "Hide password".into(),
        Text::TestConnection => "Test connection".into(),
        Text::Testing => "Testing…".into(),
        Text::TestSucceeded => "The connection works.".into(),
        Text::ProfileHostMissing => "Enter a host.".into(),
        Text::ProfilePortMissing => "Enter a port.".into(),
        Text::ProfileDatabaseMissing => "Enter a database name.".into(),
        Text::ProfileFileMissing => "Choose a database file.".into(),
        Text::ConnectionStoreError(detail) => {
            format!("Connections could not be saved: {detail}").into()
        }
        Text::ConnectionStoreMissingVersion => {
            "The saved connections file carries no schema version, so it cannot be read.".into()
        }
        Text::ConnectionStoreUnsupportedVersion { found, supported } => format!(
            "The saved connections were written by a newer dodo (version {found}; this build \
                 understands {supported}). Update dodo to open them."
        )
        .into(),
        Text::ProfileRedisDatabaseInvalid => "Enter a non-negative logical database number.".into(),
        Text::FieldUri => "Connection URI".into(),
        Text::FieldUriPlaceholder => "postgresql://user:password@host:5432/database".into(),
        Text::FillFromUri => "Fill from URI".into(),
        Text::UriFilled => "Filled in from the URI. Check the fields before saving.".into(),
        Text::UriIgnored(parts) => format!("Read but not applied: {parts}").into(),
        Text::UriTlsNotApplied => {
            "This URI asks for TLS, but dodo's Redis client connects without it.".into()
        }
        Text::UriEmpty => "Paste a connection URI first.".into(),
        Text::UriNoScheme => {
            "This has no scheme, so there is nothing to say which database it is. Start it \
                 with postgresql://, mysql://, sqlite:// or redis://."
                .into()
        }
        Text::UriUnknownScheme(scheme) => {
            format!("dodo cannot connect to \"{scheme}\". Use postgresql, mysql, sqlite or redis.")
                .into()
        }
        Text::UriInvalidPort(port) => format!("\"{port}\" is not a port number.").into(),
        Text::UriMissingFile => "This URI names no database file.".into(),
        Text::UriInvalidEscape => "A percent-escape in this URI is not valid UTF-8.".into(),
    }
}
