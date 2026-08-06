# Cleaner scanners (Phase 0/1)

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

## Current scanner set

Phase 1 uses mock scanners only (`src/cleaner/state/mock.rs`) to validate:

- incremental updates,
- cancellation semantics,
- category result wiring.

No real filesystem traversal or deletion is performed yet.
