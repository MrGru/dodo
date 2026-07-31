//! The `update.json` document, as the client reads it.
//!
//! `docs/release.md`'s "Automatic updates" is the authority on the shape;
//! `tools/update-manifest` writes it. This module is the reading half, and its
//! job is to be **strict where a mistake would be acted on** and forgiving
//! nowhere else.
//!
//! # `manifest_version` is read before anything else
//!
//! Exactly the pattern
//! [`consent_store::parse_document`](crate::api_explorer::services::consent_store::parse_document)
//! established and `AGENTS.md` names as the one to copy: pull the version out of
//! the raw JSON first, refuse anything higher than this build understands, and
//! only then deserialize. `serde_json::from_slice` straight into [`Manifest`]
//! would happily read a version-2 document with whatever fields still lined up
//! — which, here, would mean downloading and installing something on the
//! strength of a half-understood document.
//!
//! # What is validated, and why each one
//!
//! - **`sha256` must be 64 hex digits.** A truncated or placeholder digest is
//!   caught before 12 MB moves, not after.
//! - **`size` must be non-zero.** Zero would make every progress calculation a
//!   division by zero and would make a short read indistinguishable from a
//!   complete one.
//! - **`url` must be absolute `https://`.** The archive becomes an executable
//!   the user runs; fetching it over plaintext would put that in the hands of
//!   anyone on the path, and the digest that would catch it comes from the same
//!   connection. `http://` is refused here rather than at fetch time so it can
//!   never be reached by a code path that forgot to check.
//! - **`version` must parse.** The whole decision is a comparison against it.
//!
//! # An unrecognised platform key is *not* an error
//!
//! A key this build does not know — a platform added after it shipped — is kept
//! in the map and ignored by [`Manifest::file_for`]. Failing instead would mean
//! that adding a fifth platform to the release stops updates for everyone
//! already running an older dodo, which is the opposite of what the manifest is
//! for. The symmetric case — *our* platform missing from the map — is a real
//! failure and is reported by name, because "no entry for you" must never be
//! shown as "you are up to date" (the argument `docs/release.md` makes for
//! failing a release with a platform missing).

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::Value;

use crate::i18n::Str;
use crate::updater::models::platform::PlatformKey;
use crate::updater::models::sha256::is_hex_digest;
use crate::updater::models::version::{Channel, Version};

/// The highest `manifest_version` this build understands.
pub const SUPPORTED_MANIFEST_VERSION: u32 = 1;

/// One downloadable archive, as the manifest describes it.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct ManifestFile {
    pub url: String,
    /// Lowercase hex SHA-256 of the archive.
    pub sha256: String,
    pub size: u64,
    /// Reserved for a detached signature. **Nothing verifies it** — no signing
    /// exists, and this field is always `null` today. It is read so that a
    /// future build can start requiring it without a schema break; see
    /// `docs/release.md`. `#[serde(default)]` so an older manifest without the
    /// key still loads.
    #[serde(default)]
    pub signature: Option<String>,
}

/// The whole document.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct Manifest {
    pub manifest_version: u32,
    pub channel: Channel,
    pub version: String,
    pub notes: String,
    pub published_at: String,
    pub files: BTreeMap<String, ManifestFile>,
}

impl Manifest {
    /// The entry for one platform, or `None` when the manifest names no archive
    /// for it.
    pub fn file_for(&self, platform: PlatformKey) -> Option<&ManifestFile> {
        self.files.get(platform.key())
    }

    /// The offered version, already known to parse — [`parse`] refuses a
    /// manifest whose `version` does not.
    pub fn parsed_version(&self) -> Option<Version> {
        Version::parse(&self.version)
    }
}

/// Why a manifest could not be used.
///
/// Hand-rolled rather than `thiserror`: the accessor returns a [`Str`], the
/// convention `TransportError::message` and `DockerError::message` follow.
/// `thiserror`'s `Display` would produce an English `String`, which the i18n
/// guard exists to keep out of the UI.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ManifestError {
    /// Not JSON at all, or JSON missing a required field. The `serde_json`
    /// message is third-party English kept verbatim inside a translated frame.
    Malformed(String),
    /// No `manifest_version` key: nothing can be said about how to read it.
    MissingVersion,
    /// Written by a newer dodo. Refused rather than half-read.
    UnsupportedVersion { found: u64, supported: u32 },
    /// `version` is not a semantic version, so nothing can be compared to it.
    UnreadableVersion(String),
    /// A `files` entry this build would have acted on is not usable.
    InvalidFile { platform: String, detail: Str },
}

