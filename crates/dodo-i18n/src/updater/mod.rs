//! The in-app updater.
//!
//! `en` and `vi` each render every variant below; the compiler names any
//! string a language has not been given.

use crate::Str;

pub(crate) mod en;
pub(crate) mod vi;

#[cfg(test)]
pub(crate) mod samples;

/// The strings this area owns.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Text {
    SoftwareUpdate,
    Checking,
    UpToDate,
    /// The version this binary is, shown under every verdict so the user can
    /// tell what they are comparing against.
    CurrentVersion(String),
    AvailableHeadline(String),
    /// The manifest's `published_at`, verbatim. An ISO-8601 UTC timestamp is
    /// the same characters in every language.
    Published(String),
    DownloadSize(String),
    ReleaseNotes,
    DownloadAction,
    /// Nothing is downloaded until the user presses the action above, so this
    /// only ever appears after an explicit agreement.
    DownloadProgress {
        done: String,
        total: String,
        percent: u8,
    },
    Verifying,
    Installing,
    InstalledHeadline(String),
    RestartNow,
    Later,
    SkipVersion,
    Cancel,
    Retry,
    CheckAutomatically,
    /// The install could not be done here. **Not a failure** — the archive is
    /// downloaded and verified, and this says where it is.
    ManualInstall(String),
    ManualNotABundle,
    ManualNotWritable,
    ManualReadOnly,
    FailedHeadline,
    /// `reqwest`'s own message is third-party English, kept verbatim inside a
    /// translated frame — the convention this module records.
    ErrorNetwork(String),
    ErrorManifestMalformed(String),
    ErrorManifestMissingVersion,
    ErrorManifestUnsupportedVersion {
        found: u64,
        supported: u32,
    },
    ErrorManifestUnreadableVersion(String),
    /// Frames one of the three reasons below, which are written as sentence
    /// fragments so the whole message reads as one sentence in each language.
    /// Boxed because a `Str` cannot contain itself by value.
    ErrorManifestInvalidFile {
        platform: String,
        detail: Box<Str>,
    },
    ErrorManifestBadDigest(String),
    ErrorManifestZeroSize,
    ErrorManifestInsecureUrl(String),
    ErrorPlatformMissing(String),
    ErrorDownload(String),
    ErrorChecksum {
        expected: String,
        actual: String,
    },
    ErrorSize {
        expected: u64,
        actual: u64,
    },
    ErrorInstall(String),
    ErrorIo(String),
}
