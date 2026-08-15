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

- `crates/dodo-cleaner/src/macos/scanners/user_cache.rs` is a real scanner.
- `crates/dodo-cleaner/src/macos/scanners/system_junk.rs` is a second real scanner for safe recreatable junk roots.
- `crates/dodo-cleaner/src/macos/scanners/large_old_files.rs` provides analysis for user folders with non-default selection.
- `crates/dodo-cleaner/src/macos/scanners/mail_files.rs` provides Full-Disk-Access-gated Mail attachment/download analysis.
- `crates/dodo-cleaner/src/macos/scanners/trash_bins.rs` analyzes Trash bins as review-only items.
- `crates/dodo-cleaner/src/macos/scanners/installed_apps.rs` provides first-pass installed-app indexing.
- `crates/dodo-cleaner/src/macos/scanners/orphaned_files.rs` provides Full-Disk-Access-gated orphan detection
  (Phase 10), reusing Phase 9's identity/location/confidence machinery in reverse.
- `crates/dodo-cleaner/src/macos/scanners/xcode_junk.rs` provides Xcode/CoreSimulator developer-cache analysis
  (Phase 11).
- `crates/dodo-cleaner/src/macos/scanners/homebrew_cache.rs` provides Homebrew download-cache analysis (Phase 11).
- `crates/dodo-cleaner/src/node_tooling_cache.rs` provides shared Node.js package-manager cache analysis across
  providers under `crates/dodo-cleaner/src/node_tooling/` on macOS, Windows and Linux.
- `crates/dodo-cleaner/src/state/mock.rs` remains for unit tests that validate the orchestration layer in isolation.

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
- delegates `Info.plist` parsing to `crates/dodo-cleaner/src/macos/applications/bundle.rs`, shared with the
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

`crates/dodo-cleaner/src/node_tooling_cache.rs` and `crates/dodo-cleaner/src/node_tooling/` are shared by macOS, Windows and
Linux. The `NodeToolCacheProvider` trait and `NodeCacheLocation` were already platform-neutral; the
old `NodeToolEnvironment` and three providers were not, because their fallbacks assumed
`~/Library`. The environment now carries resolved host cache directories plus successful tool-query
answers, while providers remain plain Rust with no platform API or GPUI dependency.

Discovery precedence is an explicit environment override, then one fixed-argv tool query, then a
known host fallback where one is safe:

| Provider | Discovery |
|---|---|
| npm | `npm_config_cache`, `npm config get cache`, then `%LOCALAPPDATA%\\npm-cache` (Windows) or `~/.npm` (macOS/Linux); `_cacache` and `_logs` remain separate groups |
| Yarn Classic | `YARN_CACHE_FOLDER`, `yarn cache dir`, then `%LOCALAPPDATA%\\Yarn\\Cache`, `$XDG_CACHE_HOME/yarn`, or `~/Library/Caches/Yarn` |
| Yarn Berry | `yarn config get globalFolder` plus its `cache` child; the shipped `~/Library/Caches/Yarn/Berry` fallback remains macOS-only |
| pnpm | `pnpm config get cache-dir`, then the host cache convention (`pnpm-cache` on Windows, `pnpm` on macOS/Linux) |
| Bun | `BUN_INSTALL_CACHE_DIR`, `bun pm cache`, then `<BUN_INSTALL or ~/.bun>/install/cache` |
| Nub | no location; its layout remains uncertain |

Command output must be one absolute, existing path and is never evaluated by a shell. Exact and
nested roots use host-aware comparison; an item containing another provider root is omitted rather
than counted or deleted twice. Paths containing `node_modules` are excluded.

pnpm's `npm_config_store_dir`/`pnpm store path` answer is a **denied root**, not a result: the
content-addressable store is neither shown nor allow-listed, and cleanup adds it to the existing
deletion policy's protected paths. `PNPM_HOME` is not consulted because it names pnpm's executable
directory, not the store.

### AI Apps scanner

One shared scanner (`crates/dodo-cleaner/src/ai_apps.rs`) consumes resolved Ollama and LM Studio definitions
from `crates/dodo-cleaner/src/ai_apps/definitions/{macos,windows,linux}.rs`. The old core policy seam was useful,
but its static `~` paths, bundle identifiers and direct macOS activity call were not actually
cross-platform; those are now resolved inputs and a thin injected activity probe. Judgment remains
centralized on `AiAppRole` — Logs, Temporary downloads, Cache, Models, Application support and Chat
history — because models/history are user data on every host.

