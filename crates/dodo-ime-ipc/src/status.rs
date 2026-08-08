//! `input-method-status.json`: what the input method tells dodo about itself.
//!
//! **The bundle is the only writer.** dodo reads it to answer one question — is
//! the thing I installed running, and has it seen my settings — and never writes
//! it.
//!
//! # This is the file that changed a rule, so read the rule first
//!
//! `dodo_ime_macos`'s crate docs say: *"Nothing here writes a file, opens a
//! socket, or prints a composition."* The first clause is no longer true, and
//! this is the file. The rule underneath it is unchanged and is what constrains
//! this type:
//!
//! **Nothing the user typed may ever appear here.** Not a syllable, not a key,
//! not a count of them, not a timestamp of the last one, and not the identifier
//! of the application being typed into. Every field below is about the *bundle*:
//! which build it is, which process, when it started, and which settings
//! revision it has applied. [`tests::the_status_file_carries_only_these_keys`]
//! pins the key set, so a field added here fails a test and whoever adds it
//! reads this paragraph.
//!
//! Two consequences that are easy to get wrong:
//!
//! - **It is written when the bundle starts and when it applies settings, never
//!   on a keystroke.** A file written per keystroke would be a typing log by
//!   another name, whatever its fields said, because its *mtime* would carry the
//!   information.
//! - **`pid` is not a liveness check.** dodo reads it to show the running build,
//!   and a stale file from a process that has exited is the ordinary case — macOS
//!   stops the agent when nothing is typing at it and relaunches it later. dodo
//!   must not treat a pid as "running", and [`StatusDocument::describes_a_live_process`]
//!   is the deliberately narrow thing it may do instead.

use serde::{Deserialize, Serialize};

use crate::document::{IpcError, parse_versioned, read_versioned, write_atomic};

/// The file's name under [`paths::support_dir`](crate::paths::support_dir).
pub const STATUS_FILE: &str = "input-method-status.json";

/// The schema this build writes and is willing to read.
pub const STATUS_SCHEMA_VERSION: u32 = 1;

/// What the input-method process last said about itself.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct StatusDocument {
    pub version: u32,
    /// The bundle's own `CARGO_PKG_VERSION`, so dodo can say whether what is
    /// installed is what it would install.
    #[serde(default)]
    pub bundle_version: String,
    /// The process that wrote this. See the module docs: not a liveness check.
    #[serde(default)]
    pub pid: u32,
    /// Seconds since the Unix epoch, when that process started serving.
    ///
    /// Deliberately *not* "when it last typed": see the privacy note. A
    /// start-of-process timestamp says nothing about what the user was doing,
    /// and it is what makes "your settings reached a process older than the
    /// change" answerable.
    #[serde(default)]
    pub started_unix: u64,
    /// The [`SettingsDocument::revision`](crate::settings::SettingsDocument::revision)
    /// this process has applied. `0` means it is typing with its compiled-in
    /// defaults — either because no settings file existed or because the file
    /// was refused.
    #[serde(default)]
    pub settings_revision: u64,
}

impl Default for StatusDocument {
    fn default() -> StatusDocument {
        StatusDocument {
            version: STATUS_SCHEMA_VERSION,
            bundle_version: String::new(),
            pid: 0,
            started_unix: 0,
            settings_revision: 0,
        }
    }
}

impl StatusDocument {
    /// The document the bundle should write now.
    ///
    /// `bundle_version` is passed in rather than read from this crate's own
    /// `CARGO_PKG_VERSION`: the interesting version is the *bundle's*, and this
    /// crate is linked by both processes, so reading it here would report dodo's
    /// answer in the bundle's file.
    pub fn now(bundle_version: &str, settings_revision: u64) -> StatusDocument {
        StatusDocument {
            version: STATUS_SCHEMA_VERSION,
            bundle_version: bundle_version.to_owned(),
            pid: std::process::id(),
            started_unix: unix_seconds(),
            settings_revision,
        }
    }

    /// Whether this file was written by a process that still exists.
    ///
    /// `kill(pid, 0)` on Unix: no signal is sent, and the answer is whether a
    /// process with that id exists and is signallable by this user. That is a
    /// weaker claim than "the input method is running" — a recycled pid answers
    /// yes — and dodo must present it as such rather than as a status light.
    ///
    /// It is also the *only* reason dodo reads `pid`, which is why this is here
    /// rather than in dodo: a caller tempted to do more with the number finds
    /// this doc comment first.
    #[cfg(unix)]
    pub fn describes_a_live_process(&self) -> bool {
        if self.pid == 0 {
            return false;
        }
        // SAFETY: `kill` with signal 0 performs the permission and existence
        // checks and sends nothing. There is no memory involved.
        unsafe { libc_kill(self.pid as i32, 0) == 0 }
    }

