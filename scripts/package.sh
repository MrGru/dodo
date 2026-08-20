#!/usr/bin/env bash
#
# Packages an already-built dodo binary into a release archive.
#
#   scripts/package.sh [--target <triple>] [--version <v>] [--out <dir>]
#                      [--profile <name>] [--app-bundle]
#                      [--sign <identity>] [--notary-key <path-to-.p8>]
#                      [--notary-key-id <id>] [--notary-issuer <uuid>]
#
# Produces, under --out (default: dist/):
#
#   dodo-v<version>-<platform>-<arch>.tar.gz        the binary + docs
#   dodo-v<version>-<platform>-<arch>.tar.gz.sha256 its checksum
#   dodo-v<version>-macos-<arch>-app.tar.gz         --app-bundle only: dodo.app
#
# Windows is packaged by scripts/package.ps1 instead — Compress-Archive is the
# only zip tool guaranteed to exist on a windows runner.
#
# Signing is macOS-only and off by default: with no --sign the archives contain
# exactly what they always did, which is what a local build and a fork without
# secrets produce. With --sign (plus the three --notary-* values, which the
# release workflow supplies from secrets) everything is signed and notarised
# BEFORE it is tarred and checksummed — the published SHA-256 and the
# update.json entry are computed from the archive, so a signature added after
# the tar would not be in the release at all. docs/macos-signing.md §4 is the
# authority on that ordering.
#
# This script does not build anything. CI builds with `cargo build --release
# --locked` and then calls this; doing it in one step would hide which half
# failed, and cross-built binaries cannot be rebuilt on the packaging host.

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

target=""
version=""
out_dir="$repo_root/dist"
profile="release"
app_bundle=0
sign_identity="-"  # ad-hoc by default; see the header
notary_key=""
notary_key_id=""
notary_issuer=""

die() {
    printf 'package.sh: %s\n' "$1" >&2
    exit 1
}

while [ $# -gt 0 ]; do
    case "$1" in
        --target) target="${2:?--target needs a value}"; shift 2 ;;
        --version) version="${2:?--version needs a value}"; shift 2 ;;
        --out) out_dir="${2:?--out needs a value}"; shift 2 ;;
        --profile) profile="${2:?--profile needs a value}"; shift 2 ;;
        --app-bundle) app_bundle=1; shift ;;
        --sign) sign_identity="${2:?--sign needs a value}"; shift 2 ;;
        --notary-key) notary_key="${2:?--notary-key needs a value}"; shift 2 ;;
        --notary-key-id) notary_key_id="${2:?--notary-key-id needs a value}"; shift 2 ;;
        --notary-issuer) notary_issuer="${2:?--notary-issuer needs a value}"; shift 2 ;;
        -h|--help) sed -n '2,31p' "${BASH_SOURCE[0]}"; exit 0 ;;
        *) die "unknown argument: $1" ;;
    esac
done

# Host triple, when --target was not given. `rustc -vV` is the authority; the
# same string cargo uses for target directories.
if [ -z "$target" ]; then
    target="$(rustc -vV | awk '/^host: /{print $2}')"
    [ -n "$target" ] || die "could not determine the host target triple"
fi

# The version in Cargo.toml is the single source of truth for archive names;
# the release workflow checks it against the git tag before it gets here.
if [ -z "$version" ]; then
    version="$(awk '/^\[package\]/{p=1;next} /^\[/{p=0} p && /^version *=/{gsub(/[",]/,"",$3); print $3; exit}' "$repo_root/Cargo.toml")"
    [ -n "$version" ] || die "could not read version from Cargo.toml"
fi

# Triple -> the platform/arch words the naming convention uses. Anything not
# listed is a deliberate failure rather than a guess: a wrongly named archive
# is worse than no archive.
case "$target" in
    *-apple-darwin) platform="macos" ;;
    *-unknown-linux-*) platform="linux" ;;
    *-pc-windows-*) platform="windows" ;;
    *) die "unsupported target for packaging: $target" ;;
esac

case "$target" in
    aarch64-*|arm64-*) arch="arm64" ;;
    x86_64-*) arch="x64" ;;
    *) die "unsupported architecture for packaging: $target" ;;
esac

# Signing is macOS-only here. scripts/package.ps1 owns the Windows half and
# Linux packages are unsigned by convention, so a --sign that could not be acted
# on is a mistake worth failing on rather than ignoring.
if [ "$sign_identity" != "-" ] && [ "$platform" != "macos" ]; then
    die "--sign is macOS only (target: $target)"
fi
if [ "$sign_identity" = "-" ] && [ -n "$notary_key$notary_key_id$notary_issuer" ]; then
    die "--notary-* needs a real --sign identity; the ad-hoc one (-) cannot be notarised"
