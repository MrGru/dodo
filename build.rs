//! Embeds release metadata (git revision, build time, target, toolchain) into
//! the binary, where `src/build_info.rs` reads it back out with `env!`.
//!
//! Two rules shape everything here:
//!
//! 1. **It never fails the build.** `git` may be missing, `.git` may not exist
//!    (a `cargo package` tarball, a Docker `COPY .`), `rustc -vV` may not be
//!    runnable. Every lookup degrades to a placeholder instead of panicking, so
//!    a source-only build of dodo still compiles and still runs.
//! 2. **CI wins over the local checkout.** GitHub Actions builds from a
//!    detached HEAD in a shallow clone, where `git rev-parse --abbrev-ref HEAD`
//!    says `HEAD` and `git describe` sees no tags. The `GITHUB_*` environment
//!    variables carry the truth in that case, so they are consulted first.
//!
//! Two placeholder values, deliberately distinct:
//! `unknown` means "could not be determined" (no git, no metadata), `none`
//! means "determined, and the answer is that there is none" (a commit that
//! carries no tag). Release verification checks for the former.

use std::{
    env,
    path::Path,
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

fn main() {
    // Emitting any `rerun-if-*` key replaces cargo's default "rerun when any
    // tracked file changes" heuristic with exactly this list. That is what we
    // want: none of this metadata depends on `src/`, so a source edit should
    // not re-run the script (and should not churn `build_time`). A commit, a
    // checkout, or a change to one of the CI variables does.
    //
    // The trade-off, worth knowing before trusting the `-dirty` marker: editing
    // a tracked file does NOT re-run this script, so a local incremental build
    // can still report the clean commit it was last stamped with. It is
    // accurate in CI, which always builds from a fresh checkout, and after any
    // commit or checkout locally. `touch build.rs` forces a re-stamp.
    println!("cargo:rerun-if-changed=build.rs");
    for path in git_watch_paths() {
        println!("cargo:rerun-if-changed={path}");
    }
    for var in [
        "SOURCE_DATE_EPOCH",
        "GITHUB_SHA",
        "GITHUB_REF_NAME",
        "GITHUB_REF_TYPE",
        "GITHUB_HEAD_REF",
    ] {
        println!("cargo:rerun-if-env-changed={var}");
    }

    let commit = git_commit();
    emit("DODO_GIT_COMMIT_SHORT", short_commit(&commit));
    emit("DODO_GIT_COMMIT", commit);
    emit("DODO_GIT_BRANCH", git_branch());
    emit("DODO_GIT_TAG", git_tag());
    emit("DODO_BUILD_TIME", build_time());
    emit(
        "DODO_TARGET",
        env::var("TARGET").unwrap_or_else(|_| UNKNOWN.into()),
    );
    emit("DODO_RUST_VERSION", rust_version());
}

const UNKNOWN: &str = "unknown";
const NONE: &str = "none";

fn emit(key: &str, value: String) {
    println!("cargo:rustc-env={key}={value}");
}

/// `.git/HEAD` plus the file HEAD points at, so a commit or a branch switch
/// re-runs this script. Resolved through `git rev-parse --git-path`, because in
/// a worktree `.git` is a file and the real HEAD lives elsewhere.
fn git_watch_paths() -> Vec<String> {
    let Some(head) = git(&["rev-parse", "--git-path", "HEAD"]) else {
        return Vec::new();
    };
    let mut paths = vec![head.clone()];
    // The ref HEAD is attached to, if any; on a detached HEAD there is none.
    if let Some(ref_name) = git(&["symbolic-ref", "-q", "HEAD"])
        && let Some(ref_path) = git(&["rev-parse", "--git-path", &ref_name])
        && Path::new(&ref_path).exists()
    {
        paths.push(ref_path);
    }
    paths
}

fn git_commit() -> String {
    // A dirty tree is worth surfacing: it is the difference between "this
    // binary is commit abc123" and "this binary is commit abc123 plus whatever
    // was on someone's disk". Release verification refuses `-dirty`.
    let commit = env::var("GITHUB_SHA")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| git(&["rev-parse", "HEAD"]))
        .unwrap_or_else(|| UNKNOWN.into());
    if commit != UNKNOWN && is_dirty() {
        format!("{commit}-dirty")
    } else {
        commit
    }
}

fn short_commit(commit: &str) -> String {
    match commit.split_once('-') {
        Some((sha, suffix)) => format!("{}-{suffix}", short_sha(sha)),
        None => short_sha(commit).to_string(),
    }
}

