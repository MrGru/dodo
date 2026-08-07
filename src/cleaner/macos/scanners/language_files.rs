//! `CleanerCategory::LanguageFiles` (Phase 15): analysis-only. Discovers each
//! installed app's `Contents/Resources/*.lproj` localizations, sizes each
//! one, and flags — never removes — the ones the ticket says must always be
//! preserved: `Base.lproj`, the bundle's own development region, this
//! machine's preferred languages, and English as a fallback.
//!
//! # One item per `.lproj`, not one per app
//!
//! Unlike Universal Binaries (one item per binary, since a binary either has
//! multiple slices or does not), a language item is naturally per-locale:
//! each `.lproj` folder has its own size and its own protection verdict, and
//! the ticket asks to "show languages per app" — grouping by app
//! (`CleanableItem::group`) while keeping one item per locale gives both.
//!
//! # Protection is a reported verdict, not a filter
//!
//! A protected `.lproj` (`LanguageProtectionReason`) still becomes an item —
//! omitting it would contradict "show languages per app" — but with
//! `RiskLevel::Protected` and an [`ItemWarning`] naming the reason, the same
//! shape `universal_binaries` uses for a system app's binary. Every
//! *non*-protected language gets `RiskLevel::ApplicationMutation`, the same
//! tier Universal Binaries uses, since removing either kind of slice
//! invalidates the app's code signature identically. Every item, protected
//! or not, gets `SelectionPolicy::NeverBulkSelect`: there is no removal path
//! at all this phase, so nothing should ever look "selected" in a way that
//! implies otherwise.
//!
//! # Detecting preferred languages without a GPUI or Cocoa call
//!
//! `~/Library/Preferences/.GlobalPreferences.plist`'s `AppleLanguages` array
//! is the same ordered preference list `NSLocale.preferredLanguages` reads
//! at runtime, parsed here with the same `plist` crate every other Cleaner
//! Info.plist read already uses — no new dependency, no Cocoa call needed
//! just to read one preference file. A missing or malformed file falls back
//! to `["en"]`, which — combined with the unconditional English-fallback
//! rule below — never under-protects.
//!
//! # Full Disk Access
//!
//! None needed — `Contents/Resources` inside an already-readable
//! `/Applications`-family bundle is not a protected location.

use std::path::{Path, PathBuf};

use plist::Value;

use crate::cleaner::core::cancellation::CancellationToken;
use crate::cleaner::core::category::CleanerCategory;
use crate::cleaner::core::errors::ScanError;
use crate::cleaner::core::fs::measure_size;
use crate::cleaner::core::item::{
    CleanableItem, CleanableItemId, ItemMetadata, ItemWarning, LanguageMetadata,
    LanguageProtectionReason,
};
use crate::cleaner::core::permissions::MacPermission;
use crate::cleaner::core::progress::{ProgressSink, ScanPhase, ScanProgress};
use crate::cleaner::core::report::{CategoryScanResult, ScanCompleteness, ScanWarning};
use crate::cleaner::core::risk::{ItemCapability, RiskLevel, SelectionPolicy};
use crate::cleaner::core::scan_context::ScanContext;
use crate::cleaner::core::scanner::CleanerScanner;
use crate::cleaner::macos::applications::bundle::parse_bundle;
use crate::cleaner::macos::platform::is_any_bundle_running;

const DEFAULT_APP_ROOTS: &[&str] = &[
    "/Applications",
    "~/Applications",
    "/System/Applications",
    "/System/Applications/Utilities",
];

pub struct LanguageFilesScanner {
    roots: Vec<PathBuf>,
}

impl LanguageFilesScanner {
    pub fn new() -> Self {
        Self {
            roots: DEFAULT_APP_ROOTS.iter().map(PathBuf::from).collect(),
        }
    }

    #[cfg(test)]
    fn with_roots(roots: Vec<PathBuf>) -> Self {
        Self { roots }
    }
}

impl CleanerScanner for LanguageFilesScanner {
    fn category(&self) -> CleanerCategory {
        CleanerCategory::LanguageFiles
    }

    fn required_permissions(&self) -> &[MacPermission] {
        const NONE: &[MacPermission] = &[];
        NONE
    }

