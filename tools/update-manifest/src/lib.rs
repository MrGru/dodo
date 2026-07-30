//! Generates the `update.json` manifest and the combined `SHA256SUMS` for a
//! dodo release.
//!
//! This crate is **release engineering only**. It is excluded from dodo's own
//! build by `exclude = ["tools/*"]` in the root `Cargo.toml`, so it contributes
//! zero bytes to the shipped binary and zero time to the app's lint and test
//! runs. Nothing here is ever linked into `dodo`.
//!
//! # Why it exists
//!
//! `.github/workflows/release.yml` builds four platforms, three of which are
//! `experimental: true` and therefore `continue-on-error: true`. The publish job
//! gates on `needs.build.result == 'success'`, which a `continue-on-error`
//! failure still satisfies — so before this tool existed the pipeline would
//! publish a release with a platform silently missing, and did: v0.1.5 shipped
//! eleven assets and no Windows archive at all, because the Windows *verify*
//! step failed and its upload step was skipped.
//!
//! [`run`] closes that hole. It is handed the full expected platform set on the
//! command line and **fails the release** if any of them has no artifact, so the
//! manifest can never describe a partial release. See `docs/release.md` for the
//! reasoning about experimental platforms.
//!
//! # The pipeline
//!
//! ```text
//! scan --dir  →  classify by exact filename  →  hash + sidecar cross-check
//!             →  select one archive per platform  →  write update.json
//!                                                 →  write SHA256SUMS
//! ```
//!
//! Every step can fail the run, and nothing is written until all of them have
//! passed: a partial manifest is worse than no manifest, because a client would
//! act on it.

pub mod args;
pub mod hash;
pub mod manifest;
pub mod platform;

#[cfg(test)]
mod testutil;

use manifest::{MANIFEST_VERSION, Manifest, ManifestFile};
use platform::{ExpectedArtifact, Platform, artifact_index};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub use args::Args;

/// Files the artifact directory may contain that are not release archives:
/// this tool's own outputs, so re-running it over the same directory is not an
/// error.
const GENERATED_FILES: [&str; 2] = ["SHA256SUMS", "update.json"];

/// One archive found on disk, with everything the outputs need.
#[derive(Debug, Clone, PartialEq, Eq)]
struct FoundArtifact {
    expected: ExpectedArtifact,
    path: PathBuf,
    sha256: String,
    size: u64,
}

/// What a successful run produced, for the caller to log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Summary {
    /// Every archive that went into `SHA256SUMS`, in file order.
    pub hashed: Vec<String>,
    /// Platform key → the archive the manifest points at.
    pub manifest_entries: BTreeMap<String, String>,
}

/// Runs the whole generator. Writes nothing unless every check passes.
pub fn run(args: &Args) -> Result<Summary, String> {
    let notes = std::fs::read_to_string(&args.notes_file).map_err(|e| {
        format!(
            "cannot read the release notes file {}: {e}",
            args.notes_file.display()
        )
    })?;

    let found = scan(&args.dir, &args.version)?;

    // Manifest selection, by name: macOS takes the `-app` bundle, everything
    // else takes its single archive. See `Platform::manifest_kind`.
    let mut files = BTreeMap::new();
    let mut manifest_entries = BTreeMap::new();
    for platform in Platform::ALL {
        let kind = platform.manifest_kind();
        let Some(artifact) = found
            .iter()
            .find(|a| a.expected.platform == platform && a.expected.kind == kind)
        else {
            continue;
        };

        let name = &artifact.expected.file_name;
        files.insert(
            platform.key().to_string(),
            ManifestFile {
                url: format!(
                    "https://github.com/{}/releases/download/{}/{name}",
                    args.repo, args.tag
                ),
                sha256: artifact.sha256.clone(),
                size: artifact.size,
                signature: None,
            },
        );
        manifest_entries.insert(platform.key().to_string(), name.clone());
    }

    // The gate. Checked after scanning so the error can name the exact filename
    // that was looked for, and before any file is written so a failed run leaves
    // nothing behind.
    let missing: Vec<String> = args
        .expect_platforms
        .iter()
        .filter(|platform| !files.contains_key(platform.key()))
        .map(|platform| describe_missing(*platform, &args.version, &found))
        .collect();
    if !missing.is_empty() {
        return Err(format!(
            "the release is missing {} of {} expected platform(s):\n{}\n\
             Refusing to write a manifest that describes a partial release — a client \
             reading it would never be offered an update for the missing platform(s).",
            missing.len(),
            args.expect_platforms.len(),
            missing.join("\n")
        ));
    }

    let document = Manifest {
        manifest_version: MANIFEST_VERSION,
        channel: args.channel,
        version: args.version.clone(),
        notes,
        published_at: args.published_at.clone(),
        files,
    };

    write_file(&args.out, &document.to_json()?)?;
    write_file(&args.sums_out, &sha256sums(&found))?;

    Ok(Summary {
        hashed: found.iter().map(|a| a.expected.file_name.clone()).collect(),
        manifest_entries,
    })
}

