# Cleaner architecture

Cleaner is implemented as a normal feature module inside the existing `dodo` crate — no new crate,
package or workspace member. As of Phase 17, all 14 scanner categories the ticket names exist (Phases
9-15 landed after the Phase 1-8 groundwork below); Phase 16 (real architecture/language removal) is
deliberately not yet implemented — see "What's deferred" below.

- `src/cleaner/core/`: domain contracts, scan-root definitions, filesystem aggregation, and safety
  helpers with **no GPUI dependency** — unit-testable without a window, and this is checked by hand at
  every phase rather than by an automated grep the way `src/database/`'s self-contained-module
  invariant is.
- `src/cleaner/state/`: scan orchestration and UI state transitions (`CleanerState`, `CleanerStatus`).
- `src/cleaner/views/`: GPUI rendering only — no filesystem traversal happens here.
- `src/cleaner/{macos,windows,linux}/`: target implementation boundaries for filesystem roots,
  native Trash/Finder calls, permissions and app identity. CLI/path-policy scanners that genuinely
  share behavior live directly under `src/cleaner/`.
- `src/cleaner/services/`: the one persisted store this module owns (`cleaner-ignored-items.json`),
  behind a trait, following the same convention as dodo's other seven persisted files.

## Sidebar and routing integration

Cleaner is wired as a top-level tool in:

- `src/main.rs` module registration
- `src/layout.rs` `View` enum, `View::ALL`, title/icon mapping, entity creation, pane render match
- `src/app_icon.rs` icon registration
- `src/i18n.rs` localized titles and labels (800+ `Str` variants across the whole project; Cleaner's
  own strings are a few hundred of those)

## Data flow

1. UI triggers a scan from [`CleanerView`](../../src/cleaner/views/cleaner_view.rs).
2. [`CleanerState`](../../src/cleaner/state/cleaner_state.rs) moves `Idle -> Scanning`.
3. Smart Care scans every category; other sections scan only the selected category.
4. Registered scanners run on the background executor via
   [`CleanerScanner`](../../src/cleaner/core/scanner.rs) — never the UI thread.
5. Progress reaches the UI through a per-category capacity-one, latest-wins slot
   (`core::progress::LatestProgress`) that one shared 120 ms pump takes from — at most one update
   applied per category per tick, never one `cx.notify()` per file. A scan's result, cancellation
   and error never travel that way; they are the background task's return value.
6. Category results accumulate into summary fields and per-category result panels.
7. On a supported host, a category with no registered scanner is hidden. The explicit partial
   "coming later" result remains a defensive fallback rather than a roadmap row.
8. Completion transitions to `Completed`, `PartiallyCompleted`, or `CompletedWithFailures`.
9. "Clean selected" opens a confirmation dialog, then runs either the host's Trash-move pipeline
   or, for `CleanerCategory::DockerCache` only, the shared Docker-CLI prune pipeline
   (`cleaner::docker_cache::prune_items`) — `CleanerView::run_cleanup` branches on category before
   any target-specific filesystem cleanup, never mixing the two.

## Concurrency strategy

- Background work runs off the UI thread via `cx.background_executor()`.
- Progress is throttled in the filesystem engine (`core::fs::ProgressReporter`, ~8/sec) and coalesced
  again at the UI boundary — no per-file `cx.notify()` flood, and no queue that can grow behind a
  busy UI thread. Only the newest update per category survives; an intermediate one is dropped on
  purpose.
- Cancellation uses a shared `CancellationToken`, checked before each root, before descending into a
  directory, and before each external process call.
- No one-task-per-file fan-out anywhere.
- Smart Care runs its categories **sequentially** within one background task today (see
  `CleanerView::start_scan`), not with bounded concurrent category scans as the ticket's suggested
  `SmartCarePlan::max_concurrent_categories` implies — a deliberate simplification, not an oversight;
  see "What's deferred".

## Core domain (no GPUI)

- `core::fs` — bounded traversal and aggregation (`scan_root`, `AggregateMode`), hard-link–aware
  size accounting (`hard_link_identity`), mount-boundary detection (`same_filesystem`).
- `core::scan_root`, `core::scan_context`, `core::cancellation`, `core::progress` — the shared
  scan-time types every scanner passes around.
- `core::safety` — the allow-list `DeletionPolicy`, `validate_path` (component-based containment,
  symlink rejection, root-deletion rejection, protected-path rejection — always re-checked at
  cleanup time, never trusting a stale scan result), `dedupe_nested_paths`.
- `core::item`, `core::category`, `core::risk`, `core::report`, `core::errors` — the shared
  `CleanableItem`/`CategoryScanResult`/typed-error vocabulary every scanner and the view speak.
- `core::ai_app_provider`, `core::node_tool_provider` — the two provider-registry abstractions
  (Phase 11, Phase 12) that let a third Node tool or AI app be added as one data value, never a
  scanner-file change.
- `core::ignore` — the pure data shape (`IgnoredItemsDocument`) behind the "Keep" persisted list.

## Platform scanners, one file (or small module) per category