    fn scan(
        &self,
        context: &ScanContext,
        progress: &dyn ProgressSink,
        cancellation: &CancellationToken,
    ) -> Result<CategoryScanResult, ScanError> {
        progress.report(ScanProgress {
            category: CleanerCategory::LanguageFiles,
            phase: ScanPhase::Preparing,
            current_path: None,
            scanned_entries: 0,
            discovered_items: 0,
            discovered_bytes: 0,
        });

        let preferred_languages =
            read_preferred_languages(context.user_home.as_deref()).unwrap_or_default();
        let roots = resolve_roots(&self.roots, context.user_home.as_deref());
        let mut items = Vec::new();
        let mut warnings = Vec::new();
        let mut scanned_entries = 0;

        for root in roots {
            if cancellation.is_cancelled() {
                return Err(ScanError::Cancelled);
            }
            let read_dir = match std::fs::read_dir(&root) {
                Ok(read_dir) => read_dir,
                Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
                Err(err) => {
                    warnings.push(ScanWarning {
                        message: format!("{}: {err}", root.display()),
                    });
                    continue;
                }
            };
            for entry in read_dir.flatten() {
                if cancellation.is_cancelled() {
                    return Err(ScanError::Cancelled);
                }
                let app_path = entry.path();
                if !app_path
                    .extension()
                    .is_some_and(|extension| extension == "app")
                {
                    continue;
                }
                scanned_entries += 1;
                progress.report(ScanProgress {
                    category: CleanerCategory::LanguageFiles,
                    phase: ScanPhase::Classifying,
                    current_path: Some(app_path.clone()),
                    scanned_entries,
                    discovered_items: items.len() as u64,
                    discovered_bytes: items
                        .iter()
                        .map(|item: &CleanableItem| item.logical_size)
                        .sum(),
                });

                let Ok(bundle) = parse_bundle(app_path.as_path()) else {
                    continue;
                };
                let resources = app_path.join("Contents").join("Resources");
                let Ok(resource_entries) = std::fs::read_dir(&resources) else {
                    continue;
                };

                let running = bundle
                    .bundle_id
                    .as_deref()
                    .map(|id| is_any_bundle_running(&[id]))
                    .unwrap_or(false);

                for resource_entry in resource_entries.flatten() {
                    let lproj_path = resource_entry.path();
                    let Some(code) = lproj_language_code(lproj_path.as_path()) else {
                        continue;
                    };
                    items.push(build_item(
                        bundle.display_name.as_str(),
                        bundle.is_system_app,
                        bundle.development_region.as_deref(),
                        &preferred_languages,
                        lproj_path.as_path(),
                        &code,
                        running,
                    ));
                }
            }
        }

        items.sort_by_key(|item| std::cmp::Reverse(item.logical_size));
        Ok(CategoryScanResult {
            category: CleanerCategory::LanguageFiles,
            items,
            scanned_entries,
            estimated_reclaimable_bytes: 0,
            warnings,
            completeness: ScanCompleteness::Complete,
        })
    }
}

fn build_item(
    app_name: &str,
    is_system_app: bool,
    development_region: Option<&str>,
    preferred_languages: &[String],
    lproj_path: &Path,
    language_code: &str,
    app_running: bool,
) -> CleanableItem {
    let protection_reason =
        protection_reason_for(language_code, development_region, preferred_languages);
    let is_protected = is_system_app || protection_reason.is_some();
    let size = measure_size(
        lproj_path,
        CleanerCategory::LanguageFiles,
        RiskLevel::ApplicationMutation,
    );

    let mut warnings = Vec::new();
    if is_system_app {
        warnings.push(ItemWarning {
            message: "System application — Cleaner never mutates a system app's resources."
                .to_string(),
        });
    } else if let Some(reason) = protection_reason {
        warnings.push(ItemWarning {
            message: protection_explanation(reason).to_string(),
        });
    } else {
        warnings.push(ItemWarning {
            message: "Removing a localization invalidates this app's code signature; a future \
                       update may restore it."
                .to_string(),
        });
    }
    if app_running {
        warnings.push(ItemWarning {
            message: format!("{app_name} is currently running."),
        });
    }

    CleanableItem {
        id: item_id(lproj_path),
        category: CleanerCategory::LanguageFiles,
        group: Some(app_name.to_string()),
        display_name: language_code.to_string(),
        path: lproj_path.to_path_buf(),
        logical_size: size,
        allocated_size: None,
        modified_at: std::fs::metadata(lproj_path)
            .ok()
            .and_then(|metadata| metadata.modified().ok()),
        last_accessed_at: None,
        risk: if is_protected {
            RiskLevel::Protected
        } else {
            RiskLevel::ApplicationMutation
        },
        selection_policy: SelectionPolicy::NeverBulkSelect,
        capabilities: vec![ItemCapability::RevealInFinder, ItemCapability::CopyPath],
        explanation: format!(
            "Localization resources for \"{language_code}\" inside {app_name}. Analysis only \
             this phase — removal is not yet implemented."
        ),
        warnings,
        metadata: ItemMetadata::Language(LanguageMetadata {
            language_code: language_code.to_string(),
            protection_reason: if is_system_app {
                None
            } else {
                protection_reason
            },
        }),
    }
}

