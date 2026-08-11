//! `CleanerCategory::UniversalBinaries` (Phase 14): analysis-only. Discovers
//! which installed apps' main executable is a universal (fat) Mach-O binary,
//! reports its architectures and an estimated removable-slice size, and
//! **mutates nothing** — the ticket is explicit that thinning is Phase 16's
//! concern, gated on this analysis, a tested backup/rollback path, and a
//! post-operation signature check that do not exist yet.
//!
//! # Reusing the Installed Apps groundwork
//!
//! Bundle enumeration and `Info.plist` parsing are the exact same job
//! `InstalledAppsScanner` already does, so this scanner walks the same four
//! standard roots and calls the same
//! [`applications::bundle::parse_bundle`] — see that module's doc comment
//! for why it moved out of `installed_apps.rs` in the first place. Only the
//! *main* executable (`Contents/MacOS/<CFBundleExecutable>`) is inspected;
//! nested frameworks, plugins and helper tools are not walked, matching the
//! ticket's "do not enumerate package contents unless required" and keeping
//! this a bounded, one-file-per-app read rather than a second full bundle
//! crawl. See `docs/cleaner/known-limitations.md`.
//!
//! # Reading architectures without executing anything
//!
//! [`read_architectures`] uses the `object` crate purely as a binary-format
//! reader: [`object::FileKind::parse`] identifies a fat vs. thin Mach-O from
//! the first bytes, then either [`object::read::macho::MachOFatFile32`]/
//! `MachOFatFile64` (for a universal binary, giving each slice's
//! architecture and exact byte size) or [`object::File::parse`] (for a
//! single-architecture binary) reads the rest. Nothing here runs `lipo`,
//! `file`, or the binary itself.
//!
//! # Signing status via `codesign`, not a hand-rolled CMS parser
//!
//! [`signing_status`] shells out to `/usr/bin/codesign --verify --no-strict`
//! (an argument vector, no shell) and reports only whether it *exited zero*
//! — a coarse "verified or not" signal, not the identity or entitlements a
//! deeper inspection would need. `None` means the check itself could not run
//! (`codesign` missing), never a false verdict.
//!
//! # Full Disk Access
//!
//! None needed — every root here is the same ordinary, non-protected
//! `/Applications`-family location `InstalledAppsScanner` already reads
//! without it.

use std::path::{Path, PathBuf};
use std::process::Command;

use object::read::macho::{FatArch as _, MachOFatFile32, MachOFatFile64};
use object::{Architecture, FileKind, Object as _};

use crate::cleaner::core::cancellation::CancellationToken;
use crate::cleaner::core::category::CleanerCategory;
use crate::cleaner::core::errors::ScanError;
use crate::cleaner::core::item::{
    CleanableItem, CleanableItemId, ItemMetadata, ItemWarning, UniversalBinaryMetadata,
};
use crate::cleaner::core::permissions::MacPermission;
use crate::cleaner::core::progress::{ProgressSink, ScanPhase, ScanProgress};
use crate::cleaner::core::report::{CategoryScanResult, ScanCompleteness, ScanWarning};
use crate::cleaner::core::risk::{RiskLevel, SelectionPolicy};
use crate::cleaner::core::scan_context::ScanContext;
use crate::cleaner::core::scanner::CleanerScanner;
use crate::cleaner::macos::applications::bundle::parse_bundle;
use crate::cleaner::macos::platform::{application_icon_tiff, is_any_bundle_running};

const DEFAULT_APP_ROOTS: &[&str] = &[
    "/Applications",
    "~/Applications",
    "/System/Applications",
    "/System/Applications/Utilities",
];

pub struct UniversalBinariesScanner {
    roots: Vec<PathBuf>,
}

impl UniversalBinariesScanner {
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

impl CleanerScanner for UniversalBinariesScanner {
    fn category(&self) -> CleanerCategory {
        CleanerCategory::UniversalBinaries
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
            category: CleanerCategory::UniversalBinaries,
            phase: ScanPhase::Preparing,
            current_path: None,
            scanned_entries: 0,
            discovered_items: 0,
            discovered_bytes: 0,
        });