fi
if [ -n "$notary_key" ]; then
    [ -f "$notary_key" ] || die "no such App Store Connect API key: $notary_key"
fi

exe=""
[ "$platform" = "windows" ] && exe=".exe"

# cargo drops a --target build under target/<triple>/<profile>/ and a host
# build under target/<profile>/.
bin="$repo_root/target/$target/$profile/dodo$exe"
[ -f "$bin" ] || bin="$repo_root/target/$profile/dodo$exe"
[ -f "$bin" ] || die "no dodo binary found; run: cargo build --profile $profile --locked"

name="dodo-v${version}-${platform}-${arch}"
stage="$out_dir/.stage/$name"

rm -rf "$stage"
mkdir -p "$stage" "$out_dir"

install_binary() {
    # `cp` then `chmod`: `install` is not portable enough across BSD/GNU, and
    # the executable bit is the one thing a tar.gz must carry through.
    cp "$bin" "$1/dodo$exe"
    chmod 755 "$1/dodo$exe"
}

install_binary "$stage"

# Docs travel with the binary so an unzipped archive is self-explanatory.
#
# LICENSE and THIRD-PARTY-NOTICES.md are a HARD requirement, not a best-effort
# glob: dodo's source is MIT but its binary links GPL-3.0-or-later crates
# (see THIRD-PARTY-NOTICES.md), so an archive that ships the binary without
# them is worse than no archive. README.md is nice-to-have by comparison, but
# it has always been there, so a missing one means something is wrong too.
for doc in README.md LICENSE THIRD-PARTY-NOTICES.md; do
    [ -f "$repo_root/$doc" ] || die "missing $doc; it must ship inside the archive"
    cp "$repo_root/$doc" "$stage/"
done

# --- desktop integration files --------------------------------------------
#
# Linux only, and laid out under share/ exactly as they must end up on disk:
#
#   share/applications/dodo.desktop
#   share/icons/hicolor/<n>x<n>/apps/dodo.png
#
# so installing is `cp -r share/ ~/.local/` (or /usr/local/) with no renaming,
# and a future .deb or AppImage job can copy the tree wholesale into its own
# staging root. macOS carries its icon inside dodo.app instead, and Windows is
# packaged by package.ps1.
#
# These are committed artifacts (scripts/generate-icons.py regenerates them);
# packaging never builds them, because most of the tooling to do so is macOS
# only. Missing files are a hard error rather than a quietly icon-less archive.
#
# Installing them is not optional decoration: dodo.desktop is what a Wayland
# compositor matches the window's app_id against to find `Icon=`, so a binary
# run without it shows a generic task-bar icon no matter what the binary does.
# src/window_icon.rs covers the X11 half of that gap on its own (_NET_WM_ICON)
# and cannot cover the Wayland half. See "The bare-binary cases" in
# docs/release.md.
if [ "$platform" = "linux" ]; then
    desktop_file="$repo_root/assets/linux/dodo.desktop"
    hicolor="$repo_root/assets/linux/hicolor"
    [ -f "$desktop_file" ] || die "missing $desktop_file"
    [ -d "$hicolor" ] || die "missing $hicolor; run: scripts/generate-icons.py"
    mkdir -p "$stage/share/applications"
    cp "$desktop_file" "$stage/share/applications/dodo.desktop"
    mkdir -p "$stage/share/icons"
    cp -R "$hicolor" "$stage/share/icons/hicolor"
fi

# tar: GNU tar can be told to produce a byte-identical archive from identical
# inputs; BSD tar (the macOS default) cannot, so those flags are added only
# when they are understood. Everything else about the archive is already
# deterministic, so this is the last gap.
tar_flags=()
if tar --version 2>/dev/null | grep -qi 'gnu tar'; then
    tar_flags+=(--sort=name --owner=0 --group=0 --numeric-owner)
    [ -n "${SOURCE_DATE_EPOCH:-}" ] && tar_flags+=(--mtime="@$SOURCE_DATE_EPOCH")
fi

# Note the `${tar_flags[@]+"${tar_flags[@]}"}` form at the two call sites
# below: expanding an *empty* array under `set -u` is an error in bash 3.2,
# which is what macOS ships — and on macOS this array is always empty, because
# BSD tar does not take those flags. That form expands to nothing when the
# array is empty and to the quoted elements otherwise, on every bash.

# `shasum` on macOS, `sha256sum` on Linux; both write the `<sha>  <file>` line
# `shasum -c` / `sha256sum -c` read back. Written next to the archive and
# uploaded with it, so a download can be checked without trusting the page it
# came from.
checksum() {
    local file="$1" dir base
    dir="$(dirname "$file")"
    base="$(basename "$file")"
    if command -v sha256sum >/dev/null 2>&1; then
        (cd "$dir" && sha256sum "$base" > "$base.sha256")
    elif command -v shasum >/dev/null 2>&1; then
        (cd "$dir" && shasum -a 256 "$base" > "$base.sha256")
    else
        die "no sha256sum or shasum available to checksum $base"
    fi
    printf 'checksum %s\n' "$file.sha256"
}

