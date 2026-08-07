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
- `src/cleaner/macos/scanners/xcode_junk.rs` provides Xcode/CoreSimulator developer-cache analysis
  (Phase 11).
- `src/cleaner/macos/scanners/homebrew_cache.rs` provides Homebrew download-cache analysis (Phase 11).
- `src/cleaner/macos/scanners/node_tooling_cache.rs` provides Node.js package-manager cache analysis
  across six providers under `src/cleaner/macos/scanners/node_tooling/` (Phase 11).
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

### Xcode Junk scanner

Eight fixed roots under `~/Library/Developer` and `~/Library/org.swift.swiftpm`, each tagged with a
risk/selection/capability set that never drifts from `macos::cleanup::policy_for`'s allow-list
(`xcode_junk::cleanup_allowed_roots` is the single list both read):

| Root | Risk | Selected by default | `MoveToTrash` |
|---|---|---|---|
| `Xcode/DerivedData` | `SafeRecreatable` | yes | yes |
| `Xcode/UserData/Previews` | `SafeRecreatable` | yes | yes |
| `CoreSimulator/Caches` | `SafeRecreatable` | yes | yes |
| `Xcode/Archives` | `ReviewRecommended` | no | no |
| `Xcode/iOS DeviceSupport` | `ReviewRecommended` | no | no |
| `CoreSimulator/Devices` | `ReviewRecommended` | no | no |
| `XCTestDevices` | `ReviewRecommended` | no | no |
| `org.swift.swiftpm` | `ReviewRecommended` | no | no |

Current behavior:

- aggregates every root by immediate child (`AggregateMode::ImmediateChildren`), so no item's path
  ever equals its own allow-listed root — an intentional choice: `macos::safety::validate_path`
  rejects deleting an allowed root outright, so an item whose path equals the root would silently
  fail cleanup;
- groups `DerivedData` entries by project, stripping a plausible trailing `-<hash>` from Xcode's
  `<ProjectName>-<hash>` naming convention (`derived_data_project_name`);
- calls a read-only `NSRunningApplication` check once per scan (`macos::platform::xcode::
  is_xcode_running`) and, when Xcode is running, attaches an `ItemWarning` to every `DerivedData`
  item plus one category-level `ScanWarning` — this warns rather than blocking the scan;
- never grants `ItemCapability::MoveToTrash` to Archives, iOS DeviceSupport, CoreSimulator Devices,
  XCTestDevices or the SwiftPM cache, matching the allow-list exactly — see
  `docs/cleaner/known-limitations.md` for why each of those stays scan-only this phase;
- CoreSimulator Devices and XCTestDevices are never added to the cleanup allow-list at all, even
  though the scanner can see them — the same "scan-only until a more deliberate workflow exists"
  posture Phase 9 used for system-scope leftover roots.

### Homebrew Cache scanner

Current behavior:

- resolves the cache root in two tiers only — the `HOMEBREW_CACHE` environment variable, then the
  default `~/Library/Caches/Homebrew` — never the Cellar and never `brew --cache` (see
  `docs/cleaner/known-limitations.md` for why the ticket's second tier was skipped);
  `homebrew_cache::resolve_cache_root` is the single function both this scanner and
  `macos::cleanup::policy_for` call, so cleanup can never allow-list a root the scan did not use;
- separates a `Cask/` subdirectory into its own "Cask cache" group and a `Logs/` subdirectory into
  its own "Logs" group when either is present, and folds everything else at the top level into one
  "Download cache" group — Homebrew does not otherwise separate formula bottles from source
  tarballs on disk;
- marks every item `SafeRecreatable`/`SelectedByDefault` with `MoveToTrash`, matching
  `UserCacheScanner`'s bar for a `Caches`-namespaced root;
- never scans the Cellar and never invokes `brew cleanup` or any other mutating Homebrew command.

### Node Tooling Cache scanner

One `NodeToolCacheProvider` trait (`src/cleaner/core/node_tool_provider.rs` — plain Rust, no macOS
API, no GPUI, the same split as `CleanerScanner` itself) implemented by six providers under
`src/cleaner/macos/scanners/node_tooling/`, all driven by one scanner,
`src/cleaner/macos/scanners/node_tooling_cache.rs`. Every provider reads a `NodeToolEnvironment`
snapshotted once per scan (`node_tooling_cache::snapshot_environment`) rather than calling
`std::env::var_os` itself, and returns nothing at all — not an empty group, not an error — when its
tool shows no sign of being installed.

| Provider | Detects | Selected by default | Allow-listed for cleanup |
|---|---|---|---|
| npm | `_cacache` (cache) and `_logs` (logs) under `npm_config_cache` or `~/.npm`, reported as two separate groups | yes (both) | yes (both) |
| Yarn Classic | `YARN_CACHE_FOLDER` or default `~/Library/Caches/Yarn` | yes | yes |
| Yarn Berry | global cache only, default `~/Library/Caches/Yarn/Berry` (no env override honored) | yes | yes |
| pnpm | store (`npm_config_store_dir`/`PNPM_HOME`/default `~/Library/pnpm/store`) and a separate metadata cache (`~/Library/Caches/pnpm`) | store: no; cache: yes | store: **never**; cache: yes |
| Bun | install cache (`BUN_INSTALL_CACHE_DIR`/`BUN_INSTALL`/default `~/.bun/install/cache`) | yes | yes |
| Nub | nothing — see below | n/a | n/a |

Current behavior:

- every `allow_cleanup: true` location is scanned with `AggregateMode::ImmediateChildren`, so — same
  as `homebrew_cache` and `xcode_junk` — no item's path ever equals its own allow-listed root;
- `node_tooling_cache::cleanup_allowed_roots` reruns the same six providers against the same
  environment shape and keeps only `allow_cleanup: true` locations, so `macos::cleanup::policy_for`
  can never allow-list a root the scan itself did not produce;
- two duplicate-counting guards run before anything is scanned: an exact-path duplicate across two
  providers is dropped (first provider wins), and an immediate-child entry that is also *another*
  provider's own location is skipped — the same technique `homebrew_cache` uses for its `Cask`/`Logs`
  subdirectories, generalized here because Yarn Berry's default global cache
  (`~/Library/Caches/Yarn/Berry`) nests inside Yarn Classic's own cache root
  (`~/Library/Caches/Yarn`);
- pnpm's store is reported with `RiskLevel::UserData`, never selected by default, and — unlike every
  other location this scanner produces — never allow-listed for cleanup at all: there is no code path
  in this phase that can move it to Trash;
- Nub's provider always returns an empty result. It checks a defensive `NUB_HOME` override
  (`node_tooling::nub::detect_home`) purely so a future session has a wired detection point, but
  never turns a detection into a reported location — there is no well-known, version-stable
  filesystem convention for Nub this phase can point at with confidence, unlike nvm/fnm/Volta. See
  `docs/cleaner/known-limitations.md`.

### AI Apps scanner

One `AiAppDefinition` registry (`src/cleaner/core/ai_app_provider.rs` — plain data, no macOS API, no
GPUI) listing two providers today (`src/cleaner/macos/scanners/ai_app_providers.rs`: Ollama, LM
Studio), driven by one scanner, `src/cleaner/macos/scanners/ai_apps.rs`. Unlike Node Tooling Cache,
almost every judgment call is centralized on `AiAppRole` — Logs, Temporary downloads, Cache, Models,
Application support, Chat history, the six sub-categories the ticket names explicitly — rather than
decided per provider, because the ticket states these rules once ("Models are user-managed assets and
never selected by default", "Chat history requires explicit opt-in") and means them to apply
identically to every current and future provider.

| Role | Risk | Selected by default | Allow-listed for cleanup |
|---|---|---|---|
| Logs | SafeRecreatable | yes | yes |
| Cache | SafeRecreatable | yes | yes |
| Temporary downloads | SafeRecreatable | no | no (no provider registers one this phase) |
| Application support | UserData | no | no |
| Models | UserData | never (`NeverBulkSelect`) | no |
| Chat history | UserData | never (`NeverBulkSelect`) | no (no provider registers one this phase) |

Roots registered today:

| App | Role | Path |
|---|---|---|
| Ollama | Models | `~/.ollama/models` |
| Ollama | Application support | `~/Library/Application Support/Ollama` |
| Ollama | Logs | `~/Library/Logs/Ollama` |
| Ollama | Cache | `~/Library/Caches/Ollama` |
| LM Studio | Models | `~/.cache/lm-studio/models` and `~/Library/Application Support/LM Studio/models` (both checked; either may be absent) |
| LM Studio | Application support | `~/Library/Application Support/LM Studio` |
| LM Studio | Logs | `~/Library/Application Support/LM Studio/logs` |
| LM Studio | Cache | `~/Library/Caches/LM Studio` |

Current behavior:

- a `Logs`/`Cache` root is scanned with `AggregateMode::ImmediateChildren` (each log file or cache
  entry is its own item, same reasoning as `xcode_junk`/`homebrew_cache`); every other role is scanned
  as one `AggregateMode::WholeRoot` item, safe here specifically because those roles never get
  `ItemCapability::MoveToTrash`, so there is no allow-listed root an item's path could collide with;
- `ai_apps::cleanup_allowed_roots` reruns the registry and keeps only `AiAppRole::allow_cleanup()`
  locations (Logs, Cache), so `macos::cleanup::policy_for` can never allow-list Models or Application
  support even if a future UI bug tried to offer it;
- for Ollama's `Models` root only, `ai_app_providers::collect_ollama_model_names` walks the manifest
  tree's directory and file *names* (never a manifest's JSON body or any model weight) to populate
  `AiAppMetadata::model_names` for display; LM Studio has no confidently-known equivalent convention
  this phase, so its `Models` items always carry an empty `model_names` — see
  `docs/cleaner/known-limitations.md`;
- `scan()` checks once per app whether it is currently running (`platform::is_any_bundle_running`, a
  read-only `NSRunningApplication` check against a list of *candidate* bundle identifiers, since
  neither app's exact macOS bundle identifier is confidently known here) and attaches a warning to
  every item that app produced plus one category-level warning — this warns, never blocks, the same
  posture `xcode_junk` established for Xcode.

### Docker Cache scanner

`src/cleaner/macos/scanners/docker_cache.rs` — dangling/unused images, stopped containers, and
unused volumes/networks, via the `docker` CLI (`Command::new("docker")`, argument vectors, no shell).
**Deliberately does not reuse `crate::docker::services::DockerEngine`** even though `src/docker/`
already resolves a daemon connection and lists all four resource types: dodo's "self-contained-module
invariant" (see `dodo-database-internals`, which dropped a "detect running database containers"
feature in every design round to avoid exactly this) forbids `src/cleaner/` from gaining a `use
crate::docker` edge. This scanner is therefore a second, much smaller, independent Docker client —
line-delimited JSON (`--format '{{json .}}'`) parsed with `serde_json::Value`, no `bollard`, no second
tokio runtime.

| Object | Detected via | Risk | Selected by default | Cleanup |
|---|---|---|---|---|
| Dangling images | `docker image ls`, `Repository`/`Tag` both `<none>` | SafeRecreatable | yes | `docker rmi` |
| Unused tagged images | `docker image ls`, no container's `Image` field matches | ReviewRecommended | no | `docker rmi` |
| Stopped containers | `docker ps -a`, `State` is `exited`/`dead` | ReviewRecommended | no | `docker rm` |
| Unused volumes | `docker volume ls`, no container's `Mounts` names it | UserData | never (`NeverBulkSelect`) | `docker volume rm` |
| Unused networks | `docker network ls`, not predefined, no container's `Networks` names it | ReviewRecommended | no | `docker network rm` |

Current behavior:

- daemon status is folded into `ScanCompleteness::Partial { reason: UnsupportedEnvironment }` with a
  `ScanWarning` the moment the *first* command (`docker ps -a`) fails — missing binary and unreachable
  daemon are not distinguished, both just mean "Docker unavailable" and the category returns empty
  rather than failing the whole Smart Care scan;
- "in use" for images/volumes/networks is derived from `docker ps -a`'s own `Image`/`Mounts`/
  `Networks` columns, not a second `inspect` call per container — cheap, and matches the ticket's
  "avoid duplicate directory-size calculations"-style conservatism applied to process calls instead;
- `ItemCapability::MoveToTrash` is never granted; every item instead gets
  `ItemCapability::RunExternalCleanup`, and `CleanableItem::path` is a synthetic `docker://<kind>/<id>`
  string — display and `CopyPath` only, never resolved against the filesystem;
- cleanup routes through `docker_cache::prune_items`, not `cleanup::cleanup_items` —
  `views::cleaner_view::run_cleanup` branches on whether every selected item's category is
  `DockerCache`. `prune_items` calls `docker rmi`/`rm`/`volume rm`/`network rm` with no `--force`, so
  the daemon itself refuses a still-referenced object — the ticket's "Check references before
  cleanup", enforced by the engine rather than re-derived from a possibly-stale scan;
- the confirmation dialog shows dedicated wording (`Str::CleanerDockerCleanupConfirmTitle`/
  `CleanerDockerCleanupConfirmMessage`) rather than the generic "moved to Trash" text, since nothing
  here ever touches the Trash;
- image sizes are parsed back from `docker image ls`'s human-formatted string (`parse_human_size`,
  decimal units) — approximate by construction, labeled as such; containers, volumes and networks
  report no size at all (`docker ps -a`/`volume ls`/`network ls` do not include one without `docker
  system df -v`, out of scope this phase).

See `docs/cleaner/known-limitations.md` for what this phase does not cover (build cache, engine disk
usage totals, Docker Desktop's own log files, image-usage matching precision).

### Unimplemented categories

- Categories without a real scanner are surfaced as partial “coming later” results by the state layer.
- This avoids fake success while keeping the Cleaner navigation and Smart Care workflow wired end to end.

Cleanup is available only for explicit allow-listed roots. Review-only categories, such as Trash Bins today, have no destructive path yet.
