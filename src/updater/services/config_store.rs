//! Where `updater.json` lives between sessions.
//!
//! The fourth file under [`data_dir`](crate::paths::data_dir), and deliberately
//! the same shape as
//! [`consent_store`](crate::api_explorer::services::consent_store): a trait, a
//! disk implementation, a temp-file-then-rename write, and a `version` field
//! written from the **first** save with a parser that refuses anything newer.
//! `AGENTS.md` names that as the pattern to copy and `collections.json`'s
//! `#[serde(default)]`-only versioning as the one not to.
//!
//! # Why a *higher* version is refused rather than read
//!
//! A future dodo might give `channel` a fourth value, or make `manifest_url` a
//! list. Reading such a file with today's `serde` would silently take the parts
//! that still line up and drop the rest — and the parts of this file decide
//! *what gets downloaded and installed*. Refusing leaves the defaults in place,
//! which is the safe end: stable channel, official URL.
//!
//! # A missing file is first run, not an error
//!
//! Every key has a default, so dodo works with no file at all and writes one
//! the first time something is changed.
//!
//! # Threading
//!
//! Blocking by contract, like every other store here. Always called from the
//! background executor, never the UI thread.

use std::path::PathBuf;
#[cfg(test)]
use std::sync::Mutex;

use serde_json::Value;

use crate::paths::data_dir;
use crate::updater::models::config::{SCHEMA_VERSION, UpdaterConfig};
use crate::updater::models::state::UpdateError;

/// A place the updater settings are loaded from and saved to.
pub trait UpdaterConfigStore: Send + Sync + 'static {
    fn load(&self) -> Result<UpdaterConfig, UpdateError>;
    fn persist(&self, config: &UpdaterConfig) -> Result<(), UpdateError>;
}

/// Reads a settings document, refusing a schema this build does not understand.
pub fn parse_document(bytes: &[u8]) -> Result<UpdaterConfig, UpdateError> {
    let value: Value =
        serde_json::from_slice(bytes).map_err(|err| UpdateError::Io(err.to_string()))?;

    let version = value
        .get("version")
        .and_then(Value::as_u64)
        .ok_or_else(|| UpdateError::Io("updater.json carries no version".to_owned()))?;

    if version > u64::from(SCHEMA_VERSION) {
        return Err(UpdateError::Io(format!(
            "updater.json is version {version}; this dodo understands {SCHEMA_VERSION}"
        )));
    }

    serde_json::from_value(value).map_err(|err| UpdateError::Io(err.to_string()))
}

/// The settings, as one JSON file under [`data_dir`].
pub struct DiskUpdaterConfigStore {
    path: PathBuf,
}

impl Default for DiskUpdaterConfigStore {
    fn default() -> Self {
        Self {
            path: data_dir().join("updater.json"),
        }
    }
}

impl DiskUpdaterConfigStore {
    pub fn new() -> Self {
        Self::default()
    }

    #[cfg(test)]
    pub fn at(path: PathBuf) -> Self {
        Self { path }
    }
}

impl UpdaterConfigStore for DiskUpdaterConfigStore {
    fn load(&self) -> Result<UpdaterConfig, UpdateError> {
        match std::fs::read(&self.path) {
            Ok(bytes) => parse_document(&bytes).map_err(|error| match error {
                UpdateError::Io(detail) => {
                    UpdateError::Io(format!("{}: {detail}", self.path.display()))
                }
                other => other,
            }),
            // No file yet is the ordinary first-run state.
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(UpdaterConfig::default()),
            Err(err) => Err(UpdateError::Io(format!("{}: {err}", self.path.display()))),
        }
    }

    fn persist(&self, config: &UpdaterConfig) -> Result<(), UpdateError> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)
                .map_err(|err| UpdateError::Io(format!("{}: {err}", dir.display())))?;
        }
        let json =
            serde_json::to_vec_pretty(config).map_err(|err| UpdateError::Io(err.to_string()))?;

        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(&tmp, &json)
            .map_err(|err| UpdateError::Io(format!("{}: {err}", tmp.display())))?;
        std::fs::rename(&tmp, &self.path)
            .map_err(|err| UpdateError::Io(format!("{}: {err}", self.path.display())))?;
        Ok(())
    }
}

/// Settings held in memory. A test double only — see
/// [`InMemoryManifestSource`](super::manifest_source::InMemoryManifestSource)
/// for why that makes it `#[cfg(test)]`.
#[cfg(test)]
#[derive(Default)]
pub struct InMemoryConfigStore {
    config: Mutex<UpdaterConfig>,
}

