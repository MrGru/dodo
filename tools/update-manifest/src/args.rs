//! Command line parsing, by hand.
//!
//! Ten flags, all `--long`, all required except `--expect-platform` which
//! repeats. `clap` was in the first draft and was removed: it is a large
//! dependency tree for a tool whose entire interface is this file, and its
//! derive macros are a second place for the interface to drift from the
//! workflow that calls it.
//!
//! Both `--flag value` and `--flag=value` are accepted, because a human
//! debugging a failed release will type whichever they are used to.

use crate::manifest::Channel;
use crate::platform::Platform;
use std::path::PathBuf;

pub const USAGE: &str = "\
update-manifest — write update.json and SHA256SUMS for a dodo release

USAGE:
    update-manifest --version <semver> --channel <stable|beta|nightly>
                    --dir <artifacts/> --repo <owner/repo> --tag <vX.Y.Z>
                    --notes-file <path> --published-at <rfc3339>
                    --out <update.json> --sums-out <SHA256SUMS>
                    --expect-platform <key> [--expect-platform <key>]...

OPTIONS:
    --version           Release version, without the leading `v` (e.g. 0.2.0).
    --channel           Which release stream this manifest describes.
    --dir               Directory holding the downloaded release archives.
    --repo              GitHub `owner/repo`, used to build download URLs.
    --tag               Git tag the release is published under (e.g. v0.2.0).
    --notes-file        File whose contents become the manifest's `notes`.
    --published-at      RFC 3339 UTC timestamp (e.g. 2026-07-30T12:11:03Z).
    --out               Where to write update.json.
    --sums-out          Where to write the combined SHA256SUMS.
    --expect-platform   A platform key that must be present. Repeatable, and
                        at least one is required — a run with no expectations
                        could not fail for a missing platform, which is the
                        whole reason this tool exists.
";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    pub version: String,
    pub channel: Channel,
    pub dir: PathBuf,
    pub repo: String,
    pub tag: String,
    pub notes_file: PathBuf,
    pub published_at: String,
    pub out: PathBuf,
    pub sums_out: PathBuf,
    pub expect_platforms: Vec<Platform>,
}

/// Parses the argument list *after* the program name.
pub fn parse<I, S>(argv: I) -> Result<Args, String>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let argv: Vec<String> = argv.into_iter().map(Into::into).collect();

    let mut version = None;
    let mut channel = None;
    let mut dir = None;
    let mut repo = None;
    let mut tag = None;
    let mut notes_file = None;
    let mut published_at = None;
    let mut out = None;
    let mut sums_out = None;
    let mut expect_platforms: Vec<Platform> = Vec::new();

    let mut index = 0;
    while index < argv.len() {
        let arg = argv[index].clone();
        index += 1;

        if !arg.starts_with("--") {
            return Err(format!(
                "unexpected positional argument `{arg}`; every option is `--long`\n\n{USAGE}"
            ));
        }

        // `--flag=value` carries its own value; `--flag value` takes the next.
        let (flag, inline) = match arg.split_once('=') {
            Some((flag, value)) => (flag.to_string(), Some(value.to_string())),
            None => (arg.clone(), None),
        };

        let mut take_value = |flag: &str| -> Result<String, String> {
            if let Some(value) = inline.clone() {
                return Ok(value);
            }
            let value = argv
                .get(index)
                .cloned()
                .ok_or_else(|| format!("{flag} needs a value"))?;
            index += 1;
            Ok(value)
        };

        match flag.as_str() {
            "--version" => version = Some(take_value("--version")?),
            "--channel" => channel = Some(Channel::parse(&take_value("--channel")?)?),
            "--dir" => dir = Some(PathBuf::from(take_value("--dir")?)),
            "--repo" => repo = Some(take_value("--repo")?),
            "--tag" => tag = Some(take_value("--tag")?),
            "--notes-file" => notes_file = Some(PathBuf::from(take_value("--notes-file")?)),
            "--published-at" => published_at = Some(take_value("--published-at")?),
            "--out" => out = Some(PathBuf::from(take_value("--out")?)),
            "--sums-out" => sums_out = Some(PathBuf::from(take_value("--sums-out")?)),
            "--expect-platform" => {
                expect_platforms.push(Platform::parse(&take_value("--expect-platform")?)?);
            }
            "--help" | "-h" => return Err(USAGE.to_string()),
            other => return Err(format!("unknown option `{other}`\n\n{USAGE}")),
        }
    }

    fn required<T>(value: Option<T>, flag: &str) -> Result<T, String> {
        value.ok_or_else(|| format!("missing required option {flag}\n\n{USAGE}"))
    }

    let version = required(version, "--version")?;
    // Parsed only to reject a malformed version before anything is hashed; the
    // string form is what names the archives, so that is what is kept.
    semver::Version::parse(&version)
        .map_err(|e| format!("--version `{version}` is not valid semver: {e}"))?;

    let repo = required(repo, "--repo")?;
    if repo.split('/').count() != 2 || repo.split('/').any(str::is_empty) {
        return Err(format!("--repo `{repo}` is not in `owner/repo` form"));
    }

    let published_at = required(published_at, "--published-at")?;
    validate_rfc3339(&published_at)?;

    if expect_platforms.is_empty() {
        return Err(format!(
            "at least one --expect-platform is required\n\n{USAGE}"
        ));
    }
    expect_platforms.sort();
    expect_platforms.dedup();

    Ok(Args {
        version,
        channel: required(channel, "--channel")?,
        dir: required(dir, "--dir")?,
        repo,
        tag: required(tag, "--tag")?,
        notes_file: required(notes_file, "--notes-file")?,
        published_at,
        out: required(out, "--out")?,
        sums_out: required(sums_out, "--sums-out")?,
        expect_platforms,
    })
}

