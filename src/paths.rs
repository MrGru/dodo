//! Where dodo keeps the files it writes.
//!
//! One directory, resolved once, shared by every store that persists something:
//! `collections.json`, `environments.json`, `script-consent.json`,
//! `updater.json`, `connections.json` and `query-data.json`. It used to live in
//! [`api_explorer::services::collection_store`](crate::api_explorer::services::collection_store)
//! because that was the first module to persist anything; it moved here when
//! the second top-level module — the updater — started needing it, so neither
//! one owns the other's path.
//!
//! # Why the platform is read from the *target string*, not from `cfg`
//!
//! [`HostOs::current`] derives the platform from
//! [`VERSION_INFO.target`](crate::build_info::VERSION_INFO), the triple
//! `build.rs` embedded, rather than from `#[cfg(target_os = …)]`. The rule the
//! choice buys is worth the indirection: **every branch is unit-testable from
//! any host.** Two of dodo's four release targets cannot even be *compiled*
//! from the machine this is usually written on (see `docs/release.md`), so a
//! `cfg`-split resolver would have had its Windows and Linux branches shipped
//! unexecuted and untested. Here they are ordinary data, and
//! [`resolve`] is exhaustively tested for all three.
//!
//! The triple is fixed at compile time, so this is not a runtime guess — it is
//! the same fact `cfg` would have given, in a form a test can hold.
//!
//! # The macOS path is frozen
//!
//! `~/Library/Application Support/dodo` is where every existing installation's
//! saved collections already are. Whatever else changes here, that string may
//! not: a "better" path would silently orphan them.

use std::path::PathBuf;

use crate::build_info::VERSION_INFO;

/// The directory name, under whichever per-user config root the platform uses.
const APP_DIR: &str = "dodo";

/// Last resort when the environment names no home at all: a hidden folder in
/// the working directory. It keeps the app running rather than panicking, and
/// it is deliberately the *last* branch on every platform — on Windows `HOME`
/// is normally unset, which is exactly how this used to become the *first*
/// branch there and write into whatever directory the app was launched from.
const FALLBACK_DIR: &str = ".dodo";

/// The three platform families dodo's data directory differs between.
///
/// Not the same axis as [`PlatformKey`](crate::updater::models::platform::PlatformKey),
/// which enumerates the four *release* targets: this one has to answer for any
/// target anyone builds, including the Linux-on-arm64 and BSD builds dodo does
/// not release, so everything that is not macOS or Windows resolves to
/// [`HostOs::Unix`] and follows the XDG basedir spec.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HostOs {
    MacOs,
    Windows,
    Unix,
}

impl HostOs {
    /// Classifies a Rust target triple.
    ///
    /// Substring matches on purpose: `aarch64-apple-darwin` and
    /// `x86_64-apple-darwin` are both macOS, `x86_64-pc-windows-msvc` and
    /// `-gnu` are both Windows, and an unrecognised triple lands on
    /// [`HostOs::Unix`] — the branch whose fallbacks are the most conservative.
    ///
    /// `apple-ios` and the other Apple non-desktop triples would classify as
    /// macOS here. dodo does not build for them, and if it ever does, the
    /// directory is the least of what would need revisiting.
    pub fn of_target(target: &str) -> HostOs {
        if target.contains("apple-darwin") {
            HostOs::MacOs
        } else if target.contains("windows") {
            HostOs::Windows
        } else {
            HostOs::Unix
        }
    }

    /// The platform this binary was compiled for.
    pub fn current() -> HostOs {
        HostOs::of_target(VERSION_INFO.target)
    }
}

/// The environment variables the resolution reads, lifted into a value so the
/// rules can be tested without touching the real process environment (which is
/// global, and which other tests run concurrently with).
#[derive(Clone, Debug, Default)]
pub struct Environment {
    pub home: Option<PathBuf>,
    /// `%APPDATA%` — the per-user roaming application data directory. Windows
    /// only; every other platform leaves it `None`.
    pub appdata: Option<PathBuf>,
    /// `$XDG_CONFIG_HOME`. Per the XDG base directory specification a
    /// *relative* value is invalid and must be ignored, which [`resolve`] does.
    pub xdg_config_home: Option<PathBuf>,
}