#[cfg(test)]
impl InMemoryConfigStore {
    /// Starts from settings other than the defaults — the shape a test needs
    /// when it is checking what a *stored* choice does.
    pub fn holding(config: UpdaterConfig) -> Self {
        Self {
            config: Mutex::new(config),
        }
    }
}

#[cfg(test)]
impl UpdaterConfigStore for InMemoryConfigStore {
    fn load(&self) -> Result<UpdaterConfig, UpdateError> {
        Ok(self
            .config
            .lock()
            .map(|config| config.clone())
            .unwrap_or_default())
    }

    fn persist(&self, config: &UpdaterConfig) -> Result<(), UpdateError> {
        if let Ok(mut held) = self.config.lock() {
            *held = config.clone();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{DiskUpdaterConfigStore, InMemoryConfigStore, UpdaterConfigStore, parse_document};
    use crate::updater::models::config::{SCHEMA_VERSION, UpdaterConfig};
    use crate::updater::models::state::UpdateError;
    use crate::updater::models::version::Channel;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    fn temp_path() -> PathBuf {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "dodo-updater-config-test-{}-{n}/updater.json",
            std::process::id()
        ))
    }

    #[test]
    fn no_file_yet_is_the_defaults_not_an_error() {
        let store = DiskUpdaterConfigStore::at(temp_path());
        assert_eq!(store.load().expect("first run"), UpdaterConfig::default());
    }

    /// The key claim of this round: a setting that survives a restart, which is
    /// what makes "skip this version" mean anything.
    #[test]
    fn a_skip_survives_a_restart_with_its_version_in_the_file() {
        let path = temp_path();
        let mut config = UpdaterConfig::default();
        config.skip("0.2.0");
        config.channel = Channel::Beta;

        DiskUpdaterConfigStore::at(path.clone())
            .persist(&config)
            .expect("persists");

        let written = std::fs::read_to_string(&path).expect("reads back");
        assert!(
            written.contains(&format!("\"version\": {SCHEMA_VERSION}")),
            "no version field in the first file written:\n{written}"
        );

        let loaded = DiskUpdaterConfigStore::at(path.clone())
            .load()
            .expect("loads");
        assert_eq!(loaded.skipped_version.as_deref(), Some("0.2.0"));
        assert_eq!(loaded.channel, Channel::Beta);

        if let Some(dir) = path.parent() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    #[test]
    fn a_file_from_a_newer_dodo_is_refused_rather_than_misread() {
        let json = format!(r#"{{"version":{}}}"#, SCHEMA_VERSION + 3);
        let error = parse_document(json.as_bytes()).expect_err("refused");
        assert!(
            format!("{error:?}").contains("understands"),
            "the message should say what this build can read: {error:?}"
        );
    }

    #[test]
    fn a_file_with_no_version_is_refused() {
        assert!(matches!(
            parse_document(br#"{"auto_update":false}"#),
            Err(UpdateError::Io(_))
        ));
    }

    #[test]
    fn a_corrupt_file_is_an_error_rather_than_silent_defaults() {
        assert!(matches!(
            parse_document(b"{ not json"),
            Err(UpdateError::Io(_))
        ));
    }

    #[test]
    fn the_write_is_atomic_and_leaves_no_temp_file() {
        let path = temp_path();
        DiskUpdaterConfigStore::at(path.clone())
            .persist(&UpdaterConfig::default())
            .expect("persists");
        assert!(path.exists());
        assert!(
            !path.with_extension("json.tmp").exists(),
            "the temp file has to be renamed, not left beside the real one"
        );

        if let Some(dir) = path.parent() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    /// The twin has to be able to *start* from stored settings, not only to
    /// remember what it was handed: that is the shape a test needs when it is
    /// checking what an existing choice does on the next launch.
    #[test]
    fn the_in_memory_store_can_start_from_stored_settings() {
        let mut stored = UpdaterConfig {
            auto_update: false,
            ..UpdaterConfig::default()
        };
        stored.skip("0.2.0");

        let store = InMemoryConfigStore::holding(stored.clone());
        let loaded = store.load().expect("loads");
        assert_eq!(loaded, stored);
        assert!(!loaded.checks_on_startup());
    }

    #[test]
    fn the_in_memory_store_round_trips() {
        let store = InMemoryConfigStore::default();
        let config = UpdaterConfig {
            auto_update: false,
            ..UpdaterConfig::default()
        };
        store.persist(&config).expect("persists");
        assert_eq!(store.load().expect("loads"), config);
    }
}