/// A structural check on the timestamp, not a full RFC 3339 parser.
///
/// The value is written into the manifest verbatim — the tool does not compute
/// it, GitHub does — so the job here is to catch an empty string or an obviously
/// wrong shape before it reaches a client, not to re-derive `chrono`.
fn validate_rfc3339(value: &str) -> Result<(), String> {
    let bytes = value.as_bytes();
    let shaped = bytes.len() >= 20
        && bytes[..4].iter().all(u8::is_ascii_digit)
        && bytes[4] == b'-'
        && bytes[5..7].iter().all(u8::is_ascii_digit)
        && bytes[7] == b'-'
        && bytes[8..10].iter().all(u8::is_ascii_digit)
        && (bytes[10] == b'T' || bytes[10] == b't')
        && bytes[11..13].iter().all(u8::is_ascii_digit)
        && bytes[13] == b':'
        && bytes[14..16].iter().all(u8::is_ascii_digit)
        && bytes[16] == b':'
        && bytes[17..19].iter().all(u8::is_ascii_digit);

    if !shaped {
        return Err(format!(
            "--published-at `{value}` is not an RFC 3339 timestamp \
             (expected something like 2026-07-30T12:11:03Z)"
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn full() -> Vec<String> {
        [
            "--version",
            "0.2.0",
            "--channel",
            "stable",
            "--dir",
            "artifacts",
            "--repo",
            "MrGru/dodo",
            "--tag",
            "v0.2.0",
            "--notes-file",
            "notes.md",
            "--published-at",
            "2026-07-30T12:11:03Z",
            "--out",
            "update.json",
            "--sums-out",
            "SHA256SUMS",
            "--expect-platform",
            "macos-arm64",
        ]
        .into_iter()
        .map(String::from)
        .collect()
    }

    #[test]
    fn parses_a_full_command_line() {
        let args = parse(full()).expect("parses");
        assert_eq!(args.version, "0.2.0");
        assert_eq!(args.channel, Channel::Stable);
        assert_eq!(args.repo, "MrGru/dodo");
        assert_eq!(args.tag, "v0.2.0");
        assert_eq!(args.published_at, "2026-07-30T12:11:03Z");
        assert_eq!(args.expect_platforms, vec![Platform::MacosArm64]);
    }

    #[test]
    fn accepts_equals_form() {
        let mut argv = full();
        argv[0] = "--version=0.2.0".to_string();
        argv.remove(1);
        assert_eq!(parse(argv).expect("parses").version, "0.2.0");
    }

    #[test]
    fn collects_repeated_expect_platform_and_dedupes() {
        let mut argv = full();
        argv.extend(["--expect-platform", "windows-x64"].map(String::from));
        argv.extend(["--expect-platform", "macos-arm64"].map(String::from));
        let args = parse(argv).expect("parses");
        assert_eq!(
            args.expect_platforms,
            vec![Platform::MacosArm64, Platform::WindowsX64]
        );
    }

    #[test]
    fn requires_at_least_one_expected_platform() {
        let argv: Vec<String> = full().into_iter().take(18).collect();
        let err = parse(argv).expect_err("should reject");
        assert!(err.contains("at least one --expect-platform"), "{err}");
    }

    #[test]
    fn names_a_missing_required_flag() {
        let argv: Vec<String> = full().into_iter().skip(2).collect();
        let err = parse(argv).expect_err("should reject");
        assert!(err.contains("--version"), "{err}");
    }

    #[test]
    fn rejects_a_non_semver_version() {
        let mut argv = full();
        argv[1] = "0.2".to_string();
        let err = parse(argv).expect_err("should reject");
        assert!(err.contains("not valid semver"), "{err}");
    }

    #[test]
    fn rejects_a_malformed_repo() {
        let mut argv = full();
        argv[7] = "dodo".to_string();
        let err = parse(argv).expect_err("should reject");
        assert!(err.contains("owner/repo"), "{err}");
    }

    #[test]
    fn rejects_an_unshaped_timestamp() {
        let mut argv = full();
        argv[13] = "yesterday".to_string();
        let err = parse(argv).expect_err("should reject");
        assert!(err.contains("RFC 3339"), "{err}");
    }

    #[test]
    fn rejects_an_unknown_option() {
        let mut argv = full();
        argv.push("--signing-key".to_string());
        let err = parse(argv).expect_err("should reject");
        assert!(err.contains("unknown option `--signing-key`"), "{err}");
    }

    #[test]
    fn rejects_an_unknown_platform_key() {
        let mut argv = full();
        argv[19] = "macos-aarch64".to_string();
        let err = parse(argv).expect_err("should reject");
        assert!(err.contains("macos-aarch64"), "{err}");
    }

    #[test]
    fn accepts_offsets_and_fractional_seconds() {
        for stamp in [
            "2026-07-30T12:11:03Z",
            "2026-07-30T12:11:03.123Z",
            "2026-07-30T12:11:03+07:00",
        ] {
            assert_eq!(validate_rfc3339(stamp), Ok(()), "rejected {stamp}");
        }
    }
}
