//! `Info.plist` parsing shared by the Installed Apps scanner and the uninstall
//! review workflow.
//!
//! This lived only inside `scanners/installed_apps.rs` before Phase 9. Uninstall
//! review needs the same bundle identifier, display name and version to build an
//! [`crate::cleaner::macos::applications::identity::AppIdentity`], so the parsing
//! logic moved here rather than being duplicated.

use std::fs;
use std::path::Path;
use std::time::SystemTime;

use plist::Value;

/// Fields pulled out of an `.app` bundle's `Contents/Info.plist`, plus the
/// small amount of filesystem metadata (`modified_at`, `is_system_app`) that
/// callers need alongside it.
pub struct ParsedBundle {
    pub display_name: String,
    pub bundle_id: Option<String>,
    pub version: Option<String>,
    pub executable: Option<String>,
    pub modified_at: Option<SystemTime>,
    pub explanation: String,
    pub is_system_app: bool,
    /// `CFBundleDevelopmentRegion` — the language the bundle's non-localized
    /// resources are written in. Used by
    /// `macos::scanners::language_files` to protect the matching `.lproj`
    /// from ever being presented as a removal candidate.
    pub development_region: Option<String>,
}

/// Parses `<path>/Contents/Info.plist`.
///
/// Returns `Err` when the plist is missing or cannot be parsed as a
/// dictionary (a malformed or absent `Info.plist`); individual missing keys
/// inside a valid dictionary are tolerated and simply leave the corresponding
/// field `None`.
pub fn parse_bundle(path: &Path) -> Result<ParsedBundle, String> {
    let plist_path = path.join("Contents").join("Info.plist");
    let value = Value::from_file(&plist_path).map_err(|error| error.to_string())?;
    let dict = value
        .into_dictionary()
        .ok_or_else(|| "Info.plist is not a dictionary".to_string())?;
    let bundle_id = dict
        .get("CFBundleIdentifier")
        .and_then(Value::as_string)
        .map(ToOwned::to_owned);
    let display_name = dict
        .get("CFBundleDisplayName")
        .and_then(Value::as_string)
        .or_else(|| dict.get("CFBundleName").and_then(Value::as_string))
        .or_else(|| path.file_stem().and_then(|name| name.to_str()))
        .unwrap_or("Application")
        .to_string();
    let version = dict
        .get("CFBundleShortVersionString")
        .and_then(Value::as_string)
        .or_else(|| dict.get("CFBundleVersion").and_then(Value::as_string))
        .map(ToOwned::to_owned);
    let executable = dict
        .get("CFBundleExecutable")
        .and_then(Value::as_string)
        .map(ToOwned::to_owned);
    let modified_at = fs::metadata(path)
        .ok()
        .and_then(|metadata| metadata.modified().ok());
    let is_system_app = is_system_app_path(path);
    let development_region = dict
        .get("CFBundleDevelopmentRegion")
        .and_then(Value::as_string)
        .map(ToOwned::to_owned);
    let explanation = match (&bundle_id, &version) {
        (Some(bundle_id), Some(version)) => format!("{bundle_id} · version {version}"),
        (Some(bundle_id), None) => bundle_id.clone(),
        (None, Some(version)) => format!("Version {version}"),
        (None, None) => "Installed application bundle".into(),
    };

    Ok(ParsedBundle {
        display_name,
        bundle_id,
        version,
        executable,
        modified_at,
        explanation,
        is_system_app,
        development_region,
    })
}

/// Whether `path` sits under the read-only system application root. Shared so
/// the scanner and the uninstall-review refusal check agree on the same rule.
pub fn is_system_app_path(path: &Path) -> bool {
    path.starts_with("/System/Applications")
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{is_system_app_path, parse_bundle};

    fn write_plist(dir: &std::path::Path, contents: &str) {
        let contents_dir = dir.join("Contents");
        fs::create_dir_all(&contents_dir).expect("creates Contents dir");
        fs::write(contents_dir.join("Info.plist"), contents).expect("writes plist");
    }

    #[test]
    fn malformed_plist_is_rejected() {
        let temp = std::env::temp_dir().join(format!(
            "dodo-cleaner-bundle-malformed-{}",
            std::process::id()
        ));
        let app = temp.join("Broken.app");
        write_plist(app.as_path(), "not a plist at all");

        assert!(parse_bundle(app.as_path()).is_err());

        fs::remove_dir_all(&temp).expect("removes temp tree");
    }

    #[test]
    fn system_app_path_detection_is_a_pure_prefix_check() {
        assert!(is_system_app_path(std::path::Path::new(
            "/System/Applications/Notes.app"
        )));
        assert!(!is_system_app_path(std::path::Path::new(
            "/Applications/Notes.app"
        )));
    }

    #[test]
    fn missing_plist_is_rejected() {
        let temp = std::env::temp_dir().join(format!(
            "dodo-cleaner-bundle-missing-{}",
            std::process::id()
        ));
        let app = temp.join("NoPlist.app");
        fs::create_dir_all(app.join("Contents")).expect("creates Contents dir without a plist");

        assert!(parse_bundle(app.as_path()).is_err());

        fs::remove_dir_all(&temp).expect("removes temp tree");
    }
}