        let current_arch = current_architecture();
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
                let path = entry.path();
                if !path.extension().is_some_and(|extension| extension == "app") {
                    continue;
                }
                scanned_entries += 1;
                progress.report(ScanProgress {
                    category: CleanerCategory::UniversalBinaries,
                    phase: ScanPhase::Classifying,
                    current_path: Some(path.clone()),
                    scanned_entries,
                    discovered_items: items.len() as u64,
                    discovered_bytes: items
                        .iter()
                        .map(|item: &CleanableItem| item.logical_size)
                        .sum(),
                });

                let Ok(bundle) = parse_bundle(path.as_path()) else {
                    continue;
                };
                let Some(executable) = bundle.executable.as_deref() else {
                    continue;
                };
                let binary_path = path.join("Contents").join("MacOS").join(executable);
                let Ok(data) = std::fs::read(&binary_path) else {
                    continue;
                };
                let Ok(slices) = read_architectures(&data) else {
                    continue;
                };
                if slices.len() < 2 {
                    continue;
                }

                items.push(build_item(
                    path.as_path(),
                    bundle.display_name.as_str(),
                    bundle.bundle_id.as_deref(),
                    bundle.is_system_app,
                    &slices,
                    current_arch,
                    binary_path.as_path(),
                ));
            }
        }

        items.sort_by_key(|item| std::cmp::Reverse(item.logical_size));
        Ok(CategoryScanResult {
            category: CleanerCategory::UniversalBinaries,
            items,
            scanned_entries,
            estimated_reclaimable_bytes: 0,
            warnings,
            completeness: ScanCompleteness::Complete,
        })
    }
}

fn build_item(
    app_path: &Path,
    display_name: &str,
    bundle_id: Option<&str>,
    is_system_app: bool,
    slices: &[(Architecture, u64)],
    current_arch: Architecture,
    binary_path: &Path,
) -> CleanableItem {
    let architectures: Vec<String> = slices
        .iter()
        .map(|(arch, _)| architecture_label(*arch).to_string())
        .collect();
    let removable_bytes: u64 = slices
        .iter()
        .filter(|(arch, _)| *arch != current_arch)
        .map(|(_, size)| size)
        .sum();
    let total_size: u64 = slices.iter().map(|(_, size)| size).sum();
    let signed = signing_status(app_path);
    let running = bundle_id
        .map(|id| is_any_bundle_running(&[id]))
        .unwrap_or(false);

    let mut warnings = Vec::new();
    if is_system_app {
        warnings.push(ItemWarning {
            message: "System application — Cleaner never mutates a system app's binary."
                .to_string(),
        });
    }
    if running {
        warnings.push(ItemWarning {
            message: format!("{display_name} is currently running."),
        });
    }
    if signed == Some(true) {
        warnings.push(ItemWarning {
            message: "Removing a slice invalidates this app's code signature; a future update \
                       may restore the removed architecture."
                .to_string(),
        });
    }

    CleanableItem {
        id: item_id(app_path),
        category: CleanerCategory::UniversalBinaries,
        group: Some("Universal binaries".to_string()),
        display_name: display_name.to_string(),
        path: app_path.to_path_buf(),
        logical_size: total_size,
        allocated_size: None,
        modified_at: std::fs::metadata(binary_path)
            .ok()
            .and_then(|metadata| metadata.modified().ok()),
        last_accessed_at: None,
        risk: if is_system_app {
            RiskLevel::Protected
        } else {
            RiskLevel::ApplicationMutation
        },
        selection_policy: SelectionPolicy::NeverBulkSelect,
        capabilities: vec![
            crate::cleaner::core::risk::ItemCapability::RevealInFinder,
            crate::cleaner::core::risk::ItemCapability::CopyPath,
        ],
        explanation: format!(
            "Built for {}. Analysis only this phase — removing a non-{} slice is not yet \
             implemented; it will require a signature re-check and rollback before it ships.",
            architectures.join(" + "),
            architecture_label(current_arch),
        ),
        warnings,
        metadata: ItemMetadata::UniversalBinary(UniversalBinaryMetadata {
            architectures,
            current_architecture: architecture_label(current_arch).to_string(),
            estimated_removable_bytes: removable_bytes,
            signed,
            icon_tiff: application_icon_tiff(app_path),
        }),
    }
}

