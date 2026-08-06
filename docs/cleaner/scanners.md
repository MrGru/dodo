# Cleaner scanners (Phase 4 groundwork)

## Scanner contract

`CleanerScanner` is the execution seam:

- `category()`
- `required_permissions()`
- `scan(context, progress, cancellation)`

Results are returned as typed `CategoryScanResult` values.

## Progress and cancellation

- `ProgressSink` receives incremental `ScanProgress` snapshots.
- `CancellationToken` is shared and polled by scanners.
- `ScanError::Cancelled` is handled without freezing UI.
- Shared filesystem traversal throttles progress to avoid one update per file.

## Current scanner set

- `src/cleaner/macos/scanners/user_cache.rs` is a real scanner.
- `src/cleaner/macos/scanners/system_junk.rs` is a second real scanner for safe recreatable junk roots.
- `src/cleaner/macos/scanners/large_old_files.rs` provides analysis for user folders with non-default selection.
- `src/cleaner/macos/scanners/mail_files.rs` provides Full-Disk-Access-gated Mail attachment/download analysis.
- `src/cleaner/macos/scanners/trash_bins.rs` analyzes Trash bins as review-only items.
- `src/cleaner/macos/scanners/installed_apps.rs` provides first-pass installed-app indexing.
- `src/cleaner/macos/scanners/orphaned_files.rs` provides Full-Disk-Access-gated orphan detection
  (Phase 10), reusing Phase 9's identity/location/confidence machinery in reverse.
- `src/cleaner/state/mock.rs` remains for unit tests that validate the orchestration layer in isolation.

### User Cache scanner

Current behavior:

- scans `~/Library/Caches` and `~/.cache` when present;
- aggregates by immediate child, so one cache root becomes one result row;
- skips symlinks and cross-filesystem descents;
- returns partial completeness when configured roots are missing;
- marks results as safe recreatable cache data with copy-path support in the UI.

### System Junk scanner

Current behavior:

- scans `~/Library/Logs` and `/tmp`;
- aggregates by immediate child;
- keeps bounded depth for `/tmp`;
- marks findings as safe recreatable items;
- wires those items into the same selection / reveal / Trash cleanup flow as User Cache.

### Large & Old Files scanner

Current behavior:

- scans `~/Downloads`, `~/Desktop`, `~/Documents`, and `~/Movies`;
- walks files individually rather than aggregating whole folders;
- flags files over 100 MiB or files older than one year;
- marks results as `UserData` and does **not** select them by default;
- still allows explicit reveal / copy / Trash cleanup after review.

### Trash Bins scanner

Current behavior:

- analyzes `~/.Trash`;
- best-effort analyzes `/Volumes/*/.Trashes/<uid>`;
- reports one row per bin/root;
- never bulk-selects or auto-selects results;
- exposes review-only actions today (Reveal in Finder, Copy path).

### Mail Files scanner

Current behavior:

- requires Full Disk Access before scanning;
- discovers versioned roots under both:
  - `~/Library/Mail/V*/MailData/...`
  - `~/Library/Containers/com.apple.mail/Data/Library/Mail/V*/MailData/...`
- limits itself to `Attachments` and `Downloads` descendants;
- marks results as `UserData` and does not select them by default;
- allows explicit review / reveal / copy / Trash cleanup inside those discovered roots only.

### Installed Apps scanner

Current behavior:

- scans `/Applications`, `~/Applications`, `/System/Applications`, and `/System/Applications/Utilities`;
- indexes top-level `.app` bundles only;
- parses `Contents/Info.plist` for:
  - bundle identifier,
  - display/name,
  - version,
  - executable;
- marks apps as review-only and never auto-selects them;
- grants `ItemCapability::UninstallApplication` to every non-system app, and withholds it entirely
  from `/System/Applications` bundles — the "Begin uninstall review" action has nothing to gate on
  for a system app rather than needing a separate risk check at click time;
- delegates `Info.plist` parsing to `src/cleaner/macos/applications/bundle.rs`, shared with the
  Phase 9 uninstall review workflow (see `docs/cleaner/application-matching.md` for identity
  normalization, leftover matching, confidence scoring and the review dialog).

### Orphaned Files scanner

Current behavior:

- requires Full Disk Access before scanning, same as Mail Files;
- builds an installed-app index from the same fixed root list `InstalledAppsScanner` uses
  (`scanners::installed_apps::installed_app_identities`), then walks the same fixed leftover
  location list Phase 9's uninstall review uses, flagging entries *no* installed app's identity
  explains (`macos::applications::orphans::find_orphans`);
- tags every candidate with an `OrphanReason` and scores it through Phase 9's confidence pipeline —
  see `docs/cleaner/application-matching.md`'s "Orphan detection" section for the full mapping;
- loads `cleaner-ignored-items.json` itself and filters out any path the user has marked "Keep", so
  a kept item does not reappear on rescan;
- marks results as review-only: only `Confirmed`, non-system-scope candidates default-select, and
  a system-scope candidate never gets `ItemCapability::MoveToTrash` (scan-only, matching Phase 9's
  system-scope leftovers);
- grants `ItemCapability::MarkAsKept` to every result, system-scope included, so "Keep" is always
  available even where cleanup is not;
- never detects CLI tools without an `.app` bundle — see `docs/cleaner/known-limitations.md`.

### Unimplemented categories

- Categories without a real scanner are surfaced as partial “coming later” results by the state layer.
- This avoids fake success while keeping the Cleaner navigation and Smart Care workflow wired end to end.

Cleanup is available only for explicit allow-listed roots. Review-only categories, such as Trash Bins today, have no destructive path yet.
