//! One sample per [`Text`] variant, for the language tests.
//!
//! `samples!` also emits an exhaustive `match` over [`Text`], so a variant
//! with no entry here is a compile error.

use crate::i18n::tests::{DETAIL, NUMBER, NUMBER_TEXT, Sample, plain, with};

use super::Text;

samples! {
    plain SoftwareUpdate;
    plain Checking;
    plain UpToDate;
    with CurrentVersion(DETAIL.into()) [DETAIL];
    with AvailableHeadline(DETAIL.into()) [DETAIL];
    with Published(DETAIL.into()) [DETAIL];
    with DownloadSize(DETAIL.into()) [DETAIL];
    plain ReleaseNotes;
    plain DownloadAction;
    with DownloadProgress { done: DETAIL.into(), total: NUMBER_TEXT.into(), percent: 42 } [DETAIL, NUMBER_TEXT, "42"];
    plain Verifying;
    plain Installing;
    with InstalledHeadline(DETAIL.into()) [DETAIL];
    plain RestartNow;
    plain Later;
    plain SkipVersion;
    plain Cancel;
    plain Retry;
    plain CheckAutomatically;
    with ManualInstall(DETAIL.into()) [DETAIL];
    plain ManualNotABundle;
    plain ManualNotWritable;
    plain ManualReadOnly;
    plain FailedHeadline;
    with ErrorNetwork(DETAIL.into()) [DETAIL];
    with ErrorManifestMalformed(DETAIL.into()) [DETAIL];
    plain ErrorManifestMissingVersion;
    with ErrorManifestUnsupportedVersion { found: NUMBER as u64, supported: 77 } [NUMBER_TEXT, "77"];
    with ErrorManifestUnreadableVersion(DETAIL.into()) [DETAIL];
    with ErrorManifestInvalidFile { platform: DETAIL.into(), detail: Box::new(Text::ErrorManifestZeroSize.into()) } [DETAIL];
    with ErrorManifestBadDigest(DETAIL.into()) [DETAIL];
    plain ErrorManifestZeroSize;
    with ErrorManifestInsecureUrl(DETAIL.into()) [DETAIL];
    with ErrorPlatformMissing(DETAIL.into()) [DETAIL];
    with ErrorDownload(DETAIL.into()) [DETAIL];
    with ErrorChecksum { expected: DETAIL.into(), actual: NUMBER_TEXT.into() } [DETAIL, NUMBER_TEXT];
    with ErrorSize { expected: NUMBER as u64, actual: 77 } [NUMBER_TEXT, "77"];
    with ErrorInstall(DETAIL.into()) [DETAIL];
    with ErrorIo(DETAIL.into()) [DETAIL];
}