impl Environment {
    /// Reads the three variables from the real environment.
    pub fn from_env() -> Environment {
        let var = |name: &str| std::env::var_os(name).map(PathBuf::from);
        Environment {
            home: var("HOME"),
            appdata: var("APPDATA"),
            xdg_config_home: var("XDG_CONFIG_HOME"),
        }
    }
}

/// The per-user directory dodo writes its files into, by platform convention.
///
/// - **macOS** — `$HOME/Library/Application Support/dodo`. Frozen; see the
///   module doc.
/// - **Windows** — `%APPDATA%\dodo`, falling back to
///   `%USERPROFILE%`-shaped `$HOME\AppData\Roaming\dodo` when `APPDATA` is
///   unset. `HOME` is usually unset on Windows, which is why `APPDATA` is
///   first rather than second.
/// - **Everything else** — `$XDG_CONFIG_HOME/dodo` when that is set to an
///   absolute path, else `$HOME/.config/dodo`, per the XDG base directory
///   specification.
///
/// Pure: it creates nothing and reads no filesystem. The directory is made on
/// first save by whichever store writes first.
pub fn resolve(os: HostOs, env: &Environment) -> PathBuf {
    match os {
        HostOs::MacOs => match &env.home {
            Some(home) => home
                .join("Library")
                .join("Application Support")
                .join(APP_DIR),
            None => PathBuf::from(FALLBACK_DIR),
        },
        HostOs::Windows => match (&env.appdata, &env.home) {
            (Some(appdata), _) => appdata.join(APP_DIR),
            (None, Some(home)) => home.join("AppData").join("Roaming").join(APP_DIR),
            (None, None) => PathBuf::from(FALLBACK_DIR),
        },
        HostOs::Unix => {
            // A relative XDG_CONFIG_HOME is invalid per the spec and is
            // ignored, not joined — honouring one would put the config
            // wherever the app happened to be launched from.
            if let Some(xdg) = env.xdg_config_home.as_ref().filter(|p| p.is_absolute()) {
                return xdg.join(APP_DIR);
            }
            match &env.home {
                Some(home) => home.join(".config").join(APP_DIR),
                None => PathBuf::from(FALLBACK_DIR),
            }
        }
    }
}

/// dodo's data directory on this machine, created by whichever store saves
/// first.
pub fn data_dir() -> PathBuf {
    resolve(HostOs::current(), &Environment::from_env())
}

#[cfg(test)]
mod tests {
    use super::{Environment, HostOs, resolve};
    use std::path::PathBuf;

