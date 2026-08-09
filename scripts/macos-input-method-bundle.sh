#!/usr/bin/env bash
#
# Builds the "Dodo Vietnamese.app" input-method bundle.
#
#   scripts/macos-input-method-bundle.sh [--binary <path>] [--version <v>] [--out <dir>]
#                                    [--sign <identity>]
#
# With no --binary it builds one:
#     cargo build --release --locked -p dodo-ime-macos --bin DodoVietnamese
#
# Layout produced (the minimum macOS accepts for an input method):
#
#   Dodo Vietnamese.app/Contents/Info.plist
#   Dodo Vietnamese.app/Contents/MacOS/DodoVietnamese
#   Dodo Vietnamese.app/Contents/Resources/en.lproj/InfoPlist.strings
#   Dodo Vietnamese.app/Contents/Resources/vi.lproj/InfoPlist.strings
#
# Install it by copying the bundle to ~/Library/Input Methods/ and adding it in
# System Settings; docs/macos-input-method.md is the authority on that and on
# what has and has not been verified.
#
# Signing: by default the bundle is ad-hoc signed (--sign -) so it is valid for
# local use. Pass --sign "Developer ID Application: Name (TEAMID)" for a real
# identity. When nested inside dodo.app, scripts/macos-app-bundle.sh re-signs
# the inner bundle first, then the outer.

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

binary=""
version=""
out_dir="$repo_root/dist"
sign_identity="-"  # ad-hoc by default

die() {
    printf 'macos-input-method-bundle.sh: %s\n' "$1" >&2
    exit 1
}

while [ $# -gt 0 ]; do
    case "$1" in
        --binary) binary="${2:?--binary needs a value}"; shift 2 ;;
        --version) version="${2:?--version needs a value}"; shift 2 ;;
        --out) out_dir="${2:?--out needs a value}"; shift 2 ;;
        --sign) sign_identity="${2:?--sign needs a value}"; shift 2 ;;
        -h|--help) sed -n '2,25p' "${BASH_SOURCE[0]}"; exit 0 ;;
        *) die "unknown argument: $1" ;;
    esac
done

if [ -z "$version" ]; then
    version="$(awk '/^\[package\]/{p=1;next} /^\[/{p=0} p && /^version *=/{gsub(/[",]/,"",$3); print $3; exit}' "$repo_root/Cargo.toml")"
    [ -n "$version" ] || die "could not read version from Cargo.toml"
fi

if [ -z "$binary" ]; then
    # `--locked` for the same reason as everywhere else in this repo: Cargo.lock
    # is the only pin on the git dependencies.
    ( cd "$repo_root" && cargo build --release --locked -p dodo-ime-macos --bin DodoVietnamese )
    binary="$repo_root/target/release/DodoVietnamese"
fi
[ -f "$binary" ] || die "no such binary: $binary"

# --- the identifiers, which are frozen ---------------------------------------
#
# All four are load-bearing and three of them fail *silently* when wrong:
#
#   bundle_id       CFBundleIdentifier. macOS keys preferences off it and — once
#                   signing exists — it is the App ID the certificate binds to.
#                   It MUST contain ".inputmethod." as an INFIX, not merely end
#                   in ".inputmethod": measured on macOS 26.6 by installing
#                   otherwise-identical bundles, where
#                   io.github.mrgru.dodo.inputmethod did NOT appear in
#                   TISCreateInputSourceList and
#                   io.github.mrgru.dodo.inputmethod.Dodo did. Both registered
#                   with status 0 and logged nothing.
#                   crates/dodo-ime-macos/src/bundle.rs has the whole table.
#   mode_id         The input SOURCE id, which is what TISEnableInputSource and
#                   TISSelectInputSource take. It must be enabled and selected on
#                   the MODE, never on the parent input method: the parent's
#                   kTISPropertyInputSourceIsSelectCapable is false, and
#                   selecting it fails with -50 (paramErr).
#   connection      Must equal the string passed to
#                   -[IMKServer initWithName:bundleIdentifier:]. src/main.rs
#                   reads it back out of this plist rather than repeating it, so
#                   they cannot drift.
#   controller      The Objective-C runtime class name from define_class!'s
#                   #[name = "..."] in src/controller.rs. Get this wrong and the
#                   bundle installs, appears in the input-source list, and never
#                   receives a keystroke. A unit test there reads this file to
#                   check the two agree.
bundle_id="io.github.mrgru.dodo.inputmethod.Dodo"
mode_id="$bundle_id.Vietnamese"
connection="io_github_mrgru_dodo_inputmethod_Dodo_Connection"
controller="DodoInputController"
executable="DodoVietnamese"