| Category | File(s) |
|---|---|
| System Junk | `system_junk.rs` |
| User Cache | `user_cache.rs` |
| Mail Files | `mail_files.rs` |
| Trash Bins | `trash_bins.rs` |
| Large & Old Files | `large_old_files.rs` |
| Installed Apps | `installed_apps.rs` |
| Orphaned Files | `orphaned_files.rs` |
| AI Apps (shared) | `ai_apps.rs`, `ai_apps/definitions/{macos,windows,linux}.rs` |
| Xcode Junk | `xcode_junk.rs` |
| Homebrew Cache | `homebrew_cache.rs` |
| Node Tooling Cache (shared) | `node_tooling_cache.rs`, `node_tooling/{npm,yarn_classic,yarn_berry,pnpm,bun,nub}.rs` |
| Universal Binaries (analysis-only) | `universal_binaries.rs` |
| Language Files (analysis-only) | `language_files.rs` |

The table above includes the macOS-specific set. Docker Cache, Node Tooling Cache and AI Apps live
directly under `src/cleaner/` because all three are shared by every host. Node and AI Apps receive
resolved platform locations; AI process activity stays a thin target probe. Each platform's
`scanners/mod.rs` registers its set; `state::registry` picks the compiled target's registry.

## Shared macOS infrastructure

- `macos/applications/` (Phase 9, extended in Phase 10) — app identity normalization
  (`identity.rs`), the confidence-scoring model (`confidence.rs`), the fixed leftover-location list
  (`locations.rs`), `Info.plist` parsing (`bundle.rs`), the uninstall-review workflow (`review.rs`)
  and orphan detection (`orphans.rs`). A sibling of `scanners/`, not a member of it, because two
  different scanners and one dialog all depend on it. See `docs/cleaner/application-matching.md`.
- `macos/cleanup.rs` — the single Trash-move pipeline (`cleanup_items`) and the allow-list
  (`policy_for`) every category's cleanup goes through, built fresh per cleanup call so it can never
  drift from what a scan actually produced.
- `macos/permissions/` — Full Disk Access detection, TCC registration, System Settings deep link.
- `macos/platform/` — native Finder reveal, native Trash move (`objc2`), the shared
  `is_any_bundle_running` running-process check, Xcode's own thin wrapper over it.
- `core::ignore` + `services::ignore_store` — the "Keep" list's persistence (Phase 10).

## What's deferred (deliberately, not by oversight)

- **Phase 16 (real architecture/language removal) does not exist.** Universal Binaries and Language
  Files are analysis-only; `ItemCapability::RemoveArchitecture`/`RemoveLocalization` are declared but
  never granted. The ticket gates this phase on a tested backup/rollback/signature-recheck path this
  session did not build, and mutating real installed application binaries is a materially higher-risk
  change than anything shipped so far.
- **Smart Care scans categories sequentially, not with the ticket's suggested bounded concurrent
  fan-out.** `SmartCarePlan`/`SmartCareResult` (declared in `core::report`) are not yet wired into a
  concurrent scheduler; Smart Care today is "scan every category, one after another, on one
  background task." Correct and safe, just not the throughput the ticket's suggested design implies.
  A future pass can parallelize per-category scans behind the same `CleanerScanner` trait without
  changing any scanner's own code.
- **No tracing spans.** dodo has no `tracing`/`log` crate dependency anywhere in the codebase today
  (verified: `grep -n '^tracing\|^log = ' Cargo.toml` returns nothing) — adding one solely for
  Cleaner's benefit would be a new cross-cutting dependency with no existing subscriber to consume it
  usefully, and the ticket's own benchmarking asks (worker-count sweeps, sequential-vs-parallel
  comparisons) need a real-hardware run to mean anything regardless.
- **Result-table virtualization has since landed** — this section's earlier "not done" entry is
  stale. `src/cleaner/views/results_table.rs` is a `TableDelegate` driving
  `gpui_component::table::DataTable`, so only rows inside the scroll viewport are built each frame;
  its module doc is the authority. Virtualizing the rows was not on its own enough to make a large
  result cheap to *display*: the view also handed the delegate a fresh deep clone of the entire
  result every frame, which `src/cleaner/views/results_sync.rs` now does only when the result or
  the selection actually changed. Read that module before adding any other per-`render` copy.

  **The grid uses fixed columns and horizontal scrolling.** A self-sizing version briefly measured
  the pane from a zero-ink `canvas` at prepaint and fed that width back into `CleanerView`. GPUI
  does not schedule a redraw for `notify` during a draw phase, so the new width waited for an
  unrelated event and result rows could remain absent while idle. `results_table.rs` therefore
  keeps the original stable column path; the actions column is simply wide enough for all four
  supported buttons.
- **No "export scan report to a local file"** action yet, though it's in the ticket's required
  interactions list.

## Platform strategy

- macOS is the only platform that **lists** all 14 categories. Windows and Linux list their
  four filesystem scanners plus shared AI Apps, Docker Cache and Node Tooling Cache; every
  unsupported row is hidden.
  `CleanerCategory::hidden_for(HostOs)` is the whole switch and is a pure function of the platform,
  so all answers are unit tested from any host. See `docs/cleaner/advanced-tools.md`.
- `state::registry::default_scanners()` picks the platform's set and returns an empty vector only
  on a target that is none of the three. Paired invariant tests forbid both a hidden scanner and a
  listed row without a scanner.
- A wholly unsupported target still shows the explicit unsupported UI text
  (`CleanerView::supported_platform`).
- Nothing about the core domain, state machine or view layer assumes macOS; each platform module
  implements the same `CleanerScanner` trait.