# --- macOS: sign and notarise the staged binary, before it is archived ------
#
# The plain-binary archive gets the same treatment as the .app bundle, minus one
# step: it is signed with the hardened runtime and a secure timestamp, and it is
# notarised — but it can NOT be stapled. `man stapler`: stapler works only with
# UDIF disk images, signed flat installer packages and certain code-signed
# bundles such as .app, and a bare Mach-O is none of those. So this binary's
# ticket lives only on Apple's servers and Gatekeeper checks it online; that is
# the best available for this artifact shape and it is not a defect to fix.
# (docs/macos-signing.md §4.3.)
#
# The copy in the stage directory is signed, never $bin: the .app bundle below
# takes its own copy from $bin and signs that inside the bundle, and signing the
# build output in place would make a re-run of this script sign an already
# signed binary.
if [ "$sign_identity" != "-" ]; then
    printf 'signing %s with identity: %s\n' "$stage/dodo" "$sign_identity"
    codesign --force --options runtime --timestamp --sign "$sign_identity" "$stage/dodo"
    codesign --verify --strict --verbose=2 "$stage/dodo" \
        || die "the staged binary's signature is invalid"

    if [ -n "$notary_key" ] && [ -n "$notary_key_id" ] && [ -n "$notary_issuer" ]; then
        # Same shape as scripts/macos-app-bundle.sh, which carries the full
        # reasoning: a temporary zip because the notary service takes no
        # tar.gz, --issuer because dodo's key is a Team key, and an explicit
        # `status: Accepted` check because notarytool has exited 0 on Invalid.
        notary_tmp="$(mktemp -d)"
        trap 'rm -rf "$notary_tmp"' EXIT
        notary_zip="$notary_tmp/dodo-binary-notarize.zip"
        notary_log="$notary_tmp/notarytool.txt"
        ditto -c -k "$stage/dodo" "$notary_zip"
        printf 'notarising %s\n' "$stage/dodo"
        xcrun notarytool submit "$notary_zip" \
            --key "$notary_key" \
            --key-id "$notary_key_id" \
            --issuer "$notary_issuer" \
            --wait 2>&1 | tee "$notary_log"
        submission_id="$(awk '/^ *id: /{print $2; exit}' "$notary_log")"
        grep -qE '^ *status: Accepted' "$notary_log" \
            || die "notarisation was not accepted (submission $submission_id); read the reason with:
    xcrun notarytool log $submission_id --key <key.p8> --key-id <id> --issuer <uuid>"
        rm -rf "$notary_tmp"
        trap - EXIT
    else
        printf 'WARNING: signed the binary but did not notarise it — all three --notary-* values are required\n' >&2
    fi
fi

archive="$out_dir/$name.tar.gz"
tar ${tar_flags[@]+"${tar_flags[@]}"} -czf "$archive" -C "$out_dir/.stage" "$name"
printf 'packaged %s\n' "$archive"
checksum "$archive"

# --- macOS .app bundle -----------------------------------------------------
#
# A second, separate archive rather than a replacement: the plain binary is
# what CI verification runs and what a terminal user wants, the bundle is what
# a desktop user drags to /Applications.
#
# `scripts/macos-app-bundle.sh` signs the completed bundle inside-out and, when
# the --notary-* values are present, notarises and staples it — all before the
# tar below, for the reason given in this file's header. With no --sign it
# ad-hoc signs, exactly as it always has.
if [ "$app_bundle" = "1" ]; then
    [ "$platform" = "macos" ] || die "--app-bundle is macOS only (target: $target)"
    app_stage="$out_dir/.stage/app"
    rm -rf "$app_stage"
    mkdir -p "$app_stage"
    app_args=(--binary "$bin" --version "$version" --out "$app_stage"
              --sign "$sign_identity")
    if [ "$sign_identity" != "-" ] && [ -n "$notary_key" ]; then
        app_args+=(--notary-key "$notary_key"
                   --notary-key-id "$notary_key_id"
                   --notary-issuer "$notary_issuer")
    fi
    "$repo_root/scripts/macos-app-bundle.sh" "${app_args[@]}"
    app_archive="$out_dir/$name-app.tar.gz"
    tar ${tar_flags[@]+"${tar_flags[@]}"} -czf "$app_archive" -C "$app_stage" "dodo.app"
    printf 'packaged %s\n' "$app_archive"
    checksum "$app_archive"
fi

rm -rf "$out_dir/.stage"