/// Builds the "platform X is missing" line, naming the file that was looked for
/// and, when it applies, the near miss that was found instead.
fn describe_missing(platform: Platform, version: &str, found: &[FoundArtifact]) -> String {
    let kind = platform.manifest_kind();
    let wanted = artifact_index(version)
        .into_values()
        .find(|a| a.platform == platform && a.kind == kind)
        .map(|a| a.file_name)
        .unwrap_or_else(|| format!("<no archive defined for {platform}>"));

    let others: Vec<&str> = found
        .iter()
        .filter(|a| a.expected.platform == platform)
        .map(|a| a.expected.file_name.as_str())
        .collect();

    if others.is_empty() {
        format!("  - {platform}: no artifact at all (expected {wanted})")
    } else {
        // The case that matters on macOS: the bare binary arrived but the .app
        // bundle did not. Falling back to it would hand the updater something
        // it cannot install.
        format!(
            "  - {platform}: missing the {kind} `{wanted}`; found only {}",
            others.join(", ")
        )
    }
}

/// Reads the artifact directory and hashes everything in it.
///
/// Every file must be one of: a release archive for this exact version, that
/// archive's `.sha256` sidecar, or one of this tool's own outputs. Anything else
/// fails the run — including a plausible-looking archive from a *different*
/// version, which is exactly the kind of leftover that should stop a release.
fn scan(dir: &Path, version: &str) -> Result<Vec<FoundArtifact>, String> {
    let index = artifact_index(version);

    let entries = std::fs::read_dir(dir)
        .map_err(|e| format!("cannot read the artifact directory {}: {e}", dir.display()))?;

    let mut archives: Vec<(String, PathBuf)> = Vec::new();
    let mut unmapped: Vec<String> = Vec::new();

    for entry in entries {
        let entry = entry
            .map_err(|e| format!("cannot list the artifact directory {}: {e}", dir.display()))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|e| format!("cannot stat {}: {e}", path.display()))?;

        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            unmapped.push(path.display().to_string());
            continue;
        };

        if file_type.is_dir() {
            return Err(format!(
                "{} contains a subdirectory `{name}`; the artifact directory must be flat. \
                 If this came from actions/download-artifact, set `merge-multiple: true`.",
                dir.display()
            ));
        }

        if GENERATED_FILES.contains(&name) {
            continue;
        }
        if let Some(stem) = name.strip_suffix(".sha256") {
            // A sidecar is valid exactly when its archive is. `verify_against_
            // sidecar` is what actually reads it.
            if index.contains_key(stem) {
                continue;
            }
            unmapped.push(name.to_string());
            continue;
        }
        if index.contains_key(name) {
            archives.push((name.to_string(), path));
            continue;
        }
        unmapped.push(name.to_string());
    }

    if !unmapped.is_empty() {
        unmapped.sort();
        let known: Vec<&str> = index.keys().map(String::as_str).collect();
        return Err(format!(
            "{} unrecognised file(s) in {}:\n{}\n\
             A release of version {version} may contain only these archives (plus their \
             `.sha256` sidecars):\n{}\n\
             Refusing to publish: an unrecognised file is usually a leftover from another \
             version or a packaging change this tool has not been taught about.",
            unmapped.len(),
            dir.display(),
            unmapped
                .iter()
                .map(|n| format!("  - {n}"))
                .collect::<Vec<_>>()
                .join("\n"),
            known
                .iter()
                .map(|n| format!("  - {n}"))
                .collect::<Vec<_>>()
                .join("\n")
        ));
    }

    // Sorted so SHA256SUMS and the log are byte-identical between runs.
    archives.sort();

    let mut found = Vec::with_capacity(archives.len());
    for (name, path) in archives {
        let sha256 = hash::verify_against_sidecar(&path)?;
        let size = std::fs::metadata(&path)
            .map_err(|e| format!("cannot stat {}: {e}", path.display()))?
            .len();
        let Some(expected) = index.get(&name).cloned() else {
            // Unreachable: `name` came out of `index` above. Handled rather than
            // unwrapped so this file stays free of `.unwrap()`.
            return Err(format!(
                "internal error: {name} vanished from the name table"
            ));
        };
        found.push(FoundArtifact {
            expected,
            path,
            sha256,
            size,
        });
    }

    Ok(found)
}

