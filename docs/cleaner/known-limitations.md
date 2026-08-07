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
- Phase 11 adds two developer-cache scanners, Xcode Junk and Homebrew Cache. Both are scan+preview
  only, matching every other category's "review before Trash" flow, and both have deliberate scope
  cuts:
  - **Xcode Junk** never allow-lists `Archives`, `iOS DeviceSupport`, `CoreSimulator/Devices` or
    `XCTestDevices` for cleanup, even though the scanner reports them — they stay scan-only this
    phase, the same posture Phase 9 used for system-scope leftover roots. Deleting a
    `CoreSimulator/Devices` or `XCTestDevices` folder directly while CoreSimulator is active is
    unsafe; the correct removal path is `xcrun simctl delete`, which this phase does not shell out
    to (no external-process work was in scope here). Each such item's `explanation` says so.
  - Xcode Junk does not parse `device.plist`/runtime metadata for CoreSimulator Devices or
    XCTestDevices — items are named by their on-disk folder (a UUID for most simulator devices,
    already-descriptive version strings like `17.4 (21E219)` for iOS DeviceSupport). Reading
    `device.plist` for a friendlier device name would need a plist parse this phase does not add,
    since the item is scan-only either way.
  - Xcode Archives are grouped by their dated top-level folder
    (`Archives/<YYYY-MM-DD>/`), not per individual `.xcarchive` inside it — `AggregateMode::
    ImmediateChildren` aggregates one level, matching every other scanner's convention, rather than
    a bespoke two-level traversal for a category that is scan-only anyway.
  - `~/Library/org.swift.swiftpm` is treated uniformly as `ReviewRecommended`/not-selected,
    regardless of subfolder name. SwiftPM's on-disk cache layout has changed across Xcode versions
    (a "repositories" or "cache" subfolder is plausibly a safe-to-recreate clone cache in some
    versions), and this phase does not attempt to tell that apart from checked-out package sources
    well enough to default-select any of it.
  - `DerivedData`'s project grouping (stripping a trailing `-<hash>` from Xcode's
    `<ProjectName>-<hash>` naming convention) is a heuristic (`is_plausible_hash`: ASCII
    alphanumeric, at least six characters) rather than a match against Xcode's exact hash alphabet.
    A false-positive strip only shortens a group label; it never changes which path gets scanned or
    cleaned.
  - "Warn when Xcode is running" is a single read-only `NSRunningApplication` bundle-identifier
    check (`macos::platform::xcode::is_xcode_running`), not a check for which specific project
    Xcode has open — a running Xcode warns on every `DerivedData` item, not just the one it is
    actively building.
  - **Homebrew Cache** never invokes `brew --cache` (the ticket's second detection tier, "safe
    Homebrew configuration output"). Only the `HOMEBREW_CACHE` environment variable and the default
    `~/Library/Caches/Homebrew` location are used. A *safe* invocation needs a bounded timeout,
    which `std::process::Command` has no built-in support for and this phase does not add — the
    ticket itself permits skipping it ("avoid unnecessary process calls"). A non-default install
    that neither exports `HOMEBREW_CACHE` nor uses the default cache location is invisible to this
    scanner.
  - Homebrew Cache does not separate formula-cache entries from Cask-cache entries beyond a literal
    `Cask/` subdirectory check — Homebrew does not otherwise separate the two on disk, and telling
    them apart further would need `brew list`/`brew info`, out of scope here.
  - Homebrew Cache has no dedicated "stale downloads" sub-classification. Every item still carries
    `modified_at` and the result list sorts by size, so staleness is visible without a second,
    separately-defined age rule layered on top of Large & Old Files' one-year threshold.
  - Homebrew Cache's main-root scan and its `Cask/`/`Logs/` sub-scans overlap: the top-level
    `ImmediateChildren` aggregation already recurses into `Cask/` and `Logs/` to size them (before
    they are excluded from the "Download cache" group), and then each is scanned again on its own
    to produce its own group. This duplicates some directory traversal rather than adding an
    exclusion-list concept to the shared `scan_root` engine for one scanner's need.
- Phase 11 also adds **Node Tooling Cache**, six providers (npm, Yarn Classic, Yarn Berry, pnpm,
  Bun, Nub) behind one `NodeToolCacheProvider` trait (`src/cleaner/core/node_tool_provider.rs`),
  driven by `src/cleaner/macos/scanners/node_tooling_cache.rs`. Scan+preview only, same as every
  other category. Deliberate scope cuts:
  - **No CLI process call was made anywhere in this phase.** Every location is derived from an
    environment-variable override or a documented default filesystem path — `npm`, `yarn`, `pnpm`
    and `bun` are never invoked, and pnpm's own `pnpm config get store-dir` is deliberately not
    shelled out to either, the same "avoid an extra process call" reasoning `homebrew_cache` used
    for skipping `brew --cache`. A non-default install that neither sets the relevant environment
    variable nor uses the documented default location is invisible to the matching provider.
  - **pnpm's store is scan-only and is never allow-listed for cleanup at all** — not "review
    required", but structurally excluded from `node_tooling_cache::cleanup_allowed_roots`, so no
    code path in this phase can move it to Trash regardless of what a future UI bug might let a
    user select. The store is shared across every pnpm project on the machine; the ticket asks for
    a future explicit "pnpm store prune" action with its own preview, which this phase does not
    add. pnpm's separate, smaller registry-metadata cache (`~/Library/Caches/pnpm`) is unaffected
    and is treated like any other tool's cache.
  - **Yarn Berry's project-local `.yarn/cache` and Plug'n'Play files (`.pnp.cjs`/`.pnp.data.json`)
    are out of scope for discovery.** Both require knowing where a Yarn Berry project lives on
    disk, and there is no fixed, home-relative convention for either — finding one would mean
    crawling the home directory for arbitrary project checkouts, which the ticket rules out for
    normal cleanup. Only Yarn Berry's own global cache is discovered. Because project-local pieces
    are never surfaced at all, the ticket's "must be shown separately and not selected by default"
    requirement for them is satisfied by construction.
  - **Bun's project-local dependencies are out of scope for discovery**, for the same reason as
    above, and **`node_modules` is never touched by any of the six providers**, project-local or
    otherwise — the shared "never delete `node_modules` automatically" rule applies regardless of
    scope. Bun's provider also does not fabricate a separate "installation cache", logs directory,
    or temporary-data directory distinct from its one confidently-documented install cache
    (`<BUN_INSTALL>/install/cache`) — none of the three has a version-stable location this phase
    can point at with the same confidence.
  - **Yarn Classic has no separate logs location.** Unlike npm's `_logs`, there is no documented,
    stable Yarn Classic log directory distinct from its cache; only the cache is reported.
  - **Nub's provider always returns an empty result — no location, ever.** Unlike nvm, fnm or
    Volta, this phase has no well-known, version-stable on-disk convention for a tool named "Nub"
    to check with confidence, and the ticket explicitly permits falling back to scan-only/reporting
    nothing when uncertain rather than inventing a directory layout. Reported provisioned Node
    versions must never be selected by default regardless — moot here, since none are ever
    reported. `node_tooling::nub::detect_home` checks a defensive `NUB_HOME`-style override purely
    so a future session with real knowledge of Nub's layout has a wired place to extend; it is
    never turned into a location today, confirmed or not.
  - Yarn Berry's default global cache path (`~/Library/Caches/Yarn/Berry`) is a documented
    convention, the same "best-effort default location" status `homebrew_cache`'s
    `~/Library/Caches/Homebrew` has — neither is derived by calling the tool itself. Because that
    path nests inside Yarn Classic's own cache root (`~/Library/Caches/Yarn`), the scanner excludes
    Yarn Berry's directory from Yarn Classic's own immediate-children enumeration (and, more
    generally, excludes any immediate child that is also another provider's own location) rather
    than double-reporting the same cache under two provider names.
- **AI Apps (Phase 12)** — Ollama and LM Studio via one registry (`core::ai_app_provider`), driven by
  `macos::scanners::ai_apps`:
  - **Neither app's exact macOS bundle identifier is confidently known.** Both `OLLAMA_BUNDLE_IDS`
    and `LM_STUDIO_BUNDLE_IDS` (`macos::scanners::ai_app_providers`) list more than one candidate; a
    wrong guess only means the "app is running" warning never fires for that app — nothing in this
    phase depends on the check succeeding for correctness, only for a nicer warning.
  - **LM Studio's exact model directory is not confidently known**, so both plausible candidates
    (`~/.cache/lm-studio/models` and `~/Library/Application Support/LM Studio/models`) are checked;
    whichever does not exist on a given machine is simply skipped as a missing root, the same way
    every other scanner here treats an absent optional root.
  - **Neither app registers a `TemporaryDownloads` or `ChatHistory` root.** There is no
    version-stable, confidently-known on-disk convention for either sub-category for either app —
    the same "report nothing rather than guess at a layout" posture Node Tooling's Nub provider
    took. The `AiAppRole` variants and their `NeverBulkSelect`/scan-only enforcement exist and are
    unit-tested with a synthetic provider, so a future session that does know a real path only has
    to register it.
  - **Model name extraction is Ollama-only.** `ai_app_providers::collect_ollama_model_names` reads
    Ollama's manifest tree structure (directory and file *names* only, never a manifest's JSON body
    or model weights) to populate `AiAppMetadata::model_names`. LM Studio has no equivalent this
    phase; its `Models` items always carry an empty `model_names`.
  - **No CLI process call.** Neither `ollama` nor any LM Studio CLI is invoked — filesystem-convention
    detection only, the same reasoning Node Tooling Cache and Homebrew Cache used.
  - Models, Application support and Chat history are scan-only this phase (never allow-listed for
    cleanup, regardless of what a future UI bug might let a user select) — only Logs and Cache can
    ever be moved to Trash through Cleaner.
- Non-macOS support is intentionally unavailable and shown as such in UI.

These limitations are intentional: the implementation now has shared traversal, selection, Finder reveal, and Trash groundwork, but broader categories remain blocked until permission, more scanners, and deeper app-analysis phases are in place.