    pub fn read(path: &std::path::Path) -> Result<Option<StatusDocument>, IpcError> {
        read_versioned(path, STATUS_SCHEMA_VERSION)
    }

    /// Parses bytes, so the version rule is testable without a disk.
    pub fn parse(bytes: &[u8]) -> Result<StatusDocument, IpcError> {
        parse_versioned(bytes, STATUS_SCHEMA_VERSION)
    }

    /// Writes the file. **The bundle only** — see the single-writer rule in the
    /// crate docs.
    pub fn write(&self, path: &std::path::Path) -> Result<(), IpcError> {
        write_atomic(path, self)
    }
}

// `kill(2)`, declared rather than depended on.
//
// The one libc call either process needs. Adding the `libc` crate to the crate
// both of them link — for a two-argument signature that has been stable since
// POSIX.1-1988 — is a worse trade than four lines here. The signature is
// checked by the linker on every platform this crate builds for. (A `///`
// comment here is an `unused_doc_comments` error: rustdoc documents nothing
// inside an `extern` block.)
#[cfg(unix)]
unsafe extern "C" {
    #[link_name = "kill"]
    fn libc_kill(pid: i32, signal: i32) -> i32;
}

/// Seconds since the Unix epoch, or `0` on a machine whose clock is before it.
fn unix_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::{STATUS_SCHEMA_VERSION, StatusDocument};
    use crate::document::IpcError;

    /// The privacy guard. Every key in this file is about the bundle; none is
    /// about what was typed. A new field fails here on purpose — read the module
    /// docs before changing the list.
    #[test]
    fn the_status_file_carries_only_these_keys() {
        let json = serde_json::to_value(StatusDocument::now("0.1.0", 3)).unwrap();
        let mut keys: Vec<_> = json
            .as_object()
            .expect("an object")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "bundle-version",
                "pid",
                "settings-revision",
                "started-unix",
                "version"
            ]
        );
    }

    #[test]
    fn a_fresh_status_names_this_process_and_the_given_build() {
        let status = StatusDocument::now("1.2.3", 7);
        assert_eq!(status.version, STATUS_SCHEMA_VERSION);
        assert_eq!(status.bundle_version, "1.2.3");
        assert_eq!(status.pid, std::process::id());
        assert_eq!(status.settings_revision, 7);
        assert!(status.started_unix > 1_700_000_000, "a plausible clock");
    }

    #[test]
    fn a_newer_schema_is_refused() {
        let error = StatusDocument::parse(br#"{"version":50,"pid":1}"#).unwrap_err();
        assert_eq!(
            error,
            IpcError::UnsupportedVersion {
                found: 50,
                supported: STATUS_SCHEMA_VERSION
            }
        );
    }

    /// dodo reads this file from a *different* build of the bundle, so a
    /// version-1 file missing everything it did not know about is the normal
    /// forward case.
    #[test]
    fn a_partial_status_reads_as_defaults() {
        let status = StatusDocument::parse(br#"{"version":1}"#).unwrap();
        assert_eq!(status, StatusDocument::default());
    }

    #[test]
    fn junk_is_refused() {
        assert!(matches!(
            StatusDocument::parse(b"{\"version\":1,\"pid\":"),
            Err(IpcError::Io { .. })
        ));
        assert_eq!(
            StatusDocument::parse(br#"{"pid":10}"#),
            Err(IpcError::MissingVersion)
        );
    }

    #[cfg(unix)]
    #[test]
    fn this_process_is_live_and_a_zero_pid_is_not() {
        let mine = StatusDocument::now("0.0.0", 0);
        assert!(mine.describes_a_live_process());

        let none = StatusDocument {
            pid: 0,
            ..StatusDocument::default()
        };
        assert!(!none.describes_a_live_process());
    }

    #[cfg(unix)]
    #[test]
    fn an_implausible_pid_is_not_live() {
        // Above every configured `kern.maxproc` on macOS, so no process can
        // hold it. A wrong answer here would be "yes", which is the direction
        // that would make dodo claim an input method is running when none is.
        let stale = StatusDocument {
            pid: 0x7FFF_FFFF,
            ..StatusDocument::default()
        };
        assert!(!stale.describes_a_live_process());
    }

    #[test]
    fn a_written_status_reads_back() {
        let dir = std::env::temp_dir().join(format!("dodo-ime-status-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("input-method-status.json");

        assert_eq!(StatusDocument::read(&path).unwrap(), None);

        let status = StatusDocument::now("0.1.0", 4);
        status.write(&path).unwrap();
        assert_eq!(StatusDocument::read(&path).unwrap(), Some(status));

        let _ = std::fs::remove_dir_all(&dir);
    }
}
