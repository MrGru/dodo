# `dodo-cleaner`

Read `src/lib.rs`'s doc comment first — it is the authority on the crate's shape, its boundaries,
the two seams the 2026-08-15 move out of the binary added, and the three decisions settled on
2026-08-13 (`IconRaster`, `hidden_for`, `ScanState::indicator`). `docs/cleaner/` holds the scanner
inventory, the safety model, the privacy posture, application matching and the known limitations.
This file holds what neither of those can: the rules that span the crate and the regressions that
cost frames.

**This crate was `src/cleaner/` until 2026-08-15.** Any path anywhere that still says so is stale.

## `#[allow(dead_code)]` in `core/` is pending work, not dead code

Items ahead of what constructs them are annotated, never deleted, and **the allow comes off as each
producer lands** — that is the condition, and it is written next to each one. `core::permissions`
is the one remaining whole-module allow, marking an area that does not exist at all yet.

Dead-code warnings in a module under construction are scaffolding. Do not "clean them up".

## The deletion boundary is real, and it is deny-by-default

`core::safety` is the boundary all three cleanup paths go through. Host-aware lexical normalization
plus canonical allowed-root containment rejects traversal, symlink/junction escapes, filesystem
roots, declared roots and the user's home. Its `DeletionPolicy` **authorizes nothing until a scanner
root is named** — an empty policy is a policy that permits no deletion, not one that permits
everything. `docs/cleaner/safety-model.md` is the companion.

## What the window lists is `hidden_for(HostOs)`, and it is per platform and pure

`core::category::CleanerCategory::hidden_for` is the whole switch, and because a scan starts only
from a category's own pane, a hidden category is never scanned. Taking a `HostOs` rather than
splitting on `cfg` is what lets every platform's answer be asserted from a Mac.

- **macOS** lists all fourteen.
- **Windows and Linux** list the four filesystem categories plus shared AI Apps, Docker Cache and
  Node Tooling Cache, and both list Installed Apps.
- **Windows** never reads registry uninstall strings and hands actions to Installed Apps settings.
- **Linux** treats desktop entries as user-facing evidence over dpkg (Debian/Ubuntu), RPM (Fedora),
  pacman (Arch), separately-scoped Flatpak, Snap and bounded AppImages. Native packages, system
  Flatpaks and Snap are **scan-only**; only user Flatpaks and bounded AppImages have actions.
- **Neither platform deletes package-managed install locations.**
- **Language Files stays macOS-only** unless a safe equivalent appears. **Orphaned Files stays
  unavailable on Windows** and may return on Linux only with conservative, package-manager-aware
  ownership.

AI Apps keeps each host's Ollama/LM Studio paths in `src/ai_apps/definitions/<host>.rs`. Every
Windows and Linux location there is explicitly **inferred until captain validation**, and models,
chats and settings remain scan-only user data.

**Scanner registries own what can scan, and paired tests forbid disagreement in either direction**:
no hidden scanner, and no listed row without one.

## `results_sync.rs` is the pattern to copy, and it is a frame-rate fix

The root `AGENTS.md` states the rule — a `render` that copies a whole collection pays that copy on
every frame, because an ancestor re-rendering sets `Window::refreshing` and bypasses the element
cache for every descendant, however well the rows themselves are virtualized. This crate is where
that cost was measured and paid: `src/views/results_sync.rs` carries the fix, the measurements and
the decision table. **Stamp a revision where the data is mutated and compare it before re-copying.**

## The prepaint-notify regression, and why the table has fixed columns

Never use a prepaint callback to mutate and `notify` a view for the next frame.
`WindowInvalidator::invalidate_view` schedules a redraw only in `DrawPhase::None`, so a notify
during layout or prepaint records the state without dirtying the window and then waits for an
unrelated event. That is what made the Cleaner's result rows appear only after some resize
sequences.

The table therefore keeps the pre-`2dd735c` **fixed-column** path and accepts horizontal scrolling;
`src/views/results_table.rs` records why feeding prepaint measurements back into the view is unsafe,
and `cleaner_view.rs` carries the headless layout-state regression test.

The lesson underneath it is worth more than the fix: **a headless `simulate_next_frame` is not
evidence that the platform scheduled a frame.** It runs an already-queued callback by hand, which is
exactly why the failed workaround's test passed while the captain's idle window stayed blank.
