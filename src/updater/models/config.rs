//! `updater.json` — the first thing dodo persists that is a *setting*.
//!
//! Everything in the Settings dialog resets on launch by deliberate design
//! (`AGENTS.md`), and the three files under
//! [`data_dir`](crate::paths::data_dir) that survive a restart are all *data*:
//! saved collections, environments, script approvals. This is the first durable
//! setting, so it follows
//! [`script_consent`](crate::api_explorer::models::script_consent)'s file
//! discipline exactly and pointedly **not** `collections.json`'s:
//!
//! - an explicit `"version"` written from the very first save;
//! - a parser that **refuses** a higher version rather than half-reading it;
//! - a missing file meaning *first run*, not an error;
//! - a temp-file-then-rename write, so a crash mid-save cannot truncate it.
//!
//! # What the keys mean, and what `auto_update` does not mean
//!
//! The behaviour was decided with the captain: **check silently, ask before
//! downloading.** So [`auto_update`](UpdaterConfig::auto_update) is the master
//! switch for *checking*, and never authorises an unattended install — there is
//! no setting that does, and the download only ever starts from a button. It is
//! named `auto_update` because that is what a person looks for in a config file;
//! the doc comment on the field is where the distinction is stated.
//!
//! [`check_on_startup`](UpdaterConfig::check_on_startup) and
//! [`check_interval_hours`](UpdaterConfig::check_interval_hours) are both gated
//! by it: the first is the one check shortly after launch, the second the
//! cadence while the app keeps running.
//!
//! [`skipped_version`](UpdaterConfig::skipped_version) is why this file has to
//! exist at all rather than being a global that resets: "skip this one" is a
//! statement about a specific release, and re-offering it every launch would
//! make the button a lie.

use serde::{Deserialize, Serialize};

use crate::updater::models::version::Channel;

/// The schema version written into every `updater.json`.
pub const SCHEMA_VERSION: u32 = 1;

/// The manifest the app reads unless the file says otherwise.
///
/// `latest` deliberately: it resolves to the newest *non-pre-release*, which is
/// exactly the stable channel. `docs/release.md` records that this makes the URL
/// unusable for a beta channel, and what would replace it (per-channel paths on
/// GitHub Pages) when one exists.
pub const DEFAULT_MANIFEST_URL: &str =
    "https://github.com/MrGru/dodo/releases/latest/download/update.json";

/// How often the app re-checks while it keeps running, by default. A desktop
/// tool left open for a week should notice a release; one restarted daily is
/// served by the startup check alone.
pub const DEFAULT_CHECK_INTERVAL_HOURS: u32 = 24;

/// The floor a hand-edited interval is clamped to. Zero would busy-loop the
/// background executor against GitHub, so it is not an available choice; one
/// hour is already far below anything useful.
pub const MIN_CHECK_INTERVAL_HOURS: u32 = 1;
/// The ceiling. Four weeks; beyond that the check is not a check.
pub const MAX_CHECK_INTERVAL_HOURS: u32 = 24 * 28;

/// The persisted updater settings.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdaterConfig {
    /// Written first and read first. See the module doc.
    pub version: u32,
    /// The master switch for **checking**, not for installing. Nothing is ever
    /// downloaded without the user pressing a button, whatever this says.
    #[serde(default = "yes")]
    pub auto_update: bool,
    #[serde(default)]
    pub channel: Channel,
    #[serde(default = "default_manifest_url")]
    pub manifest_url: String,
    #[serde(default = "yes")]
    pub check_on_startup: bool,
    #[serde(default = "default_interval")]
    pub check_interval_hours: u32,
    /// The version the user pressed **Skip this version** for, as it appeared
    /// in the manifest. `None` once a newer one arrives.
    #[serde(default)]
    pub skipped_version: Option<String>,
}

fn yes() -> bool {
    true
}

fn default_manifest_url() -> String {
    DEFAULT_MANIFEST_URL.to_owned()
}

fn default_interval() -> u32 {
    DEFAULT_CHECK_INTERVAL_HOURS
}

impl Default for UpdaterConfig {
    fn default() -> Self {
        Self {
            version: SCHEMA_VERSION,
            auto_update: true,
            channel: Channel::Stable,
            manifest_url: default_manifest_url(),
            check_on_startup: true,
            check_interval_hours: DEFAULT_CHECK_INTERVAL_HOURS,
            skipped_version: None,
        }
    }
}

impl UpdaterConfig {
    /// The re-check cadence, clamped into a range that cannot hurt anyone. A
    /// hand-edited `0` becomes [`MIN_CHECK_INTERVAL_HOURS`] rather than a tight
    /// loop; the file is left as the user wrote it, because rewriting somebody's
    /// config behind their back is worse than ignoring one field of it.
    pub fn effective_interval_hours(&self) -> u32 {
        self.check_interval_hours
            .clamp(MIN_CHECK_INTERVAL_HOURS, MAX_CHECK_INTERVAL_HOURS)
    }