app="$out_dir/Dodo Vietnamese.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources/en.lproj" "$app/Contents/Resources/vi.lproj"

cp "$binary" "$app/Contents/MacOS/$executable"
chmod 755 "$app/Contents/MacOS/$executable"

# LSBackgroundOnly makes this a faceless agent: no Dock tile, no menu bar. Use
# LSUIElement instead if this ever needs its own window (a candidate panel);
# Vietnamese needs none.
#
# tsInputModeDefaultStateKey / tsInputModeIsVisibleKey / tsInputModePrimaryInScriptKey
# are what put the mode in the input-source picker at all. Dropping any of them
# leaves a bundle that installs and cannot be added.
#
# No tsInputMethodIconFileKey: the input menu then shows no glyph beside the
# name, which is cosmetic and needs a .pdf or .tiff that this repo's icon
# pipeline does not produce yet. Everything works without it.
cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleName</key>
    <string>Dodo Vietnamese</string>
    <key>CFBundleDisplayName</key>
    <string>Dodo Vietnamese</string>
    <key>CFBundleIdentifier</key>
    <string>${bundle_id}</string>
    <key>CFBundleExecutable</key>
    <string>${executable}</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>CFBundleSignature</key>
    <string>????</string>
    <key>CFBundleShortVersionString</key>
    <string>${version}</string>
    <key>CFBundleVersion</key>
    <string>${version}</string>
    <key>CFBundleInfoDictionaryVersion</key>
    <string>6.0</string>
    <key>CFBundleDevelopmentRegion</key>
    <string>en</string>
    <!-- Matches dodo.app's own floor. -->
    <key>LSMinimumSystemVersion</key>
    <string>11.0</string>
    <!-- A faceless background agent, launched by macOS and never by the user. -->
    <key>LSBackgroundOnly</key>
    <true/>

    <key>InputMethodConnectionName</key>
    <string>${connection}</string>
    <key>InputMethodServerControllerClass</key>
    <string>${controller}</string>
    <key>tsInputMethodCharacterRepertoireKey</key>
    <array><string>Latn</string></array>

    <key>ComponentInputModeDict</key>
    <dict>
        <key>tsInputModeListKey</key>
        <dict>
            <key>${mode_id}</key>
            <dict>
                <key>TISInputSourceID</key>
                <string>${mode_id}</string>
                <key>TISIntendedLanguage</key>
                <string>vi</string>
                <key>tsInputModeCharacterRepertoireKey</key>
                <array><string>Latn</string></array>
                <key>tsInputModeDefaultStateKey</key>
                <true/>
                <key>tsInputModeIsVisibleKey</key>
                <true/>
                <key>tsInputModePrimaryInScriptKey</key>
                <true/>
                <key>tsInputModeScriptKey</key>
                <string>smRoman</string>
            </dict>
        </dict>
        <key>tsVisibleInputModeOrderedArrayKey</key>
        <array><string>${mode_id}</string></array>
    </dict>
</dict>
</plist>
PLIST

# The two strings a user ever reads from this bundle, in macOS's own
# localisation mechanism rather than dodo's `i18n::Str`.
#
# They cannot go through `Str`: that type lives in the `dodo` crate, which this
# bundle must not link — and it would not help if it did, because *macOS* reads
# these, not dodo. System Settings picks the .lproj matching the user's system
# language, which is a different setting from dodo's interface language and is
# not expected to agree with it. Adding a language here is one more .lproj; see
# docs/macos-input-method.md.
#
# Without them the input-source list shows the raw mode id, which the
# investigation saw literally rendered as "dev.dodo.inputmethod.poc.Vietn…".
cat > "$app/Contents/Resources/en.lproj/InfoPlist.strings" <<STRINGS
"CFBundleName" = "Dodo Vietnamese";
"${mode_id}" = "Dodo Vietnamese";
STRINGS

cat > "$app/Contents/Resources/vi.lproj/InfoPlist.strings" <<STRINGS
"CFBundleName" = "Dodo Tiếng Việt";
"${mode_id}" = "Dodo Tiếng Việt";
STRINGS

# plutil catches a malformed plist here rather than as "the input source never
# appeared" an hour later.
plutil -lint "$app/Contents/Info.plist" > /dev/null || die "Info.plist is malformed"

# Sign after every bundle file is final. Ad-hoc (--sign -) is valid for local use.
printf 'signing %s with identity: %s\n' "$app" "$sign_identity"
codesign --force --options runtime --timestamp --sign "$sign_identity" "$app"
# Verify immediately so a broken signature fails the build.
codesign --verify --deep --strict --verbose=2 "$app" || die "signature verification failed"

printf 'built %s\n' "$app"
