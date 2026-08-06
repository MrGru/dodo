# Cleaner architecture (Phase 4 groundwork)

Cleaner is implemented as a normal feature module inside the existing `dodo` crate:

- `src/cleaner/core/`: domain contracts, scan-root definitions, filesystem aggregation, and safety helpers with no GPUI dependency.
- `src/cleaner/state/`: scan orchestration and UI state transitions.
- `src/cleaner/views/`: GPUI rendering only.
- `src/cleaner/macos/`: macOS-only implementation boundary (`#[cfg(target_os = "macos")]`), now including scanner registry, cleanup logic, and Finder/Trash platform calls.

## Sidebar and routing integration

Cleaner is wired as a top-level tool in:

- `src/main.rs` module registration
- `src/layout.rs` `View` enum, `View::ALL`, title/icon mapping, entity creation, pane render match
- `src/app_icon.rs` icon registration
- `src/i18n.rs` localized titles and labels

## Data flow

1. UI triggers scan from [`CleanerView`](/Users/apple/Downloads/projects/dodo/src/cleaner/views/cleaner_view.rs).
2. [`CleanerState`](/Users/apple/Downloads/projects/dodo/src/cleaner/state/cleaner_state.rs) moves `Idle -> Scanning`.
3. Smart Care scans every category; other sections scan only the selected category.
4. Registered scanners run on the background executor via [`CleanerScanner`](/Users/apple/Downloads/projects/dodo/src/cleaner/core/scanner.rs).
5. Progress events stream through a channel pump into incremental UI updates.
6. Category results accumulate into summary fields and per-category result panels.
7. Missing category implementations become explicit partial results rather than fake success.
8. Completion transitions to `Completed`, `PartiallyCompleted`, or `CompletedWithFailures`.

## Concurrency strategy

- Background work runs off UI thread.
- Progress is throttled in the filesystem engine and then batched again in the UI pump (no per-file notify flood).
- Cancellation uses shared `CancellationToken` checked by scanner loop.
- No one-task-per-file fan-out.
- Filesystem aggregation is bounded and sequential for now; future phases can add measured parallelism without changing the view layer.

## Current real platform slice

- `src/cleaner/core/fs.rs` provides shared root scanning and aggregation.
- `src/cleaner/core/scan_root.rs` defines per-root traversal rules.
- `src/cleaner/core/safety.rs` now enforces component-based containment, root-deletion rejection, symlink rejection, and nested-path dedup helpers.
- `src/cleaner/macos/scanners/user_cache.rs` aggregates `~/Library/Caches` and `~/.cache` by top-level cache root.
- `src/cleaner/macos/scanners/system_junk.rs` aggregates safe recreatable roots (`~/Library/Logs`, `/tmp`) conservatively.
- `src/cleaner/macos/scanners/large_old_files.rs` scans user folders file-by-file for large / old file analysis with conservative non-default selection.
- `src/cleaner/macos/scanners/mail_files.rs` discovers versioned Mail attachment/download roots and treats them as explicit user-data review items.
- `src/cleaner/macos/scanners/trash_bins.rs` analyzes Trash bins as review-only roots.
- `src/cleaner/macos/scanners/installed_apps.rs` indexes `.app` bundles from the standard top-level application roots and extracts basic Info.plist metadata.
- `src/cleaner/macos/scanners/xcode_junk.rs` (Phase 11) analyzes eight fixed roots under
  `~/Library/Developer` and `~/Library/org.swift.swiftpm`, and
  `src/cleaner/macos/scanners/homebrew_cache.rs` (Phase 11) analyzes Homebrew's download cache —
  see `docs/cleaner/scanners.md`.
- `src/cleaner/macos/platform/xcode.rs` (Phase 11) adds a read-only `NSRunningApplication` check
  the Xcode Junk scanner uses to warn on `DerivedData`, alongside the existing Finder/Trash calls.
- `src/cleaner/macos/applications/` (Phase 9, extended in Phase 10) holds app identity
  normalization, leftover-location matching, confidence scoring, the uninstall review workflow and
  (Phase 10) the inverse question, orphan detection (`applications/orphans.rs`) — shared by the
  `InstalledApps`/`OrphanedFiles` scanners and the review dialog, so it is a sibling of `scanners/`
  rather than a member of it. See `docs/cleaner/application-matching.md`.
- `src/cleaner/core/ignore.rs` and `src/cleaner/services/ignore_store.rs` (Phase 10) hold the
  orphan-detection "keep" list — `cleaner-ignored-items.json`, dodo's eighth persisted file — behind
  the same trait/versioned-JSON discipline every other store here uses.
- `src/cleaner/views/uninstall_review_dialog.rs` renders the uninstall review dialog as its own
  entity (per the `saved_query_form` / `row_editor` pattern), opened from an Installed Apps row.
- `src/cleaner/macos/cleanup.rs` owns cleanup planning and allow-list validation, separate from GPUI rendering.
- `src/cleaner/macos/permissions/` owns Full Disk Access checks and settings/reveal actions.
- `src/cleaner/macos/platform/` isolates native Finder and Trash calls behind small Rust helpers.
- Other categories remain explicitly incomplete rather than pretending to scan.

## Platform strategy

- macOS-first behavior is real.
- Non-macOS shows explicit unsupported UI text.
- No fake Windows/Linux scanners are registered.
- Future Windows/Linux support can be added as sibling modules under `src/cleaner/`.
