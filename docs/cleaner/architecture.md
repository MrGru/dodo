# Cleaner architecture (Phase 0/1)

Cleaner is implemented as a normal feature module inside the existing `dodo` crate:

- `src/cleaner/core/`: domain contracts and typed models, no GPUI dependency.
- `src/cleaner/state/`: scan orchestration and UI state transitions.
- `src/cleaner/views/`: GPUI rendering only.
- `src/cleaner/macos/`: macOS-only implementation boundary (`#[cfg(target_os = "macos")]`).

## Sidebar and routing integration

Cleaner is wired as a top-level tool in:

- `src/main.rs` module registration
- `src/layout.rs` `View` enum, `View::ALL`, title/icon mapping, entity creation, pane render match
- `src/app_icon.rs` icon registration
- `src/i18n.rs` localized titles and labels

## Data flow

1. UI triggers scan (`CleanerView::start_scan`).
2. `CleanerState` moves `Idle -> Scanning`.
3. Scanners run on background executor via `CleanerScanner` trait.
4. Progress events stream through channel -> state updates.
5. Category results accumulate into shared summary fields.
6. Completion transitions to `Completed` / `PartiallyCompleted` / `CompletedWithFailures`.

## Concurrency strategy (phase 1)

- Background work runs off UI thread.
- Progress is batched via polling channel pump (no per-file notify flood).
- Cancellation uses shared `CancellationToken` checked by scanner loop.
- No one-task-per-file fan-out.

## Platform strategy

- macOS-first behavior is real.
- Non-macOS shows explicit unsupported UI text.
- No fake Windows/Linux scanners are registered.
- Future Windows/Linux support can be added as sibling modules under `src/cleaner/`.
