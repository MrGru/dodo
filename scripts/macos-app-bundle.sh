#!/usr/bin/env bash
#
# Builds a dodo.app bundle around an already-built macOS binary.
#
#   scripts/macos-app-bundle.sh --binary <path> [--version <v>] [--out <dir>]
#                               [--sign <identity>]
#                               [--notary-key <path-to-.p8>]
#                               [--notary-key-id <id>] [--notary-issuer <uuid>]
#
# Layout produced (the minimum macOS accepts for a GUI app):
#
#   dodo.app/Contents/Info.plist
#   dodo.app/Contents/MacOS/dodo
#   dodo.app/Contents/Resources/dodo.icns
#   dodo.app/Contents/Resources/LICENSE
#   dodo.app/Contents/Resources/THIRD-PARTY-NOTICES.md
#
# Signing: by default the bundle is ad-hoc signed (--sign -) so it is valid for
# local use. Pass --sign "Developer ID Application: Name (TEAMID)" — or just the
# Team ID, which codesign resolves — for a real identity.
#
# Notarisation happens here too, and only when a real identity AND all three
# --notary-* values are given: an ad-hoc bundle is what a local build and a fork
# without secrets produce, and the notary service would reject it anyway.
# docs/macos-signing.md is the authority — §4.1 for the ordering, §4.2 for why
# these steps live inside this script rather than in the workflow, §2 for where
# the App Store Connect API key values come from.

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

binary=""
version=""
out_dir="$repo_root/dist"
sign_identity="-"  # ad-hoc by default
notary_key=""
notary_key_id=""
notary_issuer=""

die() {
    printf 'macos-app-bundle.sh: %s\n' "$1" >&2
    exit 1
}

while [ $# -gt 0 ]; do
    case "$1" in
        --binary) binary="${2:?--binary needs a value}"; shift 2 ;;
        --version) version="${2:?--version needs a value}"; shift 2 ;;
        --out) out_dir="${2:?--out needs a value}"; shift 2 ;;
        --sign) sign_identity="${2:?--sign needs a value}"; shift 2 ;;
        --notary-key) notary_key="${2:?--notary-key needs a value}"; shift 2 ;;
        --notary-key-id) notary_key_id="${2:?--notary-key-id needs a value}"; shift 2 ;;
        --notary-issuer) notary_issuer="${2:?--notary-issuer needs a value}"; shift 2 ;;
        -h|--help) sed -n '2,33p' "${BASH_SOURCE[0]}"; exit 0 ;;
        *) die "unknown argument: $1" ;;
    esac
done

[ -n "$binary" ] || die "--binary is required"
[ -f "$binary" ] || die "no such binary: $binary"

# Checked before anything is assembled: a credential mistake should not cost a
# whole bundle build and a signing round trip to discover.
if [ "$sign_identity" = "-" ] && [ -n "$notary_key$notary_key_id$notary_issuer" ]; then
    die "--notary-* needs a real --sign identity; the ad-hoc one (-) cannot be notarised"
fi
if [ -n "$notary_key" ]; then
    [ -f "$notary_key" ] || die "no such App Store Connect API key: $notary_key"
fi

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
# crates/dodo-app-icon and is embedded in the binary. This one is not embedded.
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

printf 'signing %s with identity: %s\n' "$app" "$sign_identity"
codesign --force --options runtime --timestamp --sign "$sign_identity" "$app"
codesign --verify --deep --strict --verbose=2 "$app" || die "outer bundle signature verification failed"

# --- notarisation and stapling ---------------------------------------------
#
# This happens HERE, before scripts/package.sh tars the bundle and checksums it:
# the published SHA-256 and the update.json entry are computed from that
# archive, so anything done to dodo.app afterwards is invisible to the release
# (docs/macos-signing.md §4.1, constraint 3).
#
# The keychain the identity comes from is set up by the release workflow, not
# here — `security set-key-partition-list` is the step everyone forgets, and
# without it codesign blocks on a dialog no runner can click.
if [ "$sign_identity" = "-" ]; then
    # The unsigned path, and the one every local build and every fork without
    # secrets takes. The bundle is complete and valid; Gatekeeper will
    # quarantine the download and the release notes say how to clear that.
    printf 'ad-hoc signed; not notarised (needs --sign plus --notary-key, --notary-key-id, --notary-issuer)\n'
