//! The four names macOS looks the bundle up by, and the rule that governs one
//! of them.
//!
//! None of these is a preference. Each is read by a different part of macOS out
//! of `Info.plist`, and **three of the four fail silently when wrong** — the
//! bundle installs, `TISRegisterInputSource` returns `0`, and either the input
//! source never appears or it appears and never receives a keystroke. So they
//! live here as constants, `scripts/macos-input-method-bundle.sh` writes them,
//! and [`tests`] reads that script back to prove the two agree.
//!
//! # `CFBundleIdentifier` must contain `.inputmethod.` as an **infix**
//!
//! Not as a suffix. This was measured on macOS 26.6 while building this round,
//! by installing eight otherwise-identical bundles that differed only in their
//! identifier:
//!
//! | `CFBundleIdentifier` | appears in `TISCreateInputSourceList` |
//! |---|---|
//! | `io.github.mrgru.dodo.inputmethod` | **no** |
//! | `dev.dodo.inputmethod` | **no** |
//! | `io.github.mrgru.dodoime` | **no** |
//! | `dev.dodo.inputmethod.probe` | yes |
//! | `io.github.mrgru.dodo.inputmethod.vietnamese` | yes |
//! | `io.github.mrgru.dodo.inputmethod.Dodo` | yes |
//!
//! `TISRegisterInputSource` returned `0` for every one of them, including the
//! three that then did not exist, and nothing was written to the system log.
//! The investigation report carried this as a **READ** note from `xkey`'s README
//! — *"the identifier should follow the `*.inputmethod.*` shape"* — with the
//! caveat that no counter-example had been tried. It is a hard requirement, and
//! the wildcard after `inputmethod` is load-bearing.
//!
//! This is why the identifier is not simply `io.github.mrgru.dodo.inputmethod`,
//! which is what every naming instinct reaches for first. The shape below is
//! Apple's own — `com.apple.inputmethod.VietnameseIM` carries the mode
//! `com.apple.inputmethod.VietnameseIM.VietnameseTelex` — read as: this is
//! `mrgru`'s `dodo`'s input method named `Dodo`, and its Vietnamese mode.

/// `CFBundleIdentifier`. See the module docs: the `.inputmethod.` infix is a
/// requirement, not a convention.
pub const BUNDLE_IDENTIFIER: &str = "io.github.mrgru.dodo.inputmethod.Dodo";

/// `TISInputSourceID` of the one input **mode**.
///
/// This — not [`BUNDLE_IDENTIFIER`] — is what `TISEnableInputSource` and
/// `TISSelectInputSource` take. Enabling the parent input method returns `0` and
/// does nothing, and selecting it then fails with `-50` (`paramErr`), because
/// the parent's `kTISPropertyInputSourceIsSelectCapable` is false.
pub const INPUT_SOURCE_ID: &str = "io.github.mrgru.dodo.inputmethod.Dodo.Vietnamese";

/// `InputMethodConnectionName`, which must equal the string handed to
/// `-[IMKServer initWithName:bundleIdentifier:]`.
///
/// `src/main.rs` reads it back out of `Info.plist` rather than repeating it, so
/// there is only ever one spelling in the running process.
pub const CONNECTION_NAME: &str = "io_github_mrgru_dodo_inputmethod_Dodo_Connection";

/// `InputMethodServerControllerClass`: the Objective-C **runtime** class name,
/// which is `define_class!`'s `#[name = "…"]` in
/// [`controller`](crate::controller).
pub const CONTROLLER_CLASS: &str = "DodoInputController";

/// The name of the bundle directory, both in `dist/` and once installed.
///
/// `docs/macos-signing.md` §7.2 is why it is nested at
/// `dodo.app/Contents/Helpers/` and not under `Contents/Library/InputMethods/`:
/// only the former is a directory `codesign` discovers as nested code.
pub const BUNDLE_NAME: &str = "Dodo Vietnamese.app";

#[cfg(test)]
mod tests {
    use super::{
        BUNDLE_IDENTIFIER, BUNDLE_NAME, CONNECTION_NAME, CONTROLLER_CLASS, INPUT_SOURCE_ID,
    };

    const SCRIPT: &str = include_str!("../../../scripts/macos-input-method-bundle.sh");
    const APP_SCRIPT: &str = include_str!("../../../scripts/macos-app-bundle.sh");

    /// The measured rule, as the assertion that would catch a rename back to the
    /// obvious-looking identifier.
    #[test]
    fn the_identifier_carries_inputmethod_as_an_infix() {
        assert!(
            BUNDLE_IDENTIFIER.contains(".inputmethod."),
            "macOS does not register an input method whose CFBundleIdentifier \
             merely ends in `.inputmethod` — see this module's docs"
        );
        assert!(
            !BUNDLE_IDENTIFIER.ends_with(".inputmethod"),
            "there must be at least one component after `inputmethod`"
        );
    }

    /// The mode is a child of the method, which is what makes the input-source
    /// list show one under the other.
    #[test]
    fn the_input_source_is_a_mode_of_this_input_method() {
        assert!(INPUT_SOURCE_ID.starts_with(BUNDLE_IDENTIFIER));
        assert_ne!(INPUT_SOURCE_ID, BUNDLE_IDENTIFIER);
    }

    /// Apple's convention, and the one thing the 2026 guidelines article and
    /// `xkey` agree on: the connection name is the identifier with dots
    /// replaced. The investigation showed a name that does *not* follow it also
    /// works, so this is a tidiness check rather than a requirement.
    #[test]
    fn the_connection_name_is_the_identifier_with_dots_replaced() {
        assert_eq!(
            CONNECTION_NAME,
            BUNDLE_IDENTIFIER.replace('.', "_") + "_Connection"
        );
    }

    /// Every one of these is read out of `Info.plist` by something that does not
    /// report a mismatch, so the generator and the code have to be compared by a
    /// test or not at all.
    #[test]
    fn the_bundle_script_writes_exactly_these_names() {
        for name in [
            BUNDLE_IDENTIFIER,
            CONNECTION_NAME,
            CONTROLLER_CLASS,
            BUNDLE_NAME,
        ] {
            assert!(
                SCRIPT.contains(name),
                "scripts/macos-input-method-bundle.sh does not mention {name}"
            );
        }
        // The mode id is built from the bundle id in the script, so it is the
        // suffix that has to be found rather than the whole string.
        assert!(SCRIPT.contains("$bundle_id.Vietnamese"));
        assert_eq!(INPUT_SOURCE_ID, format!("{BUNDLE_IDENTIFIER}.Vietnamese"));
    }

    #[test]
    fn bundle_scripts_sign_after_assembly_and_inside_out() {
        let inner_sign =
            "codesign --force --options runtime --timestamp --sign \"$sign_identity\" \"$nested\"";
        let outer_sign =
            "codesign --force --options runtime --timestamp --sign \"$sign_identity\" \"$app\"";

        assert!(
            SCRIPT.find("plutil -lint").unwrap() < SCRIPT.find(outer_sign).unwrap(),
            "the standalone bundle must be signed after its final contents are written"
        );
        assert!(
            APP_SCRIPT.find(inner_sign).unwrap() < APP_SCRIPT.find(outer_sign).unwrap(),
            "the nested input method must be signed before dodo.app"
        );
        assert!(SCRIPT.contains("codesign --verify --deep --strict"));
        assert!(APP_SCRIPT.contains("codesign --verify --deep --strict"));
    }
}