    /// Whether a background check should run at all.
    pub fn checks_automatically(&self) -> bool {
        self.auto_update
    }

    /// Whether the check shortly after launch should run.
    pub fn checks_on_startup(&self) -> bool {
        self.auto_update && self.check_on_startup
    }

    /// Records a skip. Stored as text rather than a parsed version so the file
    /// stays readable and so a version this build cannot parse still round-trips
    /// instead of being silently dropped.
    pub fn skip(&mut self, version: &str) {
        self.skipped_version = Some(version.to_owned());
    }

    /// Forgets any skip — what **Download** does, so that pressing skip and
    /// then changing your mind does not leave a stale entry behind.
    pub fn clear_skip(&mut self) {
        self.skipped_version = None;
    }
}

#[cfg(test)]
mod tests {
    use super::{
        DEFAULT_CHECK_INTERVAL_HOURS, DEFAULT_MANIFEST_URL, MAX_CHECK_INTERVAL_HOURS,
        MIN_CHECK_INTERVAL_HOURS, SCHEMA_VERSION, UpdaterConfig,
    };
    use crate::updater::models::version::Channel;

    #[test]
    fn the_defaults_are_the_documented_ones() {
        let config = UpdaterConfig::default();
        assert_eq!(config.version, SCHEMA_VERSION);
        assert!(config.auto_update);
        assert!(config.check_on_startup);
        assert_eq!(config.channel, Channel::Stable);
        assert_eq!(config.manifest_url, DEFAULT_MANIFEST_URL);
        assert_eq!(config.check_interval_hours, DEFAULT_CHECK_INTERVAL_HOURS);
        assert_eq!(config.skipped_version, None);
    }

    /// The URL is a contract with the release workflow: `docs/release.md` names
    /// exactly this as the address stable's manifest is published to.
    #[test]
    fn the_default_url_is_the_one_the_release_publishes_to() {
        assert_eq!(
            DEFAULT_MANIFEST_URL,
            "https://github.com/MrGru/dodo/releases/latest/download/update.json"
        );
    }

    #[test]
    fn a_hand_edited_interval_cannot_busy_loop_or_disable_itself() {
        let mut config = UpdaterConfig::default();

        config.check_interval_hours = 0;
        assert_eq!(config.effective_interval_hours(), MIN_CHECK_INTERVAL_HOURS);

        config.check_interval_hours = u32::MAX;
        assert_eq!(config.effective_interval_hours(), MAX_CHECK_INTERVAL_HOURS);

        config.check_interval_hours = 6;
        assert_eq!(config.effective_interval_hours(), 6);
    }

    #[test]
    fn auto_update_gates_both_kinds_of_check() {
        let mut config = UpdaterConfig::default();
        assert!(config.checks_automatically());
        assert!(config.checks_on_startup());

        config.check_on_startup = false;
        assert!(
            config.checks_automatically(),
            "the periodic check still runs"
        );
        assert!(!config.checks_on_startup());

        config.check_on_startup = true;
        config.auto_update = false;
        assert!(!config.checks_automatically());
        assert!(
            !config.checks_on_startup(),
            "the master switch has to win over the specific one"
        );
    }

    #[test]
    fn a_skip_is_recorded_verbatim_and_can_be_taken_back() {
        let mut config = UpdaterConfig::default();
        config.skip("0.2.0");
        assert_eq!(config.skipped_version.as_deref(), Some("0.2.0"));
        config.clear_skip();
        assert_eq!(config.skipped_version, None);
    }

    /// Every optional key has a default, so a file written by an older dodo —
    /// or hand-trimmed to one line — still loads.
    #[test]
    fn a_file_with_only_a_version_loads_as_the_defaults() {
        let config: UpdaterConfig =
            serde_json::from_str(r#"{"version":1}"#).expect("every other key defaults");
        assert_eq!(config, UpdaterConfig::default());
    }

    #[test]
    fn the_document_round_trips() {
        let mut config = UpdaterConfig::default();
        config.channel = Channel::Beta;
        config.skip("1.0.0-beta.2");
        config.check_interval_hours = 6;

        let json = serde_json::to_string(&config).expect("serializes");
        assert!(json.contains("\"version\":1"), "{json}");
        assert!(json.contains("\"channel\":\"beta\""), "{json}");
        assert_eq!(
            serde_json::from_str::<UpdaterConfig>(&json).expect("reads back"),
            config
        );
    }
}
