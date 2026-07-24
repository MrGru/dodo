//! Release metadata baked in at compile time by `build.rs`.
//!
//! Everything here is a `&'static str` read out of an environment variable the
//! build script set with `cargo:rustc-env`, so it costs nothing at runtime and
//! nothing at startup — there is no lazy initialisation to defer because there
//! is no initialisation at all.
//!
//! `build.rs` documents where each value comes from and how it degrades. The
//! two placeholders to know about: `unknown` means "could not be determined"
//! (built without git, e.g. from a source tarball) and `none` means
//! "determined, and there is none" (a commit with no tag on it). Release
//! verification greps for the former; see `docs/release.md`.
//!
//! Printed by `dodo --version` / `dodo --build-info` (see `main.rs`), which is
//! also how CI proves a freshly packaged binary actually executes.

/// The build metadata of this binary.
///
/// Field-for-field the struct the release specification asks for; the one
/// addition, [`GIT_COMMIT_SHORT`], sits outside it to keep that shape.
pub struct VersionInfo {
    pub version: &'static str,
    pub git_commit: &'static str,
    pub git_branch: &'static str,
    pub git_tag: &'static str,
    pub build_time: &'static str,
    pub target: &'static str,
    pub rust_version: &'static str,
}

/// This binary's metadata.
pub const VERSION_INFO: VersionInfo = VersionInfo {
    version: env!("CARGO_PKG_VERSION"),
    git_commit: env!("DODO_GIT_COMMIT"),
    git_branch: env!("DODO_GIT_BRANCH"),
    git_tag: env!("DODO_GIT_TAG"),
    build_time: env!("DODO_BUILD_TIME"),
    target: env!("DODO_TARGET"),
    rust_version: env!("DODO_RUST_VERSION"),
};

/// The commit abbreviated to 8 hex digits, keeping any `-dirty` marker.
pub const GIT_COMMIT_SHORT: &str = env!("DODO_GIT_COMMIT_SHORT");

/// The cargo features this binary was compiled with, for `--build-info`.
///
/// Hand-maintained: cargo exposes features to the crate only as `cfg` flags,
/// with no way to enumerate them. One line per feature in `Cargo.toml`.
const FEATURES: &[(&str, bool)] = &[("syntax-highlighting", cfg!(feature = "syntax-highlighting"))];

impl VersionInfo {
    /// One line, the shape `--version` prints and other tools parse:
    /// `dodo 0.1.0 (9f88c69a 2026-07-24T09:12:33Z)`.
    pub fn short(&self) -> String {
        format!(
            "dodo {} ({} {})",
            self.version, GIT_COMMIT_SHORT, self.build_time
        )
    }

    /// The full block `--build-info` prints, one `key: value` per line so a
    /// verification script can grep it without parsing.
    pub fn long(&self) -> String {
        let enabled: Vec<&str> = FEATURES
            .iter()
            .filter(|(_, on)| *on)
            .map(|(name, _)| *name)
            .collect();
        let features = if enabled.is_empty() {
            "none".to_string()
        } else {
            enabled.join(",")
        };
        // `debug_assertions` is the honest, always-correct signal here: a
        // custom profile such as `dist` still reports `PROFILE=release` to the
        // build script, so the build script cannot name the profile reliably.
        let optimization = if cfg!(debug_assertions) {
            "debug"
        } else {
            "release"
        };

        [
            "name:         dodo".to_string(),
            format!("version:      {}", self.version),
            format!("commit:       {}", self.git_commit),
            format!("branch:       {}", self.git_branch),
            format!("tag:          {}", self.git_tag),
            format!("build_time:   {}", self.build_time),
            format!("target:       {}", self.target),
            format!("rust_version: {}", self.rust_version),
            format!("optimization: {optimization}"),
            format!("features:     {features}"),
        ]
        .join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::{GIT_COMMIT_SHORT, VERSION_INFO};

    /// Every field must be populated with *something*. `build.rs` is written
    /// never to fail the build, so an empty value here would mean it silently
    /// emitted nothing and `--version` would print a hole.
    #[test]
    fn every_field_is_populated() {
        for (name, value) in [
            ("version", VERSION_INFO.version),
            ("git_commit", VERSION_INFO.git_commit),
            ("git_branch", VERSION_INFO.git_branch),
            ("git_tag", VERSION_INFO.git_tag),
            ("build_time", VERSION_INFO.build_time),
            ("target", VERSION_INFO.target),
            ("rust_version", VERSION_INFO.rust_version),
            ("git_commit_short", GIT_COMMIT_SHORT),
        ] {
            assert!(!value.trim().is_empty(), "build metadata `{name}` is empty");
        }
    }

    /// The version string is the package version, which the release workflow
    /// derives the tag from; a mismatch would ship `dodo-v0.0.0-*` archives.
    #[test]
    fn version_matches_package_version() {
        assert_eq!(VERSION_INFO.version, env!("CARGO_PKG_VERSION"));
    }

    /// `build_time` is what `SOURCE_DATE_EPOCH` pins for reproducible builds;
    /// it has to stay in the ISO 8601 UTC shape release notes and verification
    /// expect, or `unknown` when the clock was unreadable.
    #[test]
    fn build_time_is_iso8601_utc() {
        let t = VERSION_INFO.build_time;
        if t == "unknown" {
            return;
        }
        assert_eq!(t.len(), 20, "expected YYYY-MM-DDTHH:MM:SSZ, got {t}");
        assert!(t.ends_with('Z'), "{t} is not UTC-suffixed");
        assert_eq!(t.as_bytes()[10], b'T', "{t} has no date/time separator");
        assert!(
            t.chars()
                .all(|c| c.is_ascii_digit() || matches!(c, '-' | ':' | 'T' | 'Z')),
            "{t} has unexpected characters"
        );
    }

    /// The rendered lines are what a human reads and what CI greps.
    #[test]
    fn short_and_long_render_the_metadata() {
        let short = VERSION_INFO.short();
        assert!(short.starts_with("dodo "), "{short}");
        assert!(short.contains(VERSION_INFO.version), "{short}");

        let long = VERSION_INFO.long();
        for key in [
            "version:",
            "commit:",
            "branch:",
            "tag:",
            "build_time:",
            "target:",
            "rust_version:",
        ] {
            assert!(
                long.contains(key),
                "`--build-info` output is missing `{key}`:\n{long}"
            );
        }
    }
}
