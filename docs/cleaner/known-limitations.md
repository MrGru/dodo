# Known limitations (after the first real scanner)

- Real filesystem scans now cover User Cache, System Junk safe roots, Large & Old Files, Mail Files, Trash Bins analysis, and basic Installed Apps indexing.
- Cleanup execution still covers only explicit allow-listed roots.
- Smart Care still reports most categories as planned-but-not-implemented partial results.
- Full Disk Access has a real detection/check/settings flow, but retry/resume is still manual and limited.
- Installed Apps indexing exists, and Phase 9 adds a real uninstall review workflow (identity
  normalization, leftover-location matching, confidence scoring, a review dialog, Trash cleanup
  reusing the existing pipeline). Phase 10 adds orphan detection (the inverse question: which
  leftovers does *no* installed app explain) on top of the same identity/location/confidence
  machinery — see `docs/cleaner/application-matching.md`.
- Orphan detection is explicitly not perfectly accurate, and the ticket permits that. Specific,
  deliberate gaps:
  - CLI tools with no `.app` bundle (Homebrew formulae, language toolchains, anything a package
    manager put in `/usr/local`, `/opt/homebrew` or a dotfile) are not detected at all — there is
    no bundle identifier and no fixed leftover-location convention to reverse-match against, and
    the ticket hedges this requirement with "where possible". Left undone rather than guessed at.
  - Any entry whose name starts with `com.apple.` (case-insensitively) is never flagged as an
    orphan, in every scope. dodo's installed-app index only ever comes from `/Applications`,
    `~/Applications` and `/System/Applications`, so it has no way to tell a leftover Apple daemon
    from a live one; without this filter, a system-scope scan would flag most of `/Library`'s own
    daemons and caches as "orphaned" — hundreds of unconfirmable false positives.
  - `~/Library/Group Containers` entries are always scored as the most conservative confidence
    bucket (`SharedOrUnsafe`, reason `UnknownContainerOwner`), never `Confirmed` or selected by
    default: attributing an unclaimed group container to one specific missing app is never
    reliable (see the entitlements limitation below).
  - The installed-app index used for orphan matching is built from the same fixed root list
    Phase 9 uses (`/Applications`, `~/Applications`, `/System/Applications`,
    `/System/Applications/Utilities`). An app installed anywhere else is invisible to the index,
    so its leftovers can be misidentified as orphaned.
  - Only `MatchConfidence::Confirmed`, non-system-scope candidates default-select — the same bar
    Phase 9 set for leftover matches, applied identically here. Every other bucket, including
    `High`, requires the user to select it explicitly.
- Uninstall review's `team_id` is always `None` from a real scan: reading it needs code-signing
  inspection (the `Security` framework or shelling out to `codesign`), and this phase adds neither
  a new dependency nor an external process for it. The scoring logic handles a supplied team id
  correctly (unit-tested with one) — only real extraction is missing.
- Group Container matching is a name heuristic (team-id prefix, vendor, app-specific text), not a
  real read of an app's `com.apple.security.application-groups` entitlement.
- No Docker/Xcode/Homebrew/Node provider integrations yet.
- Non-macOS support is intentionally unavailable and shown as such in UI.

These limitations are intentional: the implementation now has shared traversal, selection, Finder reveal, and Trash groundwork, but broader categories remain blocked until permission, more scanners, and deeper app-analysis phases are in place.