/// The combined checksum file, covering **every** archive in the release — not
/// only the ones the manifest points at.
///
/// The bare macOS tarballs are published assets too, and a user running
/// `sha256sum -c SHA256SUMS` expects a line for everything they can download.
fn sha256sums(found: &[FoundArtifact]) -> String {
    let mut out = String::new();
    for artifact in found {
        // Two spaces: the layout `sha256sum -c` and `shasum -a 256 -c` read.
        out.push_str(&artifact.sha256);
        out.push_str("  ");
        out.push_str(&artifact.expected.file_name);
        out.push('\n');
    }
    out
}

fn write_file(path: &Path, contents: &str) -> Result<(), String> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
    }
    std::fs::write(path, contents).map_err(|e| format!("cannot write {}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::Channel;
    use crate::testutil::TempDir;

    const VERSION: &str = "0.2.0";

    /// The layout the workflow actually has: a flat `artifacts/` directory that
    /// is all the tool scans, and a separate working directory holding the notes
    /// file and the two outputs. Keeping them apart in the fixture matters —
    /// putting the notes file inside `artifacts/` makes it an unrecognised file,
    /// which is correct behaviour and was how this fixture first went wrong.
    struct Release {
        root: TempDir,
    }

    impl Release {
        fn new(label: &str) -> Release {
            let root = TempDir::new(label);
            std::fs::create_dir_all(root.path().join("artifacts")).expect("creates artifacts");
            std::fs::create_dir_all(root.path().join("work")).expect("creates work");
            Release { root }
        }

        fn artifacts(&self) -> PathBuf {
            self.root.path().join("artifacts")
        }

        fn work(&self) -> PathBuf {
            self.root.path().join("work")
        }

        /// Writes an archive plus a correct sidecar, the way
        /// `scripts/package.sh` does.
        fn place(&self, name: &str, body: &[u8]) {
            let path = self.artifacts().join(name);
            std::fs::write(&path, body).expect("writes archive");
            let digest = hash::sha256_file(&path).expect("hashes");
            std::fs::write(
                self.artifacts().join(format!("{name}.sha256")),
                format!("{digest}  {name}\n"),
            )
            .expect("writes sidecar");
        }

        fn write_artifact(&self, name: &str, body: &[u8]) {
            std::fs::write(self.artifacts().join(name), body).expect("writes file");
        }

        fn remove_artifact(&self, name: &str) {
            std::fs::remove_file(self.artifacts().join(name)).expect("removes file");
        }

        /// Everything a green four-platform release produces.
        fn all_four(&self) {
            self.place(
                &format!("dodo-v{VERSION}-macos-arm64.tar.gz"),
                b"bare arm64",
            );
            self.place(
                &format!("dodo-v{VERSION}-macos-arm64-app.tar.gz"),
                b"app arm64",
            );
            self.place(&format!("dodo-v{VERSION}-macos-x64.tar.gz"), b"bare x64");
            self.place(&format!("dodo-v{VERSION}-macos-x64-app.tar.gz"), b"app x64");
            self.place(&format!("dodo-v{VERSION}-linux-x64.tar.gz"), b"linux");
            self.place(&format!("dodo-v{VERSION}-windows-x64.zip"), b"windows");
        }

        fn args(&self, expect: &[Platform]) -> Args {
            std::fs::write(self.work().join("notes.md"), b"## dodo v0.2.0\n")
                .expect("writes notes");
            Args {
                version: VERSION.to_string(),
                channel: Channel::Stable,
                dir: self.artifacts(),
                repo: "MrGru/dodo".to_string(),
                tag: format!("v{VERSION}"),
                notes_file: self.work().join("notes.md"),
                published_at: "2026-07-30T12:11:03Z".to_string(),
                out: self.work().join("update.json"),
                sums_out: self.work().join("SHA256SUMS"),
                expect_platforms: expect.to_vec(),
            }
        }
    }

    fn read_manifest(args: &Args) -> serde_json::Value {
        let text = std::fs::read_to_string(&args.out).expect("manifest written");
        serde_json::from_str(&text).expect("valid JSON")
    }

    #[test]
    fn a_complete_release_produces_a_manifest_for_all_four_platforms() {
        let release = Release::new("complete");
        release.all_four();
        let args = release.args(&Platform::ALL);

        let summary = run(&args).expect("should succeed");
        assert_eq!(summary.manifest_entries.len(), 4);

        let value = read_manifest(&args);
        assert_eq!(value["manifest_version"], 1);
        assert_eq!(value["channel"], "stable");
        assert_eq!(value["version"], VERSION);
        assert_eq!(value["notes"], "## dodo v0.2.0\n");
        assert_eq!(value["published_at"], "2026-07-30T12:11:03Z");
        for platform in Platform::ALL {
            assert!(
                value["files"][platform.key()].is_object(),
                "missing {platform}"
            );
            assert!(value["files"][platform.key()]["signature"].is_null());
        }
    }

    /// The requirement this test exists for: the manifest must point at the
    /// `.app` bundle, never at the bare binary, and it must do so by name rather
    /// than by whatever `read_dir` happened to yield first.
    #[test]
    fn macos_entries_point_at_the_app_bundle_and_never_the_bare_binary() {
        let release = Release::new("app-selection");
        release.all_four();
        let args = release.args(&Platform::ALL);
        run(&args).expect("should succeed");

        let value = read_manifest(&args);
        for platform in [Platform::MacosArm64, Platform::MacosX64] {
            let url = value["files"][platform.key()]["url"]
                .as_str()
                .expect("url is a string")
                .to_string();
            assert!(
                url.ends_with(&format!("dodo-v{VERSION}-{platform}-app.tar.gz")),
                "{platform} should point at the bundle, got {url}"
            );
            assert!(
                !url.ends_with(&format!("dodo-v{VERSION}-{platform}.tar.gz")),
                "{platform} must never point at the bare binary, got {url}"
            );
        }
    }

    /// The same requirement from the other side: the bare archive's *content*
    /// must not be what the manifest describes. Distinct bodies make the two
    /// digests distinguishable, so this cannot pass by coincidence.
    #[test]
    fn the_macos_digest_is_the_bundles_not_the_bare_binarys() {
        let release = Release::new("app-digest");
        release.all_four();
        let args = release.args(&Platform::ALL);
        run(&args).expect("should succeed");

        let bare = hash::sha256_file(
            &release
                .artifacts()
                .join(format!("dodo-v{VERSION}-macos-arm64.tar.gz")),
        )
        .expect("hashes");
        let app = hash::sha256_file(
            &release
                .artifacts()
                .join(format!("dodo-v{VERSION}-macos-arm64-app.tar.gz")),
        )
        .expect("hashes");
        assert_ne!(bare, app, "fixture must give the archives different bodies");

        let value = read_manifest(&args);
        assert_eq!(value["files"]["macos-arm64"]["sha256"], app);
        assert_ne!(value["files"]["macos-arm64"]["sha256"], bare);
    }

    /// A macOS platform whose bundle is absent must fail even though its bare
    /// binary is right there — the tempting silent fallback.
    #[test]
    fn a_missing_app_bundle_is_not_satisfied_by_the_bare_binary() {
        let release = Release::new("app-missing");
        release.all_four();
        release.remove_artifact(&format!("dodo-v{VERSION}-macos-arm64-app.tar.gz"));
        release.remove_artifact(&format!("dodo-v{VERSION}-macos-arm64-app.tar.gz.sha256"));

        let args = release.args(&Platform::ALL);
        let err = run(&args).expect_err("should reject");
        assert!(err.contains("macos-arm64"), "{err}");
        assert!(err.contains("app bundle"), "{err}");
        assert!(err.contains("found only"), "{err}");
        assert!(
            err.contains(&format!("dodo-v{VERSION}-macos-arm64.tar.gz")),
            "the error should name the bare archive it found: {err}"
        );
        assert!(!args.out.exists(), "no manifest may be written");
    }

    /// The v0.1.5 case: three platforms built, Windows never uploaded.
    #[test]
    fn a_missing_platform_fails_the_run_and_writes_nothing() {
        let release = Release::new("missing-windows");
        release.all_four();
        release.remove_artifact(&format!("dodo-v{VERSION}-windows-x64.zip"));
        release.remove_artifact(&format!("dodo-v{VERSION}-windows-x64.zip.sha256"));

        let args = release.args(&Platform::ALL);
        let err = run(&args).expect_err("should reject");
        assert!(err.contains("windows-x64"), "{err}");
        assert!(err.contains("no artifact at all"), "{err}");
        assert!(err.contains("dodo-v0.2.0-windows-x64.zip"), "{err}");
        assert!(!args.out.exists(), "no manifest may be written");
        assert!(!args.sums_out.exists(), "no sums may be written");
    }

    /// Not expecting a platform is how a deliberately partial manifest would be
    /// produced. The workflow never does this — it passes all four — but the
    /// tool has to behave predictably when it happens.
    #[test]
    fn a_platform_that_is_absent_and_unexpected_is_simply_omitted() {
        let release = Release::new("unexpected-absent");
        release.all_four();
        release.remove_artifact(&format!("dodo-v{VERSION}-windows-x64.zip"));
        release.remove_artifact(&format!("dodo-v{VERSION}-windows-x64.zip.sha256"));

        let args = release.args(&[Platform::MacosArm64, Platform::MacosX64, Platform::LinuxX64]);
        run(&args).expect("should succeed");
        let value = read_manifest(&args);
        assert!(value["files"]["windows-x64"].is_null());
        assert!(value["files"]["linux-x64"].is_object());
    }

    #[test]
    fn an_unmapped_file_fails_the_run() {
        let release = Release::new("unmapped");
        release.all_four();
        release.write_artifact("dodo-v0.2.0-freebsd-x64.tar.gz", b"surprise");

        let args = release.args(&Platform::ALL);
        let err = run(&args).expect_err("should reject");
        assert!(err.contains("unrecognised file"), "{err}");
        assert!(err.contains("dodo-v0.2.0-freebsd-x64.tar.gz"), "{err}");
        assert!(!args.out.exists());
    }

    /// A leftover archive from a previous version is the realistic version of
    /// the above, and a loose filename parse would have accepted it as
    /// `linux-x64`.
    #[test]
    fn an_archive_from_another_version_fails_the_run() {
        let release = Release::new("stale-version");
        release.all_four();
        release.place("dodo-v0.1.9-linux-x64.tar.gz", b"stale");

        let args = release.args(&Platform::ALL);
        let err = run(&args).expect_err("should reject");
        assert!(err.contains("dodo-v0.1.9-linux-x64.tar.gz"), "{err}");
        assert!(!args.out.exists());
    }

    /// An orphan sidecar names no archive this release contains.
    #[test]
    fn an_unmatched_sidecar_fails_the_run() {
        let release = Release::new("orphan-sidecar");
        release.all_four();
        release.write_artifact("dodo-v0.1.9-linux-x64.tar.gz.sha256", b"whatever\n");

        let args = release.args(&Platform::ALL);
        let err = run(&args).expect_err("should reject");
        assert!(err.contains("dodo-v0.1.9-linux-x64.tar.gz.sha256"), "{err}");
    }

    #[test]
    fn a_sidecar_that_disagrees_fails_the_run() {
        let release = Release::new("sidecar-mismatch-e2e");
        release.all_four();
        release.write_artifact(
            &format!("dodo-v{VERSION}-linux-x64.tar.gz.sha256"),
            format!(
                "0000000000000000000000000000000000000000000000000000000000000000  \
                 dodo-v{VERSION}-linux-x64.tar.gz\n"
            )
            .as_bytes(),
        );

        let args = release.args(&Platform::ALL);
        let err = run(&args).expect_err("should reject");
        assert!(err.contains("checksum mismatch"), "{err}");
        assert!(err.contains("dodo-v0.2.0-linux-x64.tar.gz"), "{err}");
        assert!(!args.out.exists());
    }

    /// SHA256SUMS covers every published archive, including the bare macOS
    /// tarballs the manifest deliberately does not point at.
    #[test]
    fn sha256sums_covers_every_archive_not_only_the_manifest_entries() {
        let release = Release::new("sums-complete");
        release.all_four();
        let args = release.args(&Platform::ALL);
        run(&args).expect("should succeed");

        let sums = std::fs::read_to_string(&args.sums_out).expect("sums written");
        let names: Vec<&str> = sums
            .lines()
            .filter_map(|line| line.split_whitespace().nth(1))
            .collect();
        assert_eq!(
            names,
            vec![
                "dodo-v0.2.0-linux-x64.tar.gz",
                "dodo-v0.2.0-macos-arm64-app.tar.gz",
                "dodo-v0.2.0-macos-arm64.tar.gz",
                "dodo-v0.2.0-macos-x64-app.tar.gz",
                "dodo-v0.2.0-macos-x64.tar.gz",
                "dodo-v0.2.0-windows-x64.zip",
            ]
        );
        for line in sums.lines() {
            let digest = line.split_whitespace().next().unwrap_or("");
            assert_eq!(digest.len(), 64, "bad digest in `{line}`");
        }
    }

    /// The combined file must be exactly what `sha256sum -c` reads: two spaces,
    /// one line per archive, trailing newline.
    #[test]
    fn sha256sums_uses_the_two_space_layout() {
        let release = Release::new("sums-layout");
        release.all_four();
        let args = release.args(&Platform::ALL);
        run(&args).expect("should succeed");

        let sums = std::fs::read_to_string(&args.sums_out).expect("sums written");
        assert!(sums.ends_with('\n'));
        for line in sums.lines() {
            let (digest, name) = line.split_at(64);
            assert!(digest.chars().all(|c| c.is_ascii_hexdigit()), "{line}");
            assert!(name.starts_with("  "), "expected two spaces in `{line}`");
            assert!(!name[2..].starts_with(' '), "{line}");
        }
    }

    #[test]
    fn sizes_are_the_exact_byte_counts() {
        let release = Release::new("sizes");
        release.all_four();
        let args = release.args(&Platform::ALL);
        run(&args).expect("should succeed");

        let value = read_manifest(&args);
        assert_eq!(value["files"]["linux-x64"]["size"], "linux".len());
        assert_eq!(value["files"]["macos-arm64"]["size"], "app arm64".len());
    }

    #[test]
    fn urls_are_built_from_repo_and_tag() {
        let release = Release::new("urls");
        release.all_four();
        let args = release.args(&Platform::ALL);
        run(&args).expect("should succeed");

        assert_eq!(
            read_manifest(&args)["files"]["windows-x64"]["url"],
            "https://github.com/MrGru/dodo/releases/download/v0.2.0/dodo-v0.2.0-windows-x64.zip"
        );
    }

    /// Re-running over a directory that already holds the previous outputs must
    /// work: the publish job is re-runnable by design, so this tool has to be
    /// too.
    #[test]
    fn its_own_outputs_are_not_unmapped_files() {
        let release = Release::new("rerun");
        release.all_four();
        release.write_artifact("SHA256SUMS", b"stale\n");
        release.write_artifact("update.json", b"{}\n");

        let args = release.args(&Platform::ALL);
        run(&args).expect("should succeed");
    }

    /// Running twice must produce byte-identical output — the scan is sorted
    /// precisely so a re-run cannot reorder the manifest.
    #[test]
    fn two_runs_produce_identical_output() {
        let release = Release::new("deterministic");
        release.all_four();
        let args = release.args(&Platform::ALL);

        run(&args).expect("first run");
        let first_manifest = std::fs::read_to_string(&args.out).expect("read");
        let first_sums = std::fs::read_to_string(&args.sums_out).expect("read");

        run(&args).expect("second run");
        assert_eq!(
            std::fs::read_to_string(&args.out).expect("read"),
            first_manifest
        );
        assert_eq!(
            std::fs::read_to_string(&args.sums_out).expect("read"),
            first_sums
        );
    }

    #[test]
    fn a_subdirectory_is_reported_as_a_download_artifact_misconfiguration() {
        let release = Release::new("nested");
        release.all_four();
        std::fs::create_dir(release.artifacts().join("dodo-macos-arm64")).expect("creates");

        let args = release.args(&Platform::ALL);
        let err = run(&args).expect_err("should reject");
        assert!(err.contains("must be flat"), "{err}");
        assert!(err.contains("merge-multiple"), "{err}");
    }

    #[test]
    fn a_missing_notes_file_is_reported_by_path() {
        let release = Release::new("no-notes");
        release.all_four();
        let mut args = release.args(&Platform::ALL);
        args.notes_file = release.work().join("absent.md");

        let err = run(&args).expect_err("should reject");
        assert!(err.contains("release notes file"), "{err}");
        assert!(err.contains("absent.md"), "{err}");
    }

    #[test]
    fn a_missing_artifact_directory_is_reported_by_path() {
        let release = Release::new("no-dir");
        let mut args = release.args(&Platform::ALL);
        args.dir = release.work().join("nowhere");

        let err = run(&args).expect_err("should reject");
        assert!(err.contains("cannot read the artifact directory"), "{err}");
        assert!(err.contains("nowhere"), "{err}");
    }

    #[test]
    fn the_channel_reaches_the_document() {
        for channel in Channel::ALL {
            let release = Release::new(&format!("channel-{channel}"));
            release.all_four();
            let mut args = release.args(&Platform::ALL);
            args.channel = channel;
            run(&args).expect("should succeed");
            assert_eq!(read_manifest(&args)["channel"], channel.as_str());
        }
    }
}
