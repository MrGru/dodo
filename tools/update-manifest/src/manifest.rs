//! The `update.json` document.
//!
//! Two fields exist for the future rather than for today, and both are cheap
//! now and impossible to retrofit:
//!
//! * **`manifest_version`** lets a future updater refuse a document it does not
//!   understand instead of mis-parsing it. A client that has only ever seen
//!   version 1 can compare and bail; without the field its only options are to
//!   guess or to break.
//! * **`signature`**, per file, is reserved for Ed25519/minisign. It is written
//!   as `null` and **nothing verifies it** — adding signing later becomes
//!   populating a field rather than a schema break that strands every client
//!   already in the wild.
//!
//! `channel` is written into the document from the first release for the same
//! reason: `stable` is the only channel that has to work today, but a client
//! that reads a manifest and finds no channel cannot tell a stable release from
//! a nightly one, and by then the shape is fixed.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;

/// The schema version of the document this crate writes.
pub const MANIFEST_VERSION: u32 = 1;

/// Which stream of releases a manifest describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Channel {
    Stable,
    Beta,
    Nightly,
}

impl Channel {
    pub const ALL: [Channel; 3] = [Channel::Stable, Channel::Beta, Channel::Nightly];

    pub fn as_str(self) -> &'static str {
        match self {
            Channel::Stable => "stable",
            Channel::Beta => "beta",
            Channel::Nightly => "nightly",
        }
    }

    pub fn parse(value: &str) -> Result<Channel, String> {
        Channel::ALL
            .into_iter()
            .find(|c| c.as_str() == value)
            .ok_or_else(|| {
                let valid: Vec<&str> = Channel::ALL.iter().map(|c| c.as_str()).collect();
                format!(
                    "unknown channel `{value}` (valid channels: {})",
                    valid.join(", ")
                )
            })
    }
}

impl fmt::Display for Channel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One downloadable archive.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestFile {
    /// Absolute download URL on the GitHub release.
    pub url: String,
    /// Lowercase hex SHA-256 of the archive, computed by this tool and checked
    /// against the archive's own `.sha256` sidecar.
    pub sha256: String,
    /// Exact size in bytes, so a client can show progress and reject a short
    /// read before it bothers hashing.
    pub size: u64,
    /// Reserved for a detached signature. Always `null` today; see the module
    /// doc. `skip_serializing_if` is deliberately **not** used — the key has to
    /// be present for its reservation to mean anything.
    pub signature: Option<String>,
}

/// The whole `update.json`.
///
/// Field order here is the order in the file, and `files` is a `BTreeMap` so
/// platform keys are sorted. Both are deliberate: the document is regenerated
/// on every release and a stable field order makes two manifests diffable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub manifest_version: u32,
    pub channel: Channel,
    pub version: String,
    pub notes: String,
    pub published_at: String,
    pub files: BTreeMap<String, ManifestFile>,
}

impl Manifest {
    /// Serializes with a trailing newline, so the file is well-formed text and
    /// `git diff` does not report "\ No newline at end of file".
    pub fn to_json(&self) -> Result<String, String> {
        let mut json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("cannot serialize the manifest: {e}"))?;
        json.push('\n');
        Ok(json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Manifest {
        Manifest {
            manifest_version: MANIFEST_VERSION,
            channel: Channel::Stable,
            version: "0.2.0".to_string(),
            notes: "## dodo v0.2.0".to_string(),
            published_at: "2026-07-30T12:11:03Z".to_string(),
            files: BTreeMap::from([(
                "macos-arm64".to_string(),
                ManifestFile {
                    url: "https://github.com/MrGru/dodo/releases/download/v0.2.0/dodo-v0.2.0-macos-arm64-app.tar.gz".to_string(),
                    sha256: "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9".to_string(),
                    size: 123_456,
                    signature: None,
                },
            )]),
        }
    }

    #[test]
    fn json_has_the_documented_shape() {
        let json = sample().to_json().expect("serializes");
        let value: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");

        assert_eq!(value["manifest_version"], 1);
        assert_eq!(value["channel"], "stable");
        assert_eq!(value["version"], "0.2.0");
        assert_eq!(value["published_at"], "2026-07-30T12:11:03Z");

        let entry = &value["files"]["macos-arm64"];
        assert_eq!(entry["size"], 123_456);
        assert!(
            entry["url"]
                .as_str()
                .is_some_and(|u| u.ends_with("-app.tar.gz"))
        );
        assert_eq!(entry["sha256"].as_str().map(str::len), Some(64));
    }

    /// The reserved key must be *present* and null, not absent. An updater that
    /// later learns to read signatures distinguishes "unsigned" from "field I
    /// do not know about" by this.
    #[test]
    fn signature_is_written_as_an_explicit_null() {
        let json = sample().to_json().expect("serializes");
        assert!(json.contains("\"signature\": null"), "{json}");

        let value: serde_json::Value = serde_json::from_str(&json).expect("valid JSON");
        let entry = &value["files"]["macos-arm64"];
        assert!(entry.get("signature").is_some(), "key must exist");
        assert!(entry["signature"].is_null());
    }

    #[test]
    fn channels_round_trip() {
        for channel in Channel::ALL {
            assert_eq!(Channel::parse(channel.as_str()), Ok(channel));
        }
        let err = Channel::parse("edge").expect_err("should reject");
        assert!(err.contains("edge"), "{err}");
        assert!(err.contains("stable"), "{err}");
    }

    #[test]
    fn a_manifest_round_trips_through_json() {
        let original = sample();
        let json = original.to_json().expect("serializes");
        let parsed: Manifest = serde_json::from_str(&json).expect("deserializes");
        assert_eq!(parsed, original);
    }

    #[test]
    fn json_ends_with_a_newline() {
        assert!(sample().to_json().expect("serializes").ends_with("}\n"));
    }
}