impl ManifestError {
    pub fn message(&self) -> Str {
        match self {
            ManifestError::Malformed(detail) => Str::UpdateErrorManifestMalformed(detail.clone()),
            ManifestError::MissingVersion => Str::UpdateErrorManifestMissingVersion,
            ManifestError::UnsupportedVersion { found, supported } => {
                Str::UpdateErrorManifestUnsupportedVersion {
                    found: *found,
                    supported: *supported,
                }
            }
            ManifestError::UnreadableVersion(text) => {
                Str::UpdateErrorManifestUnreadableVersion(text.clone())
            }
            ManifestError::InvalidFile { platform, detail } => {
                Str::UpdateErrorManifestInvalidFile {
                    platform: platform.clone(),
                    detail: Box::new(detail.clone()),
                }
            }
        }
    }
}

/// Reads a manifest, refusing anything this build cannot act on safely.
pub fn parse(bytes: &[u8]) -> Result<Manifest, ManifestError> {
    let value: Value =
        serde_json::from_slice(bytes).map_err(|err| ManifestError::Malformed(err.to_string()))?;

    // The version gate runs against the raw value, before any field of ours is
    // read out of it.
    let version = value
        .get("manifest_version")
        .and_then(Value::as_u64)
        .ok_or(ManifestError::MissingVersion)?;
    if version > u64::from(SUPPORTED_MANIFEST_VERSION) {
        return Err(ManifestError::UnsupportedVersion {
            found: version,
            supported: SUPPORTED_MANIFEST_VERSION,
        });
    }

    let manifest: Manifest =
        serde_json::from_value(value).map_err(|err| ManifestError::Malformed(err.to_string()))?;

    if Version::parse(&manifest.version).is_none() {
        return Err(ManifestError::UnreadableVersion(manifest.version.clone()));
    }

    // Only the entries this build could actually act on are validated: a
    // platform dodo does not run on may carry whatever it likes.
    for platform in PlatformKey::ALL {
        if let Some(file) = manifest.files.get(platform.key()) {
            validate_file(platform.key(), file)?;
        }
    }

    Ok(manifest)
}