fn protection_reason_for(
    language_code: &str,
    development_region: Option<&str>,
    preferred_languages: &[String],
) -> Option<LanguageProtectionReason> {
    if language_code.eq_ignore_ascii_case("Base") {
        return Some(LanguageProtectionReason::BaseLocalization);
    }
    if let Some(region) = development_region
        && primary_subtag_matches(language_code, region)
    {
        return Some(LanguageProtectionReason::DevelopmentRegion);
    }
    if preferred_languages
        .iter()
        .any(|preferred| primary_subtag_matches(language_code, preferred))
    {
        return Some(LanguageProtectionReason::PreferredLanguage);
    }
    if primary_subtag_matches(language_code, "en") {
        return Some(LanguageProtectionReason::EnglishFallback);
    }
    None
}

fn protection_explanation(reason: LanguageProtectionReason) -> &'static str {
    match reason {
        LanguageProtectionReason::BaseLocalization => {
            "Base localization — storyboard and XIB strings with no dedicated translation. \
             Never a removal candidate."
        }
        LanguageProtectionReason::PreferredLanguage => {
            "Matches one of this Mac's preferred languages (System Settings ▸ General ▸ \
             Language & Region)."
        }
        LanguageProtectionReason::DevelopmentRegion => {
            "This app's development region — its non-localized resources are written in this \
             language."
        }
        LanguageProtectionReason::EnglishFallback => {
            "Kept as the English fallback regardless of preferred-language settings."
        }
    }
}

/// Whether two language identifiers share a primary subtag, case-insensitive
/// (`"zh-Hans"` vs. `"zh-Hant-TW"` → `true` on `"zh"`; `"en"` vs. `"en-US"` →
/// `true`). Both `.lproj` names and `AppleLanguages`/
/// `CFBundleDevelopmentRegion` entries use a leading primary subtag before
/// any region/script suffix, so this is enough without a full BCP-47 parse.
fn primary_subtag_matches(left: &str, right: &str) -> bool {
    let left = left.split(['-', '_']).next().unwrap_or(left);
    let right = right.split(['-', '_']).next().unwrap_or(right);
    left.eq_ignore_ascii_case(right)
}

/// `<name>.lproj` → `Some(name)`; anything else → `None`.
fn lproj_language_code(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_str()?;
    name.strip_suffix(".lproj").map(ToOwned::to_owned)
}

/// This machine's ordered preferred-language list
/// (`NSGlobalDomain.AppleLanguages`), read directly from
/// `.GlobalPreferences.plist` rather than a Cocoa call. `None` when the file
/// is missing or not a dictionary with a string array at that key — callers
/// treat that the same as an empty list.
fn read_preferred_languages(home: Option<&Path>) -> Option<Vec<String>> {
    let home = home?;
    let path = home
        .join("Library")
        .join("Preferences")
        .join(".GlobalPreferences.plist");
    let value = Value::from_file(path).ok()?;
    let languages = value
        .as_dictionary()?
        .get("AppleLanguages")?
        .as_array()?
        .iter()
        .filter_map(Value::as_string)
        .map(ToOwned::to_owned)
        .collect();
    Some(languages)
}

fn resolve_roots(roots: &[PathBuf], home: Option<&Path>) -> Vec<PathBuf> {
    roots
        .iter()
        .filter_map(|root| {
            let Some(path) = root.to_str() else {
                return Some(root.clone());
            };
            if !path.starts_with("~/") {
                return Some(root.clone());
            }
            home.map(|home| home.join(&path[2..]))
        })
        .collect()
}