    /// Builds an [`Environment`] from `(name, value)` pairs, so each case names
    /// only the variables it is about and leaves the rest unset.
    fn env_of(pairs: &[(&str, &str)]) -> Environment {
        let get = |name: &str| -> Option<PathBuf> {
            pairs
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| PathBuf::from(*value))
        };
        Environment {
            home: get("HOME"),
            appdata: get("APPDATA"),
            xdg_config_home: get("XDG_CONFIG_HOME"),
        }
    }

    #[test]
    fn every_release_target_triple_classifies() {
        for (target, expected) in [
            ("aarch64-apple-darwin", HostOs::MacOs),
            ("x86_64-apple-darwin", HostOs::MacOs),
            ("x86_64-unknown-linux-gnu", HostOs::Unix),
            ("x86_64-pc-windows-msvc", HostOs::Windows),
        ] {
            assert_eq!(HostOs::of_target(target), expected, "{target}");
        }
    }

    #[test]
    fn unusual_triples_land_on_conservative_branches() {
        assert_eq!(
            HostOs::of_target("x86_64-pc-windows-gnu"),
            HostOs::Windows,
            "the gnu ABI is still Windows"
        );
        assert_eq!(
            HostOs::of_target("aarch64-unknown-linux-musl"),
            HostOs::Unix
        );
        assert_eq!(
            HostOs::of_target("something-nobody-has-built"),
            HostOs::Unix,
            "an unrecognised triple must not be mistaken for Windows or macOS"
        );
    }

    /// The one path in this module that may never change: it is where every
    /// existing installation's saved collections already are.
    #[test]
    fn the_macos_path_is_the_one_that_already_holds_peoples_files() {
        let dir = resolve(HostOs::MacOs, &env_of(&[("HOME", "/Users/someone")]));
        assert_eq!(
            dir,
            PathBuf::from("/Users/someone/Library/Application Support/dodo")
        );
    }

    /// The input-method bundle resolves this directory itself, in
    /// `dodo_ime_ipc::paths::support_dir`, because it has no `build_info` to
    /// classify a platform from and no reason to know what `%APPDATA%` is. That
    /// makes two spellings of a frozen path in two crates, and **this is the test
    /// that keeps them one answer**: without it, a change here would silently
    /// leave the input method reading settings dodo no longer writes.
    ///
    /// It lives on dodo's side rather than the contract crate's because only dodo
    /// can see both functions.
    #[test]
    fn the_input_method_agrees_about_the_data_directory() {
        let home = PathBuf::from("/Users/someone");
        assert_eq!(
            resolve(HostOs::MacOs, &env_of(&[("HOME", "/Users/someone")])),
            dodo_ime_ipc::paths::support_dir(&home),
            "dodo and its input method must look in the same directory"
        );
    }

    /// The expectations below join rather than spell the path out, because
    /// `PathBuf::join` uses the *host's* separator: run on this Mac, the
    /// Windows branch produces `C:\Users\someone\AppData\Roaming/dodo`. That is
    /// correct — on Windows the same call produces a backslash — so what these
    /// assert is the decision (which variable, which directories), which is the
    /// part that was wrong before.
    #[test]
    fn windows_prefers_appdata_because_home_is_usually_unset_there() {
        // The bug this branch exists to fix: with only the old macOS branch and
        // its `.dodo` fallback, a Windows launch wrote into the *working
        // directory*, because `HOME` is normally not set on Windows at all.
        let appdata = PathBuf::from(r"C:\Users\someone\AppData\Roaming");
        let dir = resolve(
            HostOs::Windows,
            &env_of(&[("APPDATA", r"C:\Users\someone\AppData\Roaming")]),
        );
        assert_eq!(dir, appdata.join("dodo"));
    }

    #[test]
    fn windows_falls_back_to_the_roaming_path_under_home() {
        let home = PathBuf::from(r"C:\Users\someone");
        let dir = resolve(HostOs::Windows, &env_of(&[("HOME", r"C:\Users\someone")]));
        assert_eq!(dir, home.join("AppData").join("Roaming").join("dodo"));
    }

    #[test]
    fn appdata_wins_over_home_on_windows() {
        let dir = resolve(
            HostOs::Windows,
            &env_of(&[("APPDATA", "/roaming"), ("HOME", "/home/someone")]),
        );
        assert_eq!(dir, PathBuf::from("/roaming/dodo"));
    }

    #[test]
    fn unix_follows_xdg_when_it_is_absolute() {
        let dir = resolve(
            HostOs::Unix,
            &env_of(&[("XDG_CONFIG_HOME", "/home/someone/.config-alt")]),
        );
        assert_eq!(dir, PathBuf::from("/home/someone/.config-alt/dodo"));
    }

    #[test]
    fn unix_ignores_a_relative_xdg_as_the_spec_requires() {
        let dir = resolve(
            HostOs::Unix,
            &env_of(&[("XDG_CONFIG_HOME", "relative/path"), ("HOME", "/home/x")]),
        );
        assert_eq!(
            dir,
            PathBuf::from("/home/x/.config/dodo"),
            "a relative XDG_CONFIG_HOME is invalid and must be ignored, not joined"
        );
    }

    #[test]
    fn unix_defaults_to_dot_config() {
        let dir = resolve(HostOs::Unix, &env_of(&[("HOME", "/home/someone")]));
        assert_eq!(dir, PathBuf::from("/home/someone/.config/dodo"));
    }

    #[test]
    fn an_empty_environment_still_yields_a_usable_relative_path() {
        for os in [HostOs::MacOs, HostOs::Windows, HostOs::Unix] {
            assert_eq!(
                resolve(os, &Environment::default()),
                PathBuf::from(".dodo"),
                "{os:?} must degrade rather than panic"
            );
        }
    }
}
