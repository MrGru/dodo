# Application matching (Phase 9)

Identity normalization, leftover-location matching, confidence scoring and the uninstall review
workflow now live under `src/cleaner/macos/applications/`, a sibling of `scanners/` rather than a
member of it — the code here is shared by the `InstalledApps` scanner (`bundle::parse_bundle`) and
the uninstall review workflow, and none of it is itself a `CleanerScanner`.

- `applications/bundle.rs`: `Info.plist` parsing, moved out of `scanners/installed_apps.rs` so both
  the scanner and the review workflow call the same code.
- `applications/identity.rs`: pure normalization. `AppIdentity::new` derives, from a bundle
  identifier and display name: the final bundle-ID component, the bundle ID with a trailing
  `.helper`/`.Helper`/`-helper` suffix removed, a lowercased vendor guess from the reverse-DNS
  bundle ID, and a normalized app name (lowercased, punctuation collapsed, a trailing
  version-looking token dropped). No filesystem access.
- `applications/confidence.rs`: `MatchConfidence` (`Confirmed`/`High`/`Medium`/`Low`/
  `SharedOrUnsafe`) and `NameMatchKind`, one variant per named row in the ticket's point table.
  `total_score` adds the two situational penalties (ambiguous with another installed app, a
  protected system path) on top of a signal's base points; `classify` buckets the total.
- `applications/locations.rs`: the fixed leftover-location list and `find_leftovers`, the only
  impure function in the module — it lists directories and matches entry names against an
  `AppIdentity`, never reads file contents.
- `applications/review.rs`: `build_uninstall_review` ties the above together into an
  `UninstallReview` (the app item plus every leftover `UninstallCandidate`), refusing
  `RiskLevel::Protected` apps outright.

## Confidence scoring

`NameMatchKind`'s base points mirror the ticket's table exactly (`ExactBundleIdentifier` /
`ExactSandboxContainer` = 100, down to `VendorOnlyMatch` = 10, `SharedContainer` = -80,
`KnownSharedVendorDirectory` = -70). Two situational penalties apply on top of any signal:
"another installed app also matches" (-70) and "protected system path" (-100) — the latter makes
every system-scope candidate `SharedOrUnsafe` regardless of how exact the name match was.
Thresholds: `Confirmed` >= 100, `High` >= 80, `Medium` >= 45, `Low` > 0, else `SharedOrUnsafe`.

Only `Confirmed` matches default-select (`SelectionPolicy::SelectedByDefault`). The ticket's
"carefully validated high-confidence matches" carve-out for `High` is deliberately not
implemented — there is no concrete validation rule yet to tell a trustworthy `High` apart from an
untrustworthy one, so every `High`/`Medium`/`Low` candidate stays `NotSelectedByDefault`, and every
`SharedOrUnsafe` candidate is `NeverBulkSelect`.

## Leftover locations

`find_leftovers` covers every location the ticket names:

- Identifier-exact lookups (existence checks, not directory scans): `~/Library/Containers/<bundle
  id>`, `~/Library/Saved Application State/<bundle id>.savedState`,
  `~/Library/Preferences/<bundle id>.plist`, `~/Library/LaunchAgents/<bundle id>.plist`.
- A dedicated Group Containers matcher (`classify_group_container`) that requires **both** an
  app-specific signal (the final bundle component, or the normalized name) **and** an ownership
  signal (a team-id prefix, or the vendor) before calling a match app-specific; a directory that
  only carries the ownership signal is exactly the ticket's "shared across a vendor's apps" case.
- A generic directory-name scan (`classify_name_match`) for Application Support, Caches, Logs,
  WebKit, HTTPStorages, Cookies, Services and Autosave Information, plus the system-scope roots.
  Application Support's exact-name match is upgraded to `ExactApplicationSupportDirectory`; the
  rest use `ExactNormalizedAppName`.
- System-scope roots (`/Library/Application Support`, `/Library/Caches`, `/Library/Preferences`,
  `/Library/LaunchAgents`, `/Library/LaunchDaemons`, `/Library/PrivilegedHelperTools`) are scanned
  for transparency — the review dialog shows what was found there — but are never added to
  `cleanup.rs`'s `DeletionPolicy::allowed_roots`, so they stay scan-only even if a UI bug ever
  let one through with a checkbox: `validate_path` rejects them with `OutsideAllowedRoot`.

A small `KnownAppRule` registry (`KNOWN_APP_RULES`, empty by default) exists for an app whose
leftover naming the generic heuristics would miss; adding a rule is a data change, not new
matching code (`known_app_rule_is_data_driven` tests exactly that).

## Uninstall review workflow

"Begin uninstall review" appears on an Installed Apps row only when the scanned item carries
`ItemCapability::UninstallApplication` — the scanner (`scanners/installed_apps.rs`) omits that
capability entirely for `/System/Applications` bundles, so a system app has no button to click.
`build_uninstall_review` refuses a `RiskLevel::Protected` item defensively as well
(`UninstallReviewError::ProtectedApplication`), in case that ever changes.

The dialog (`views/uninstall_review_dialog.rs`) opens immediately in a loading state and updates
itself once the background analysis (`build_uninstall_review`, including a best-effort size
measurement via the shared `core::fs::scan_root`) finishes — no code path needs a `Window` handle
after the initial `open_dialog` call. It shows, per the ticket's confirmation-flow list: the app,
every related file with its confidence badge, shared/system-scope paths (labelled and
uncheckable), and an estimated total size. Shared and uncertain candidates start unchecked; the
app bundle itself has no checkbox and is always included. Confirming calls
`CleanerView::start_uninstall_cleanup`, which reuses the exact same `cleanup::cleanup_items` /
`CleanupReport` pipeline as ordinary category cleanup — there is no second Trash pathway.

## Known limitations

- `team_id` is always `None` from a real scan. Reading it would need code-signing inspection (the
  `Security` framework or shelling out to `codesign`), and this phase adds neither a new dependency
  nor an external process for it. The scoring and matching logic still handles a `Some` team id
  correctly and is unit-tested with one; only *populating* the field from a real scan is missing.
- Entitlements (e.g. an app's actual `com.apple.security.application-groups` value) are not parsed.
  Group Container matching is a name-based heuristic (team-id prefix, vendor, app-specific text),
  not a real entitlement read.
- Confidence scoring does not yet distinguish a validated `High` match from an unvalidated one, so
  `High` never defaults to selected even though the ticket allows it for "carefully validated"
  cases.