fn item_id(path: &Path) -> CleanableItemId {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
    CleanableItemId(hasher.finish())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use crate::cleaner::core::cancellation::CancellationToken;
    use crate::cleaner::core::item::{ItemMetadata, LanguageProtectionReason};
    use crate::cleaner::core::progress::{ProgressSink, ScanProgress};
    use crate::cleaner::core::risk::{RiskLevel, SelectionPolicy};
    use crate::cleaner::core::scan_context::ScanContext;
    use crate::cleaner::core::scanner::CleanerScanner;

    use super::{LanguageFilesScanner, primary_subtag_matches, protection_reason_for};

    struct RecordingSink;
    impl ProgressSink for RecordingSink {
        fn report(&self, _progress: ScanProgress) {}
    }

    fn context(home: std::path::PathBuf) -> ScanContext {
        ScanContext {
            started_at: std::time::SystemTime::now(),
            user_home: Some(home),
        }
    }

    fn write_app(
        apps_root: &std::path::Path,
        name: &str,
        languages: &[&str],
    ) -> std::path::PathBuf {
        let app = apps_root.join(name);
        let resources = app.join("Contents").join("Resources");
        fs::create_dir_all(&resources).expect("creates Resources dir");
        fs::write(
            app.join("Contents").join("Info.plist"),
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>com.example.app</string>
<key>CFBundleExecutable</key><string>Example</string>
</dict></plist>"#,
        )
        .expect("writes plist");
        for language in languages {
            let lproj = resources.join(format!("{language}.lproj"));
            fs::create_dir_all(&lproj).expect("creates lproj dir");
            fs::write(lproj.join("Localizable.strings"), vec![0u8; 16]).expect("writes strings");
        }
        app
    }

    #[test]
    fn primary_subtag_matching_ignores_region_and_script() {
        assert!(primary_subtag_matches("zh-Hans", "zh-Hant-TW"));
        assert!(primary_subtag_matches("en", "en-US"));
        assert!(!primary_subtag_matches("en", "fr"));
    }

    #[test]
    fn base_localization_is_always_protected() {
        assert_eq!(
            protection_reason_for("Base", None, &[]),
            Some(LanguageProtectionReason::BaseLocalization)
        );
    }

    #[test]
    fn english_is_protected_even_with_no_preferred_languages() {
        assert_eq!(
            protection_reason_for("en", None, &[]),
            Some(LanguageProtectionReason::EnglishFallback)
        );
    }

    #[test]
    fn an_unrelated_language_is_not_protected() {
        assert_eq!(
            protection_reason_for("fr", Some("en"), &["en-US".to_string()]),
            None
        );
    }

    #[test]
    fn scanner_reports_one_item_per_lproj_grouped_by_app() {
        let temp = std::env::temp_dir().join(format!(
            "dodo-cleaner-language-files-{}",
            std::process::id()
        ));
        let apps = temp.join("Applications");
        write_app(&apps, "Example.app", &["en", "fr", "Base"]);

        let scanner = LanguageFilesScanner::with_roots(vec![apps.clone()]);
        let result = scanner
            .scan(
                &context(temp.clone()),
                &RecordingSink,
                &CancellationToken::new(),
            )
            .expect("scans");

        assert_eq!(result.items.len(), 3);
        assert!(
            result
                .items
                .iter()
                .all(|item| item.group.as_deref() == Some("Example"))
        );

        let french = result
            .items
            .iter()
            .find(|item| item.display_name == "fr")
            .expect("finds the French localization");
        assert_eq!(french.risk, RiskLevel::ApplicationMutation);
        assert_eq!(french.selection_policy, SelectionPolicy::NeverBulkSelect);

        let base = result
            .items
            .iter()
            .find(|item| item.display_name == "Base")
            .expect("finds Base");
        assert_eq!(base.risk, RiskLevel::Protected);
        assert!(matches!(
            &base.metadata,
            ItemMetadata::Language(metadata)
                if metadata.protection_reason == Some(LanguageProtectionReason::BaseLocalization)
        ));

        fs::remove_dir_all(&temp).expect("removes temp tree");
    }
}
