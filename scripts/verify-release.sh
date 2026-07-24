#!/usr/bin/env bash
#
# Verifies a packaged dodo archive before it is published.
#
#   scripts/verify-release.sh <archive> [--expect-version <v>] [--expect-tag <t>]
#
# Checks, in order:
#
#   1. the archive exists and its .sha256 sidecar matches
#   2. the contents list (printed, so a reviewer sees what shipped)
#   3. the binary is present and has its executable bit
#   4. the binary runs: `dodo --build-info` (see the caveat below)
#   5. the embedded metadata is real — version matches, commit is not
#      `unknown` and not `-dirty`, and the tag matches when one was expected
#   6. sizes of the archive and of the unpacked binary
#
# WHAT STEP 4 PROVES, AND WHAT IT DOES NOT. dodo is a GUI application; a CI
# runner has no display, so the window cannot be opened there. `--build-info`
# returns before any GPUI or window code runs (see `print_build_metadata_and_exit`
# in src/main.rs), which proves the file is a valid executable for this
# platform, that its dynamic libraries resolve, and that the embedded metadata
# is what we think it is. It does NOT prove the UI renders. That check is
# manual, on a real desktop, and docs/release.md says so.
#
# Exit status is non-zero on the first failed check, so this is safe to run as
# a required release step.

set -euo pipefail

archive=""
expect_version=""
expect_tag=""

die() {
    printf '\033[31mFAIL\033[0m %s\n' "$1" >&2
    exit 1
}
ok() { printf '\033[32m  ok\033[0m %s\n' "$1"; }
info() { printf '     %s\n' "$1"; }

while [ $# -gt 0 ]; do
    case "$1" in
        --expect-version) expect_version="${2:?}"; shift 2 ;;
        --expect-tag) expect_tag="${2:?}"; shift 2 ;;
        -h|--help) sed -n '2,28p' "${BASH_SOURCE[0]}"; exit 0 ;;
        -*) die "unknown argument: $1" ;;
        *) archive="$1"; shift ;;
    esac
done

[ -n "$archive" ] || die "usage: verify-release.sh <archive> [--expect-version <v>]"
[ -f "$archive" ] || die "no such archive: $archive"

archive_dir="$(cd "$(dirname "$archive")" && pwd)"
archive_name="$(basename "$archive")"
archive="$archive_dir/$archive_name"

printf '\n== verifying %s\n\n' "$archive_name"

# --- 1. checksum -----------------------------------------------------------
if [ -f "$archive.sha256" ]; then
    if command -v sha256sum >/dev/null 2>&1; then
        (cd "$archive_dir" && sha256sum -c "$archive_name.sha256" >/dev/null) \
            || die "SHA256 does not match $archive_name.sha256"
    elif command -v shasum >/dev/null 2>&1; then
        (cd "$archive_dir" && shasum -a 256 -c "$archive_name.sha256" >/dev/null) \
            || die "SHA256 does not match $archive_name.sha256"
    else
        die "no sha256sum or shasum available"
    fi
    ok "sha256 matches sidecar"
else
    die "missing checksum sidecar: $archive_name.sha256"
fi
sha="$(awk '{print $1; exit}' "$archive.sha256")"
info "sha256: $sha"

# --- 2. contents -----------------------------------------------------------
workdir="$(mktemp -d)"
# shellcheck disable=SC2064  # $workdir must expand now, not at trap time
trap "rm -rf '$workdir'" EXIT

case "$archive_name" in
    *.tar.gz)
        info "contents:"
        tar -tzf "$archive" | sed 's/^/       /'
        tar -xzf "$archive" -C "$workdir"
        ;;
    *.zip)
        info "contents:"
        unzip -l "$archive" | sed 's/^/       /'
        unzip -q "$archive" -d "$workdir"
        ;;
    *) die "unknown archive type: $archive_name" ;;
esac
ok "archive unpacks"

# --- 3. the binary ---------------------------------------------------------
# `dodo`, `dodo.exe`, or dodo.app/Contents/MacOS/dodo for the bundle archive.
bin="$(find "$workdir" -type f \( -name dodo -o -name dodo.exe \) -print | head -1)"
[ -n "$bin" ] || die "no dodo binary inside the archive"
info "binary: ${bin#"$workdir"/}"

case "$archive_name" in
    *.zip) ;; # NTFS has no executable bit; nothing to assert
    *) [ -x "$bin" ] || die "the binary lost its executable bit in packaging" ;;
esac
ok "executable bit preserved"

# --- 4/5. it runs, and says what it should ---------------------------------
host_os="$(uname -s)"
runnable=0
case "$archive_name:$host_os" in
    *-macos-*:Darwin) runnable=1 ;;
    *-linux-*:Linux) runnable=1 ;;
esac
# An arm64 archive cannot be executed on an x64 host (Rosetta only goes the
# other way), so skip rather than fail: the matching runner verifies it.
if [ "$runnable" = "1" ]; then
    case "$archive_name" in
        *-arm64.*|*-arm64-*) [ "$(uname -m)" = "arm64" ] || [ "$(uname -m)" = "aarch64" ] || runnable=0 ;;
        *-x64.*|*-x64-*) [ "$(uname -m)" = "x86_64" ] || runnable=0 ;;
    esac
fi

if [ "$runnable" = "1" ]; then
    output="$("$bin" --build-info)" || die "the binary did not run (--build-info exited non-zero)"
    ok "binary launches (--build-info)"
    printf '%s\n' "$output" | sed 's/^/       /'

    field() { printf '%s\n' "$output" | awk -v k="$1:" '$1==k {print $2; exit}'; }

    got_version="$(field version)"
    [ -n "$got_version" ] || die "--build-info printed no version"
    if [ -n "$expect_version" ] && [ "$got_version" != "$expect_version" ]; then
        die "version mismatch: binary says $got_version, expected $expect_version"
    fi
    ok "version: $got_version"

    got_commit="$(field commit)"
    case "$got_commit" in
        unknown) die "commit is 'unknown' — built without git metadata" ;;
        *-dirty) die "commit is $got_commit — built from a modified working tree" ;;
    esac
    ok "commit: $got_commit"

    if [ -n "$expect_tag" ]; then
        got_tag="$(field tag)"
        [ "$got_tag" = "$expect_tag" ] \
            || die "tag mismatch: binary says $got_tag, expected $expect_tag"
        ok "tag: $got_tag"
    fi

    [ "$(field build_time)" != "unknown" ] || die "build_time is 'unknown'"
    [ "$(field target)" != "unknown" ] || die "target is 'unknown'"
    ok "build metadata complete"
else
    info "skipped execution: $archive_name cannot run on $host_os/$(uname -m)"
    info "(the matching platform's job verifies it)"
fi

# --- 6. sizes --------------------------------------------------------------
# `wc -c` rather than `stat`, whose flags differ between BSD and GNU.
archive_bytes="$(wc -c < "$archive" | tr -d ' ')"
binary_bytes="$(wc -c < "$bin" | tr -d ' ')"
human() { awk -v b="$1" 'BEGIN { printf "%.1f MiB", b/1048576 }'; }
info "archive size: $archive_bytes bytes ($(human "$archive_bytes"))"
info "binary size:  $binary_bytes bytes ($(human "$binary_bytes"))"

printf '\n\033[32mverified\033[0m %s\n\n' "$archive_name"