fn short_sha(sha: &str) -> &str {
    if sha.len() >= 8 && sha.chars().all(|c| c.is_ascii_hexdigit()) {
        &sha[..8]
    } else {
        sha
    }
}

fn is_dirty() -> bool {
    // `--porcelain` prints one line per modified/untracked path and nothing at
    // all for a clean tree. No git means "not known to be dirty".
    git(&["status", "--porcelain", "--untracked-files=no"]).is_some_and(|out| !out.is_empty())
}

fn git_branch() -> String {
    // On a tag build `GITHUB_REF_TYPE` is `tag` and `GITHUB_REF_NAME` is the
    // tag, so it must not be read as a branch. Pull requests build a detached
    // merge commit and expose the source branch as `GITHUB_HEAD_REF`.
    if env::var("GITHUB_REF_TYPE").as_deref() == Ok("branch")
        && let Ok(name) = env::var("GITHUB_REF_NAME")
        && !name.is_empty()
    {
        return name;
    }
    if let Ok(head_ref) = env::var("GITHUB_HEAD_REF")
        && !head_ref.is_empty()
    {
        return head_ref;
    }
    git(&["rev-parse", "--abbrev-ref", "HEAD"])
        .filter(|b| b != "HEAD") // detached: not a branch
        .unwrap_or_else(|| UNKNOWN.into())
}

fn git_tag() -> String {
    if env::var("GITHUB_REF_TYPE").as_deref() == Ok("tag")
        && let Ok(name) = env::var("GITHUB_REF_NAME")
        && !name.is_empty()
    {
        return name;
    }
    // `--exact-match` so a build three commits past `v0.1.0` reports no tag
    // rather than claiming to be a release build of `v0.1.0`.
    match git(&["describe", "--tags", "--exact-match"]) {
        Some(tag) => tag,
        // Distinguish "git works, this commit is untagged" from "no git".
        None if git(&["rev-parse", "HEAD"]).is_some() => NONE.into(),
        None => UNKNOWN.into(),
    }
}

/// ISO 8601 UTC, honouring `SOURCE_DATE_EPOCH`.
///
/// A wall-clock timestamp is the one thing in this file that makes two builds
/// of the same commit differ byte for byte. `SOURCE_DATE_EPOCH` is the
/// cross-ecosystem convention for pinning it (reproducible-builds.org); the
/// release workflow sets it to the tagged commit's committer date, so a rebuild
/// of a release reproduces the same string. Local builds get "now".
fn build_time() -> String {
    let epoch = env::var("SOURCE_DATE_EPOCH")
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
        .or_else(|| {
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .ok()
                .map(|d| d.as_secs())
        });
    match epoch {
        Some(secs) => format_utc(secs),
        None => UNKNOWN.into(),
    }
}

/// Unix seconds to `YYYY-MM-DDTHH:MM:SSZ`, without pulling in a date crate for
/// one string. Civil-from-days after Howard Hinnant's `chrono`-compatible
/// algorithm; correct for every date after 1970 and needs no leap-second table
/// because Unix time has none.
fn format_utc(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let time_of_day = secs % 86_400;
    let (hour, minute, second) = (
        time_of_day / 3600,
        (time_of_day / 60) % 60,
        time_of_day % 60,
    );

    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = if month <= 2 { y + 1 } else { y };

    format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// The compiler that produced this binary, e.g. `1.96.0 (ac68faa20 2026-05-25)`.
/// `RUSTC` is set by cargo, so this follows a `+toolchain` override or a
/// non-default `rustup` default rather than whatever is first on `PATH`.
fn rust_version() -> String {
    let rustc = env::var("RUSTC").unwrap_or_else(|_| "rustc".into());
    let Ok(out) = Command::new(rustc).arg("-vV").output() else {
        return UNKNOWN.into();
    };
    if !out.status.success() {
        return UNKNOWN.into();
    }
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .next()
        .and_then(|line| line.strip_prefix("rustc "))
        .map(|v| v.trim().to_string())
        .unwrap_or_else(|| UNKNOWN.into())
}

/// Runs `git` in the manifest directory, returning trimmed stdout on success
/// and `None` on any failure — missing binary, not a repository, empty output.
fn git(args: &[&str]) -> Option<String> {
    let dir = env::var("CARGO_MANIFEST_DIR").ok()?;
    let out = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let value = String::from_utf8(out.stdout).ok()?.trim().to_string();
    (!value.is_empty()).then_some(value)
}
