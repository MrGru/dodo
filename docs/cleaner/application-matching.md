# Application matching (Phase 9, extended in Phase 10)

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
  This includes the Phase 10 installed-app index (`installed_apps::installed_app_identities`) —
  every identity it builds also has `team_id: None`, for the same reason.
- Entitlements (e.g. an app's actual `com.apple.security.application-groups` value) are not parsed.
  Group Container matching is a name-based heuristic (team-id prefix, vendor, app-specific text),
  not a real entitlement read.
- Confidence scoring does not yet distinguish a validated `High` match from an unvalidated one, so
  `High` never defaults to selected even though the ticket allows it for "carefully validated"
  cases.

## Orphan detection (Phase 10)

`applications/orphans.rs` asks the inverse of Phase 9's question. `locations::find_leftovers`
answers "what does *this one app* own?" for a single [`AppIdentity`]; `orphans::find_orphans`
(wrapping the injectable `find_orphans_from`) walks the exact same fixed location list and asks,
for every entry found there, "does *any* installed app's identity explain this?" — using the whole
installed-app index at once. An entry no identity explains becomes an `OrphanCandidate`.

- **The installed-app index** (`scanners::installed_apps::installed_app_identities`) is built from
  the same fixed root list and `Info.plist` parsing the `InstalledApps` scanner uses — one list, so
  the scanner and the orphan detector never disagree about what "installed" means. It is a plain
  `Vec<AppIdentity>`: a dedicated index type would have been a thin wrapper over that, so there
  isn't one.
- **Ownership check.** An entry is "owned" — and therefore not an orphan — when
  `locations::classify_name_match` (identifier-suffixed and generically-scanned locations) or
  `locations::classify_group_container` (Group Containers) returns `Some(_)` for *any* identity in
  the index, including a weak vendor-only match. Only an entry no identity explains at all becomes
  an `OrphanCandidate`. There is no "ambiguous with another app" penalty here — that penalty exists
  to downgrade a match *for one known app* when a second app could also explain it, and an orphan
  is by definition explained by no app, so the situation never arises.
- **`OrphanReason`**, exactly as the ticket suggests it: `BundleIdentifierNotInstalled` (Containers),
  `StaleSavedState` (Saved Application State), `StalePreference` (Preferences),
  `MissingOwnerApplication` (LaunchAgents/LaunchDaemons), `UnknownContainerOwner` (Group
  Containers), `AppNameNotInstalled` (every generically-named location — Application Support,
  Caches, Logs, WebKit, HTTPStorages, Cookies, Services, Autosave Information).
- **Confidence reuses Phase 9's scheme rather than inventing a second one.** Every location keeps
  the same base `NameMatchKind` Phase 9 assigned it when checking a *known* identity's ownership —
  `ExactSandboxContainer` for Containers, `ExactSavedStateIdentifier` for Saved Application State,
  `ExactPreferenceIdentifier` for Preferences and LaunchAgents/LaunchDaemons — run through the same
  `total_score`/`classify` pipeline (including the protected-system-path penalty, which is why
  every system-scope orphan candidate is `SharedOrUnsafe` regardless of location). Group Containers
  is always scored via the negative `SharedContainer` kind, and every generically-named location
  uses the conservative `PartialAppNameMatch` (20 points, `Low`) — there is no *matched* signal to
  grade the strength of, since every candidate that reaches these locations failed to match
  anything at all.
- **Apple's own namespace is never flagged.** Any entry whose name starts with `com.apple.`
  (case-insensitively) is skipped before it is ever scored, in every scope — see
  `docs/cleaner/known-limitations.md` for why.
- **The "keep" list** (`core::ignore`, `services::ignore_store`) is dodo's eighth persisted file,
  `cleaner-ignored-items.json`. It follows the `script-consent.json`/`quick-nav.json` discipline —
  an explicit `version` refused if higher than this build understands — and keys each kept item by
  its absolute path string rather than its `CleanableItemId`, since that id is a session-local hash
  with no promise of surviving a restart. `scanners::orphaned_files::OrphanedFilesScanner::scan`
  loads this list itself (self-contained, like every other macOS scanner reading its own inputs)
  and filters candidates before they ever become `CleanableItem`s, so a kept path does not reappear
  on rescan.
- **CLI tools without `.app` bundles are not detected.** See
  `docs/cleaner/known-limitations.md` for why this is a deliberate scope cut rather than an
  oversight.