| Role | Risk | Selected by default | Allow-listed for cleanup |
|---|---|---|---|
| Logs | SafeRecreatable | yes | yes |
| Cache | SafeRecreatable | yes | yes |
| Temporary downloads | SafeRecreatable | no | no (no provider registers one this phase) |
| Application support | UserData | no | no |
| Models | UserData | never (`NeverBulkSelect`) | no |
| Chat history | UserData | never (`NeverBulkSelect`) | no (no provider registers one this phase) |

Roots registered today:

| Host | App | Scan-only data | Cleanable generated data |
|---|---|---|---|
| macOS | Ollama | `~/.ollama/models`, Application Support | `~/Library/Logs/Ollama`, `~/Library/Caches/Ollama` |
| macOS | LM Studio | two model candidates plus Application Support | exact `logs` and cache directories |
| Windows *(inferred)* | Ollama | `$OLLAMA_MODELS` or `%USERPROFILE%\\.ollama\\models` | exact `%LOCALAPPDATA%\\Ollama\\{server.log,app.log}` files |
| Windows *(inferred)* | LM Studio | `%USERPROFILE%\\.lmstudio\\models` | exact `Cache`, `Code Cache`, `GPUCache`, `logs` under `%APPDATA%\\LM Studio` |
| Linux *(inferred)* | Ollama | `$OLLAMA_MODELS` or `~/.ollama/models` | none; journal output is not fabricated as a path |
| Linux *(inferred)* | LM Studio | `~/.lmstudio/models`, `$XDG_CACHE_HOME/lm-studio/models` | exact Electron cache/log children under XDG config/cache roots |

Every Windows/Linux location carries `AiAppPathEvidence::Inferred`; the platform definition modules
are intentionally the one correction point after captain validation. No whole AppData/XDG root is
cleanable.

Current behavior:

- `ExactFile`, `DirectoryContents` and `DirectorySummary` preserve exact log-file targets, generated
  directory children and one scan-only user-data summary respectively;
- `ai_apps::cleanup_allowed_roots` keeps only Logs/Cache and refuses every `DirectorySummary`, so all
  three platform cleanup policies use the existing deletion boundary without authorizing Models,
  Application Support or Chat History;
- for Ollama's `Models` root only, `ai_apps::collect_ollama_model_names` walks the manifest
  tree's directory and file *names* (never a manifest's JSON body or any model weight) to populate
  `AiAppMetadata::model_names` for display; LM Studio has no confidently-known equivalent convention
  this phase, so its `Models` items always carry an empty `model_names` — see
  `docs/cleaner/known-limitations.md`;
- `scan()` checks activity once per app through an injected probe: `NSRunningApplication` on macOS,
  ToolHelp on Windows, `/proc/*/comm` on Linux. Running or unknown activity suppresses default
  selection and attaches warnings; unknown is never treated as not running. No shell is invoked.

### Docker Cache scanner

