#!/usr/bin/env bash
#
# Builds an unsigned dodo.app bundle around an already-built macOS binary.
#
#   scripts/macos-app-bundle.sh --binary <path> [--version <v>] [--out <dir>]
#
# Layout produced (the minimum macOS accepts for a GUI app):
#
#   dodo.app/Contents/Info.plist
#   dodo.app/Contents/MacOS/dodo
#   dodo.app/Contents/Resources/dodo.icns
#   dodo.app/Contents/Resources/LICENSE
#   dodo.app/Contents/Resources/THIRD-PARTY-NOTICES.md
#
# Signing and notarisation are deliberately NOT done here — see the block at
# the bottom of this file and "Future readiness" in docs/release.md. This
# script exists so that turning them on later is an edit in one place.

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

binary=""
version=""
out_dir="$repo_root/dist"

die() {
    printf 'macos-app-bundle.sh: %s\n' "$1" >&2
    exit 1
}

while [ $# -gt 0 ]; do
    case "$1" in
        --binary) binary="${2:?--binary needs a value}"; shift 2 ;;
        --version) version="${2:?--version needs a value}"; shift 2 ;;
        --out) out_dir="${2:?--out needs a value}"; shift 2 ;;
        -h|--help) sed -n '2,18p' "${BASH_SOURCE[0]}"; exit 0 ;;
        *) die "unknown argument: $1" ;;
    esac
done

[ -n "$binary" ] || die "--binary is required"
[ -f "$binary" ] || die "no such binary: $binary"

if [ -z "$version" ]; then
    version="$(awk '/^\[package\]/{p=1;next} /^\[/{p=0} p && /^version *=/{gsub(/[",]/,"",$3); print $3; exit}' "$repo_root/Cargo.toml")"
    [ -n "$version" ] || die "could not read version from Cargo.toml"
fi

app="$out_dir/dodo.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"

cp "$binary" "$app/Contents/MacOS/dodo"
chmod 755 "$app/Contents/MacOS/dodo"

# The licence and the third-party notice, inside the bundle.
#
# The .app archive contains nothing but dodo.app — a bundle is a directory that
# Finder presents as one object, so anything alongside it would be a second
# thing to drag. Putting these in Contents/Resources/ is how a macOS app ships
# its licence text, and it means the terms travel with the application even
# after it has been dragged to /Applications and the archive thrown away. That
# matters here: dodo's source is MIT but the binary links GPL-3.0-or-later
# crates (see THIRD-PARTY-NOTICES.md). Missing either one is a hard error.
for doc in LICENSE THIRD-PARTY-NOTICES.md; do
    [ -f "$repo_root/$doc" ] || die "missing $doc; it must ship inside dodo.app"
    cp "$repo_root/$doc" "$app/Contents/Resources/$doc"
done

# The application icon. Committed, not built here: `iconutil` only exists on
# macOS, and regenerating it at package time would make the bundle depend on
# the host. `scripts/generate-icons.py` rebuilds it from the 1024 master in
# assets/branding/ whenever the artwork changes.
#
# Not to be confused with assets/icons, which is the in-app SVG set behind
# src/app_icon.rs and is embedded in the binary. This one is not embedded.
icon_source="$repo_root/assets/macos/dodo.icns"
[ -f "$icon_source" ] || die "missing $icon_source; run: scripts/generate-icons.py"
cp "$icon_source" "$app/Contents/Resources/dodo.icns"
# CFBundleIconFile names the file in Resources/ without its extension. Getting
# this wrong is silent: the bundle builds and Finder shows the generic app
# icon, so any change here needs a look at the built bundle, not just exit 0.
icon_entry='
    <key>CFBundleIconFile</key>
    <string>dodo</string>'

# CFBundleIdentifier must stay stable forever: it is the key macOS uses for
# preferences, keychain items and — once signing exists — the App ID the
# certificate and notarisation ticket are bound to.
cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>dodo</string>
    <key>CFBundleDisplayName</key>
    <string>dodo</string>
    <key>CFBundleIdentifier</key>
    <string>io.github.mrgru.dodo</string>
    <key>CFBundleExecutable</key>
    <string>dodo</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleShortVersionString</key>
    <string>${version}</string>
    <key>CFBundleVersion</key>
    <string>${version}</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>${icon_entry}
    <!-- GPUI renders at native resolution; without this macOS would upscale a
         1x framebuffer and the whole UI would look soft on a Retina display. -->
    <key>NSHighResolutionCapable</key>
    <true/>
    <!-- dodo is a windowed app, not a background agent. -->
    <key>LSUIElement</key>
    <false/>
    <!-- Matches what the GPUI/Zed toolchain supports; raise deliberately. -->
    <key>LSMinimumSystemVersion</key>
    <string>11.0</string>
</dict>
</plist>
PLIST

printf 'built %s\n' "$app"

# --- Future: signing and notarisation --------------------------------------
#
# Everything below is intentionally not implemented; it needs secrets this
# repository does not have. The order matters, so it is recorded here rather
# than rediscovered:
#
#   1. Import the Developer ID Application certificate into a temporary
#      keychain (secrets: MACOS_CERTIFICATE, MACOS_CERTIFICATE_PWD).
#   2. codesign --deep --force --options runtime --timestamp \
#          --sign "Developer ID Application: ..." "$app"
#      `--options runtime` (hardened runtime) is required for notarisation.
#   3. Zip the bundle with `ditto -c -k --keepParent` — notarytool rejects a
#      plain tar.gz — and submit:
#      xcrun notarytool submit --wait --apple-id ... --team-id ... --password ...
#   4. xcrun stapler staple "$app", so the ticket travels with the download.
#   5. Verify: codesign --verify --deep --strict --verbose=2 "$app"
#              spctl --assess --type execute "$app"
#
# Until then the archive is unsigned and Gatekeeper will quarantine it; the
# release notes tell users to clear that themselves (docs/release.md).
