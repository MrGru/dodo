# Advanced tools (planned)

Advanced categories are modeled in `CleanerCategory`:

- AI Apps
- Xcode Junk *(not listed in the window — see below)*
- Homebrew Cache *(not listed in the window — see below)*
- Node Tooling Cache
- Docker Cache
- Universal Binaries *(not listed in the window — see below)*
- Language Files

## What the window lists

Three of those are deliberately absent from the Cleaner's navigation as of
2026-08-13. Their scanners, tests and cleanup paths are untouched and still
compiled — only the listing changed, and `CleanerCategory::HIDDEN` is the
entire switch: deleting a name from that one array puts the category back.
`CleanerCategory::is_visible` and `CleanerCategory::categories_for` are what
read it, and `core::category`'s unit tests pin both.

Because a scan is only ever started from a category's own pane, a hidden
category is not scanned at all — it has no row to select and therefore no
`Scan` button.

Universal Binaries is the one whose absence is more than a preference: it is
analysis-only, and its own per-item explanation says slice removal "is not yet
implemented", so the page could report a number and offer nothing to do about
it.

Phase 1 status:

- discovery/cleanup logic not implemented yet,
- no mutation tools are enabled,
- no destructive operations run automatically.

Future phases will keep high-risk operations out of automatic Smart Care cleanup and require explicit confirmation workflows.