/// This machine's own architecture, in Mach-O naming (`arm64`/`x86_64`)
/// rather than Rust's `std::env::consts::ARCH` (`aarch64`/`x86_64`).
fn current_architecture() -> Architecture {
    match std::env::consts::ARCH {
        "aarch64" => Architecture::Aarch64,
        "x86_64" => Architecture::X86_64,
        "x86" => Architecture::I386,
        "arm" => Architecture::Arm,
        _ => Architecture::Unknown,
    }
}

fn architecture_label(architecture: Architecture) -> &'static str {
    match architecture {
        Architecture::Aarch64 => "arm64",
        Architecture::X86_64 => "x86_64",
        Architecture::I386 => "i386",
        Architecture::Arm => "arm",
        _ => "unknown",
    }
}

/// Reads every architecture slice in a Mach-O binary and its exact byte
/// size. A single-architecture ("thin") binary reports one slice whose size
/// is the whole file — `read_architectures` never mutates the input, and
/// [`CleanerScanner::scan`] discards results with fewer than two slices, so
/// only genuinely universal binaries ever become an item.
fn read_architectures(data: &[u8]) -> Result<Vec<(Architecture, u64)>, String> {
    let kind = FileKind::parse(data).map_err(|error| error.to_string())?;
    match kind {
        FileKind::MachOFat32 => {
            let fat = MachOFatFile32::parse(data).map_err(|error| error.to_string())?;
            Ok(fat
                .arches()
                .iter()
                .map(|arch| (arch.architecture(), arch.size().into()))
                .collect())
        }
        FileKind::MachOFat64 => {
            let fat = MachOFatFile64::parse(data).map_err(|error| error.to_string())?;
            Ok(fat
                .arches()
                .iter()
                .map(|arch| (arch.architecture(), arch.size()))
                .collect())
        }
        FileKind::MachO32 | FileKind::MachO64 => {
            let file = object::File::parse(data).map_err(|error| error.to_string())?;
            Ok(vec![(file.architecture(), data.len() as u64)])
        }
        other => Err(format!("not a Mach-O executable: {other:?}")),
    }
}

/// `true`/`false` from `codesign --verify`'s exit status; `None` only when
/// the tool itself could not be launched.
fn signing_status(app_path: &Path) -> Option<bool> {
    Command::new("/usr/bin/codesign")
        .args(["--verify", "--no-strict", "--"])
        .arg(app_path)
        .output()
        .ok()
        .map(|output| output.status.success())
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
    use crate::cleaner::core::progress::{ProgressSink, ScanProgress};
    use crate::cleaner::core::scan_context::ScanContext;
    use crate::cleaner::core::scanner::CleanerScanner;

    use super::{UniversalBinariesScanner, read_architectures};

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

    #[test]
    fn a_thin_binary_yields_no_item() {
        // A real thin Mach-O header (64-bit, arm64) with no load commands —
        // just enough for `object::FileKind::parse` and `object::File::parse`
        // to succeed and report exactly one architecture.
        let mut data = vec![0u8; 32];
        data[0..4].copy_from_slice(&0xfeedfacfu32.to_le_bytes()); // MH_MAGIC_64
        data[4..8].copy_from_slice(&0x0100000cu32.to_le_bytes()); // CPU_TYPE_ARM64
        let slices = read_architectures(&data).expect("parses a thin Mach-O header");
        assert_eq!(slices.len(), 1);
    }

    #[test]
    fn scanner_skips_bundles_with_no_readable_executable() {
        let temp = std::env::temp_dir().join(format!(
            "dodo-cleaner-universal-binaries-{}",
            std::process::id()
        ));
        let apps = temp.join("Applications");
        let app = apps.join("Example.app").join("Contents");
        fs::create_dir_all(&app).expect("creates app bundle");
        fs::write(
            app.join("Info.plist"),
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>CFBundleIdentifier</key><string>com.example.app</string>
<key>CFBundleExecutable</key><string>Example</string>
</dict></plist>"#,
        )
        .expect("writes plist");

        let scanner = UniversalBinariesScanner::with_roots(vec![apps.clone()]);
        let result = scanner
            .scan(
                &context(temp.clone()),
                &RecordingSink,
                &CancellationToken::new(),
            )
            .expect("scans without the executable present");

        assert!(result.items.is_empty());
        fs::remove_dir_all(&temp).expect("removes temp tree");
    }
}
