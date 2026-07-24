#!/usr/bin/env bash
#
# Packages an already-built dodo binary into a release archive.
#
#   scripts/package.sh [--target <triple>] [--version <v>] [--out <dir>]
#                      [--profile <name>] [--app-bundle]
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
        -h|--help) sed -n '2,20p' "${BASH_SOURCE[0]}"; exit 0 ;;
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
# The bundle is unsigned. `scripts/macos-app-bundle.sh` is where codesign and
# notarisation hook in later; see "Future readiness" in docs/release.md.
if [ "$app_bundle" = "1" ]; then
    [ "$platform" = "macos" ] || die "--app-bundle is macOS only (target: $target)"
    app_stage="$out_dir/.stage/app"
    rm -rf "$app_stage"
    mkdir -p "$app_stage"
    "$repo_root/scripts/macos-app-bundle.sh" \
        --binary "$bin" --version "$version" --out "$app_stage"
    app_archive="$out_dir/$name-app.tar.gz"
    tar ${tar_flags[@]+"${tar_flags[@]}"} -czf "$app_archive" -C "$app_stage" "dodo.app"
    printf 'packaged %s\n' "$app_archive"
    checksum "$app_archive"
fi

rm -rf "$out_dir/.stage"