`crates/dodo-cleaner/src/docker_cache.rs` — a scanner shared by macOS, Windows and Linux for dangling/unused
images, stopped containers, and unused volumes/networks, via the `docker` CLI (fixed argument
vectors, no shell).
**Deliberately does not reuse `crate::docker::services::DockerEngine`** even though `src/docker/`
already resolves a daemon connection and lists all four resource types: dodo's "self-contained-module
invariant" (see `dodo-database-internals`, which dropped a "detect running database containers"
feature in every design round to avoid exactly this) forbids `crates/dodo-cleaner/src/` from gaining a `use
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

- a missing CLI or non-zero exit from any of the four list commands is folded into
  `ScanCompleteness::Partial { reason: UnsupportedEnvironment }` with a `ScanWarning`; the category
  returns empty rather than showing objects from an uncertain partial inventory;
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

### Universal Binaries scanner (analysis-only)

`crates/dodo-cleaner/src/macos/scanners/universal_binaries.rs` — discovers which installed app's *main*
executable (`Contents/MacOS/<CFBundleExecutable>`) is a universal (fat) Mach-O binary, via the
`object` crate (`read`, `macho` features only — no `write`, since this phase never mutates a binary).
**Analysis only**, per the ticket: no removal exists yet, and none is planned until Phase 16 lands a
tested backup/rollback/signature-recheck path.

Walks the same four standard app roots `InstalledAppsScanner` uses and reuses
`applications::bundle::parse_bundle` for `Info.plist`. Only the app's one main executable is
inspected — nested frameworks, plugins and helper tools are not walked (see
`docs/cleaner/known-limitations.md`). A bundle whose executable reports fewer than two architecture
slices is not reported at all — this category exists only for genuinely universal binaries.

Current behavior:

- `object::FileKind::parse` identifies fat vs. thin Mach-O from the file's first bytes;
  `MachOFatFile32`/`MachOFatFile64` then give each slice's exact `Architecture` and byte size —
  nothing here runs `lipo`, `file`, or the binary itself;
- `estimated_removable_bytes` (`UniversalBinaryMetadata`) sums every slice that is not this machine's
  own architecture (`std::env::consts::ARCH`, mapped to Mach-O naming: `aarch64` → `arm64`);
- signing status comes from `codesign --verify --no-strict`'s exit code alone (`Option<bool>`; `None`
  only when `codesign` itself could not run) — a coarse verified-or-not signal, not identity or
  entitlement inspection;
- a system app (`applications::bundle::is_system_app_path`) gets `RiskLevel::Protected` and an
  `ItemWarning` explaining Cleaner never mutates one; every other universal binary gets
  `RiskLevel::ApplicationMutation` and `SelectionPolicy::NeverBulkSelect` — the strongest selection
  guard in Cleaner, since even after Phase 16 ships, thinning a binary should never be something a
  bulk "Select safe items" click can reach;
- items carry only `RevealInFinder`/`CopyPath` — no `RemoveArchitecture` capability yet, since nothing
  reads it;
- a running app (checked the same way `xcode_junk`/`ai_apps` do, via a bundle-identifier match against
  `NSRunningApplication`) gets an `ItemWarning`, not a skipped scan.

### Language Files scanner (analysis-only)

`crates/dodo-cleaner/src/macos/scanners/language_files.rs` — one item per `<App>.app/Contents/Resources/*.lproj`
localization folder, grouped by app (`CleanableItem::group`). **Analysis only**, the same posture as
Universal Binaries: no removal exists yet, and `SelectionPolicy::NeverBulkSelect` on every item —
protected or not — reflects that there is nothing to select *for*.

A `.lproj` is protected (`RiskLevel::Protected`, an `ItemWarning` naming the reason,
`LanguageMetadata::protection_reason`) rather than omitted, so "show languages per app" stays true even
for the ones the ticket says must never be removed:

| Reason | Detected via |
|---|---|
| `BaseLocalization` | The folder is literally `Base.lproj` |
| `DevelopmentRegion` | Primary subtag matches the bundle's own `CFBundleDevelopmentRegion` |
| `PreferredLanguage` | Primary subtag matches an entry in `AppleLanguages` |
| `EnglishFallback` | Primary subtag is `en`, unconditionally |

Everything else gets `RiskLevel::ApplicationMutation`, the same tier Universal Binaries uses.

Current behavior:

- `AppleLanguages` — this machine's ordered preferred-language list — is read directly from
  `~/Library/Preferences/.GlobalPreferences.plist` with the `plist` crate (no new dependency, no Cocoa
  `NSLocale` call); a missing or malformed file falls back to an empty list, which the unconditional
  English-fallback rule still covers;
- `applications::bundle::parse_bundle` gained a `development_region` field
  (`CFBundleDevelopmentRegion`) for this scanner — the one addition to Phase 9's shared bundle parser;
- primary-subtag matching (`"zh-Hans"` vs. `"zh-Hant-TW"` → match on `"zh"`) is a simple split-on-hyphen
  comparison, not a full BCP-47 parse — enough for every case this phase needs;
- a system app's languages are `RiskLevel::Protected` with their own warning, same as
  `universal_binaries`; a running app gets an `ItemWarning`, not a skipped scan.

### Unimplemented categories

- Categories without a real scanner are surfaced as partial “coming later” results by the state layer.
- This avoids fake success while keeping the Cleaner navigation and Smart Care workflow wired end to end.

Cleanup is available only for explicit allow-listed roots. Review-only categories, such as Trash Bins today, have no destructive path yet.
