//! One sample per [`Text`] variant, for the language tests.
//!
//! `samples!` also emits an exhaustive `match` over [`Text`], so a variant
//! with no entry here is a compile error.

use crate::tests::{DETAIL, NUMBER, NUMBER_TEXT, Sample, plain, term, with};

use super::Text;

samples! {
    plain NewConnection;
    plain EditConnectionTitle;
    plain Cancel;
    plain Save;
    plain FieldName;
    plain FieldNamePlaceholder;
    plain FieldEngine;
    plain FieldHost;
    plain FieldPort;
    plain FieldDatabase;
    plain FieldUser;
    term FieldUrl;
    plain FieldPassword;
    plain FieldFile;
    plain FieldFilePlaceholder;
    term FieldSsl;
    plain SslDisable;
    plain SslPrefer;
    plain SslRequire;
    plain PasswordStorageNotice;
    plain RevealPassword;
    plain HidePassword;
    plain TestConnection;
    plain Testing;
    plain TestSucceeded;
    plain ProfileHostMissing;
    plain ProfilePortMissing;
    plain ProfileDatabaseMissing;
    plain ProfileFileMissing;
    with ConnectionStoreError(DETAIL.into()) [DETAIL];
    plain ConnectionStoreMissingVersion;
    with ConnectionStoreUnsupportedVersion { found: NUMBER as u64, supported: 77 } [NUMBER_TEXT, "77"];
    plain ProfileRedisDatabaseInvalid;
    plain FieldUri;
    term FieldUriPlaceholder;
    plain FillFromUri;
    plain UriFilled;
    with UriIgnored(DETAIL.into()) [DETAIL];
    plain UriTlsNotApplied;
    plain UriEmpty;
    plain UriNoScheme;
    with UriUnknownScheme(DETAIL.into()) [DETAIL];
    with UriInvalidPort(DETAIL.into()) [DETAIL];
    plain UriMissingFile;
    plain UriInvalidEscape;
}