fn validate_file(platform: &str, file: &ManifestFile) -> Result<(), ManifestError> {
    let invalid = |detail: Str| ManifestError::InvalidFile {
        platform: platform.to_owned(),
        detail,
    };

    if !is_hex_digest(&file.sha256) {
        return Err(invalid(Str::UpdateErrorManifestBadDigest(
            file.sha256.clone(),
        )));
    }
    if file.size == 0 {
        return Err(invalid(Str::UpdateErrorManifestZeroSize));
    }
    if !file.url.starts_with("https://") {
        return Err(invalid(Str::UpdateErrorManifestInsecureUrl(
            file.url.clone(),
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Manifest, ManifestError, parse};
    use crate::updater::models::platform::PlatformKey;
    use crate::updater::models::version::Channel;

    /// The real v0.1.6 manifest, trimmed to two platforms. Kept verbatim in
    /// shape so this test fails if the published document ever stops matching
    /// what the client reads.
    const VALID: &str = r###"{
      "manifest_version": 1,
      "channel": "stable",
      "version": "0.1.6",
      "notes": "## dodo v0.1.6\n\nrelease notes",
      "published_at": "2026-07-30T15:03:24Z",
      "files": {
        "macos-arm64": {
          "url": "https://github.com/MrGru/dodo/releases/download/v0.1.6/dodo-v0.1.6-macos-arm64-app.tar.gz",
          "sha256": "0a404f822c95e9f3f5a93abdfef8b005017c033eff4d72248f595798afaad1c3",
          "size": 11569143,
          "signature": null
        },
        "windows-x64": {
          "url": "https://github.com/MrGru/dodo/releases/download/v0.1.6/dodo-v0.1.6-windows-x64.zip",
          "sha256": "8ac817331932c2b6bbc40d7e770469955e757a184b896b1dcd1ec4a406b4485b",
          "size": 12332862,
          "signature": null
        }
      }
    }"###;

    fn valid() -> Manifest {
        parse(VALID.as_bytes()).expect("the published manifest shape must parse")
    }

    #[test]
    fn reads_a_published_manifest() {
        let manifest = valid();
        assert_eq!(manifest.manifest_version, 1);
        assert_eq!(manifest.channel, Channel::Stable);
        assert_eq!(manifest.version, "0.1.6");
        assert!(manifest.notes.contains("release notes"));
        assert_eq!(manifest.published_at, "2026-07-30T15:03:24Z");

        let file = manifest
            .file_for(PlatformKey::MacosArm64)
            .expect("macos-arm64 is present");
        assert_eq!(file.size, 11_569_143);
        assert!(file.url.ends_with("-app.tar.gz"), "{}", file.url);
        assert_eq!(file.signature, None, "nothing signs anything yet");
    }

    /// The macOS entry has to point at the `.app` bundle, not the bare binary:
    /// the installer swaps the bundle. `docs/release.md` states it; this is the
    /// client-side half of the same assertion.
    #[test]
    fn the_macos_entry_names_the_app_bundle() {
        for key in [PlatformKey::MacosArm64] {
            let manifest = valid();
            let url = &manifest.file_for(key).expect("present").url;
            assert!(url.ends_with("-app.tar.gz"), "{url}");
        }
    }

    #[test]
    fn a_platform_with_no_entry_is_absent_rather_than_wrong() {
        assert!(valid().file_for(PlatformKey::LinuxX64).is_none());
    }

    #[test]
    fn a_missing_required_field_is_refused() {
        let json = r#"{"manifest_version":1,"channel":"stable","version":"9.9.9",
                       "published_at":"x","files":{}}"#;
        assert!(
            matches!(parse(json.as_bytes()), Err(ManifestError::Malformed(_))),
            "a document with no `notes` must not half-parse"
        );
    }

    #[test]
    fn a_document_that_is_not_json_is_refused() {
        assert!(matches!(
            parse(b"<html>404</html>"),
            Err(ManifestError::Malformed(_))
        ));
    }

    #[test]
    fn a_manifest_version_from_the_future_is_refused_rather_than_misread() {
        let json = VALID.replace("\"manifest_version\": 1", "\"manifest_version\": 2");
        assert_eq!(
            parse(json.as_bytes()),
            Err(ManifestError::UnsupportedVersion {
                found: 2,
                supported: 1
            })
        );
    }

    #[test]
    fn a_manifest_with_no_version_key_is_refused() {
        let json = VALID.replace("\"manifest_version\": 1,", "");
        assert_eq!(parse(json.as_bytes()), Err(ManifestError::MissingVersion));
    }

    /// Forward compatibility in the direction that matters: a fifth platform
    /// must not stop the four that already work.
    #[test]
    fn an_unknown_platform_key_is_ignored_not_fatal() {
        let json = VALID.replace(
            "\"files\": {",
            r#""files": {
              "freebsd-x64": {
                "url": "https://example.invalid/x", "sha256": "nope", "size": 0, "signature": null
              },"#,
        );
        let manifest = parse(json.as_bytes()).expect("an unknown key must not fail the parse");
        assert!(manifest.file_for(PlatformKey::MacosArm64).is_some());
        assert!(
            manifest.files.contains_key("freebsd-x64"),
            "kept in the map, just not resolvable"
        );
    }

    #[test]
    fn an_unreadable_version_is_refused() {
        let json = VALID.replace("\"version\": \"0.1.6\"", "\"version\": \"latest\"");
        assert_eq!(
            parse(json.as_bytes()),
            Err(ManifestError::UnreadableVersion("latest".into()))
        );
    }

    #[test]
    fn a_malformed_digest_fails_the_parse_not_the_download() {
        let json = VALID.replace(
            "0a404f822c95e9f3f5a93abdfef8b005017c033eff4d72248f595798afaad1c3",
            "0a404f82",
        );
        let err = parse(json.as_bytes()).expect_err("a truncated digest is unusable");
        assert!(
            matches!(err, ManifestError::InvalidFile { ref platform, .. } if platform == "macos-arm64"),
            "{err:?}"
        );
    }

    #[test]
    fn a_zero_size_is_refused() {
        let json = VALID.replace("\"size\": 11569143", "\"size\": 0");
        assert!(matches!(
            parse(json.as_bytes()),
            Err(ManifestError::InvalidFile { .. })
        ));
    }

    /// The archive becomes an executable the user runs. Plaintext is refused at
    /// parse time so no fetch path can forget to check.
    #[test]
    fn a_plaintext_url_is_refused() {
        let json = VALID.replace(
            "https://github.com/MrGru/dodo/releases/download/v0.1.6/dodo-v0.1.6-macos-arm64-app.tar.gz",
            "http://github.com/MrGru/dodo/releases/download/v0.1.6/dodo-v0.1.6-macos-arm64-app.tar.gz",
        );
        assert!(matches!(
            parse(json.as_bytes()),
            Err(ManifestError::InvalidFile { .. })
        ));
    }

    /// A platform dodo does not run on may carry whatever it likes; refusing it
    /// would let one broken row block everyone.
    #[test]
    fn a_bad_entry_for_a_platform_we_are_not_does_not_block_the_parse() {
        let json = VALID.replace("\"size\": 12332862", "\"size\": 12332862, \"extra\": true");
        assert!(parse(json.as_bytes()).is_ok(), "unknown fields are ignored");
    }
}
