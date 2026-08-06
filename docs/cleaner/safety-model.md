# Cleaner safety model (Phase 4 groundwork)

## Core principles

- Scanning is discovery-only: scanners never delete.
- Cleanup is a separate action path (not implemented in phase 1).
- Unsupported platforms must be explicit.
- No destructive fallback behavior exists.

## Selection defaults

Domain types model explicit risk and selection intent:

- `RiskLevel`
- `SelectionPolicy`
- `ItemCapability`

The first real User Cache scan uses recreatable-safe defaults only.

## Prepared enforcement helpers

Core types and helpers now exist for future enforcement:

- `DeletionPolicy`
- `AllowedRoot`
- `SafetyError`
- `contains_path`
- `dedupe_nested_paths`
- `validate_path`

Current guarantees:

- containment checks compare path components, so `/tmp/foo` does not authorize `/tmp/foobar`;
- selecting a parent path removes nested children from the cleanup set;
- symlinks are rejected before cleanup;
- deleting an allowed root itself is rejected unless explicitly permitted;
- protected roots can block deleting themselves or an ancestor path;
- current cleanup allow-lists cover only explicitly scanned safe roots (`~/Library/Caches`, `~/.cache`, `~/Library/Logs`, `/tmp`);
- Phase 9 extends the allow-list with `/Applications`, `~/Applications` and every user-scope
  leftover location (`~/Library/Application Support`, `Caches`, `Preferences`, `Containers`,
  `Group Containers`, `Logs`, `Saved Application State`, `LaunchAgents`, `WebKit`, `HTTPStorages`,
  `Cookies`, `Services`, `Autosave Information`) for `CleanerCategory::InstalledApps` only — the
  matching system-scope roots (`/Library/...`) are deliberately absent, so an uninstall-review
  candidate found there fails `OutsideAllowedRoot` even if a UI bug ever let it through selected;
- Phase 10 adds `CleanerCategory::OrphanedFiles` to those same user-scope `AllowedRoot` entries
  rather than a second, duplicate set — orphan candidates are found under the identical location
  list — and, like Phase 9, never adds the matching system-scope roots;
- Phase 11 adds `CleanerCategory::XcodeJunk`, scoped to exactly three sub-paths
  (`Xcode/DerivedData`, `Xcode/UserData/Previews`, `CoreSimulator/Caches` —
  `xcode_junk::cleanup_allowed_roots`) even though the scanner reports eight; Archives, iOS
  DeviceSupport, CoreSimulator Devices, XCTestDevices and the SwiftPM cache stay scan-only. Phase
  11 also adds `CleanerCategory::HomebrewCache`, scoped to the one cache root
  `homebrew_cache::resolve_cache_root` detects — never `/opt/homebrew`, `/usr/local` or the
  Cellar — with the scanner and cleanup calling that same function so the two can never disagree
  on where the root is;
- cleanup uses native macOS Trash moves rather than permanent deletion;
- review-oriented categories can exist without a cleanup capability (Trash Bins currently does).

These are the boundary for allow-list based path validation before any future Trash move operation.
