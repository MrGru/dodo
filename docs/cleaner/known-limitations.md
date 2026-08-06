# Known limitations (after the first real scanner)

- Real filesystem scans now cover User Cache, System Junk safe roots, Large & Old Files, Mail Files, Trash Bins analysis, and basic Installed Apps indexing.
- Cleanup execution still covers only explicit allow-listed roots.
- Smart Care still reports most categories as planned-but-not-implemented partial results.
- Full Disk Access has a real detection/check/settings flow, but retry/resume is still manual and limited.
- Installed Apps indexing exists, and Phase 9 adds a real uninstall review workflow (identity
  normalization, leftover-location matching, confidence scoring, a review dialog, Trash cleanup
  reusing the existing pipeline) — but orphan matching (Phase 10) is still missing.
- Uninstall review's `team_id` is always `None` from a real scan: reading it needs code-signing
  inspection (the `Security` framework or shelling out to `codesign`), and this phase adds neither
  a new dependency nor an external process for it. The scoring logic handles a supplied team id
  correctly (unit-tested with one) — only real extraction is missing.
- Group Container matching is a name heuristic (team-id prefix, vendor, app-specific text), not a
  real read of an app's `com.apple.security.application-groups` entitlement.
- No Docker/Xcode/Homebrew/Node provider integrations yet.
- Non-macOS support is intentionally unavailable and shown as such in UI.

These limitations are intentional: the implementation now has shared traversal, selection, Finder reveal, and Trash groundwork, but broader categories remain blocked until permission, more scanners, and deeper app-analysis phases are in place.
