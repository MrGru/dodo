# Cleaner safety model (Phase 0/1)

## Core principles

- Scanning is discovery-only: scanners never delete.
- Cleanup is a separate action path (not implemented in phase 1).
- Unsupported platforms must be explicit.
- No destructive fallback behavior exists.

## Selection defaults

Domain types model explicit risk and selection intent:

- `RiskLevel`
- `SelectionPolicy`
- `ItemCapability`

Phase 1 mock results use recreatable-safe defaults only.

## Deletion policy shape (prepared, not executed yet)

Core types exist for future enforcement:

- `DeletionPolicy`
- `AllowedRoot`
- `SafetyError`

These are the boundary for allow-list based path validation before any future Trash move operation.
