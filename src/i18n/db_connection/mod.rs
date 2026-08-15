//! The Database Explorer's connection form.
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
    NewConnection,
    EditConnectionTitle,
    Cancel,
    Save,
    FieldName,
    FieldNamePlaceholder,
    FieldEngine,
    FieldHost,
    FieldPort,
    FieldDatabase,
    FieldUser,
    FieldUrl,
    FieldPassword,
    FieldFile,
    FieldFilePlaceholder,
    FieldSsl,
    SslDisable,
    SslPrefer,
    SslRequire,
    /// Never hidden while a password field is on screen. See
    /// `database::models::connection`'s module doc for why the posture is
    /// plaintext-and-say-so rather than an OS keychain.
    PasswordStorageNotice,
    RevealPassword,
    HidePassword,
    TestConnection,
    Testing,
    TestSucceeded,
    ProfileHostMissing,
    ProfilePortMissing,
    ProfileDatabaseMissing,
    ProfileFileMissing,
    ConnectionStoreError(String),
    ConnectionStoreMissingVersion,
    ConnectionStoreUnsupportedVersion {
        found: u64,
        supported: u32,
    },

    // Database Explorer round 4: non-SQL console and keyspace paging.
    ProfileRedisDatabaseInvalid,

    // Database Explorer: filling the connection form from a pasted URI.
    FieldUri,
    FieldUriPlaceholder,
    FillFromUri,
    UriFilled,
    UriIgnored(String),
    UriTlsNotApplied,
    UriEmpty,
    UriNoScheme,
    UriUnknownScheme(String),
    UriInvalidPort(String),
    UriMissingFile,
    UriInvalidEscape,
}