elif [ -z "$notary_key" ] || [ -z "$notary_key_id" ] || [ -z "$notary_issuer" ]; then
    # Deliberately a warning rather than an error: a real signature without a
    # ticket is still a step up from ad-hoc, and rehearsing signing alone is a
    # legitimate thing to do by hand.
    printf 'WARNING: signed with %s but NOT notarised — all three --notary-* values are required\n' "$sign_identity" >&2
else
    # The notary service takes a zip, a UDIF disk image or a flat installer
    # package — never a .tar.gz, which is what dodo actually ships. So this zip
    # is built for Apple and thrown away; the published artifact stays
    # dodo-vX-macos-<arch>-app.tar.gz, because tools/update-manifest selects the
    # macOS entry by exact filename and asserts that URL (docs/macos-signing.md
    # §4.2). Do not "simplify" this zip into being the release asset.
    #
    # `ditto -c -k --keepParent` is the documented way to build it: it keeps the
    # dodo.app directory itself as the archive root, which is what the notary
    # service expects to find a bundle in.
    notary_tmp="$(mktemp -d)"
    trap 'rm -rf "$notary_tmp"' EXIT
    notary_zip="$notary_tmp/dodo-notarize.zip"
    ditto -c -k --keepParent "$app" "$notary_zip"

    # --wait blocks until Apple returns a terminal status. Without it the ticket
    # would not exist yet when stapler runs, which is the "Error 65" everyone
    # hits. It has no upper bound, which is what the macOS release job's timeout
    # has to accommodate.
    #
    # --issuer is REQUIRED for a Team API key and must NOT be passed for an
    # Individual one (docs/macos-signing.md §1.4). dodo's key is a Team key, so
    # this is unconditional rather than "add it if authentication fails".
    printf 'notarising %s\n' "$app"
    notary_log="$notary_tmp/notarytool.txt"
    xcrun notarytool submit "$notary_zip" \
        --key "$notary_key" \
        --key-id "$notary_key_id" \
        --issuer "$notary_issuer" \
        --wait 2>&1 | tee "$notary_log"

    # Checked rather than trusted to the exit status: notarytool has shipped
    # versions that exit 0 having reported `status: Invalid`, and an unnoticed
    # Invalid produces an archive that fails at the user rather than here.
    submission_id="$(awk '/^ *id: /{print $2; exit}' "$notary_log")"
    grep -qE '^ *status: Accepted' "$notary_log" \
        || die "notarisation was not accepted (submission $submission_id); read the reason with:
    xcrun notarytool log $submission_id --key <key.p8> --key-id <id> --issuer <uuid>"

    # Staple, so the ticket travels inside the bundle and Gatekeeper does not
    # have to reach Apple on first launch. NOTHING may re-sign the bundle after
    # this line — code-signing a stapled bundle silently invalidates the ticket
    # (`man stapler`).
    xcrun stapler staple "$app"

    rm -rf "$notary_tmp"
    trap - EXIT

    # Three checks, because each catches something the others do not: that the
    # signature still verifies with the ticket attached, that Gatekeeper's own
    # policy engine now accepts the bundle, and that the ticket is really there.
    # `--deep` is deprecated for *signing* only; it is still correct here.
    codesign --verify --deep --strict --verbose=2 "$app" \
        || die "signature verification failed after stapling"
    spctl --assess --type execute --verbose=4 "$app" \
        || die "Gatekeeper rejected the notarised bundle"
    xcrun stapler validate "$app" \
        || die "no valid stapled notarisation ticket on $app"
    printf 'notarised and stapled %s\n' "$app"
fi
