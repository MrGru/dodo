//! The English column of the in-app updater.

use std::borrow::Cow;

use crate::Language;

use super::Text;

pub(crate) fn text(text: Text) -> Cow<'static, str> {
    match text {
        Text::SoftwareUpdate => "Software update".into(),
        Text::Checking => "Checking for updates…".into(),
        Text::UpToDate => "dodo is up to date.".into(),
        Text::CurrentVersion(version) => format!("You are running version {version}.").into(),
        Text::AvailableHeadline(version) => format!("Version {version} is available.").into(),
        Text::Published(when) => format!("Published {when}").into(),
        Text::DownloadSize(size) => format!("Download size {size}").into(),
        Text::ReleaseNotes => "Release notes".into(),
        Text::DownloadAction => "Download and install".into(),
        Text::DownloadProgress {
            done,
            total,
            percent,
        } => format!("Downloading… {done} of {total} ({percent}%)").into(),
        Text::Verifying => "Verifying the download…".into(),
        Text::Installing => "Installing…".into(),
        Text::InstalledHeadline(version) => format!("Version {version} is installed.").into(),
        Text::RestartNow => "Restart now".into(),
        Text::Later => "Later".into(),
        Text::SkipVersion => "Skip this version".into(),
        Text::Cancel => "Cancel".into(),
        Text::Retry => "Try again".into(),
        Text::CheckAutomatically => "Check for updates automatically".into(),
        Text::ManualInstall(path) => format!(
            "The update was downloaded and verified, but dodo cannot replace itself where \
                 it is installed. The archive is at {path}."
        )
        .into(),
        Text::ManualNotABundle => {
            "dodo is running as a plain executable rather than from an app bundle.".into()
        }
        Text::ManualNotWritable => "The folder dodo is installed in cannot be written to.".into(),
        Text::ManualReadOnly => "dodo is running from a read-only location.".into(),
        Text::FailedHeadline => "The update could not be completed.".into(),
        Text::ErrorNetwork(detail) => format!("Could not reach the update server: {detail}").into(),
        Text::ErrorManifestMalformed(detail) => {
            format!("The update manifest could not be read: {detail}").into()
        }
        Text::ErrorManifestMissingVersion => {
            "The update manifest carries no version, so dodo cannot tell how to read it.".into()
        }
        Text::ErrorManifestUnsupportedVersion { found, supported } => format!(
            "The update manifest is version {found}; this dodo understands version \
                 {supported}. Update dodo by hand."
        )
        .into(),
        Text::ErrorManifestUnreadableVersion(text) => {
            format!("The update manifest names a version dodo cannot read: {text}").into()
        }
        Text::ErrorManifestInvalidFile { platform, detail } => format!(
            "The update manifest's {platform} entry is unusable: {}",
            detail.text(Language::English)
        )
        .into(),
        Text::ErrorManifestBadDigest(digest) => {
            format!("{digest} is not a SHA-256 checksum").into()
        }
        Text::ErrorManifestZeroSize => "the download size is zero".into(),
        Text::ErrorManifestInsecureUrl(url) => {
            format!("the download address does not use https: {url}").into()
        }
        Text::ErrorPlatformMissing(key) => {
            format!("This release publishes no download for {key}.").into()
        }
        Text::ErrorDownload(detail) => format!("The download failed: {detail}").into(),
        Text::ErrorChecksum { expected, actual } => format!(
            "The download does not match the checksum this release published — expected \
                 {expected}, got {actual}. It has been discarded and nothing was installed."
        )
        .into(),
        Text::ErrorSize { expected, actual } => format!(
            "The download is {actual} bytes; this release says {expected}. It has been \
                 discarded and nothing was installed."
        )
        .into(),
        Text::ErrorInstall(detail) => format!("The update could not be installed: {detail}").into(),
        Text::ErrorIo(detail) => format!("A file could not be written: {detail}").into(),
    }
}
