# Cleaner permissions (Phase 0/1)

Phase 1 introduces typed permission contracts without claiming implementation completeness.

## Typed model

- `MacPermission::FullDiskAccess`
- `PermissionState`
- `PermissionRequirement`
- `PermissionService` trait

## Current behavior

- No fake permission success is reported.
- No TCC bypass/escalation is attempted.
- Non-macOS remains explicitly unsupported in the Cleaner UI.

## Next phase target

Implement real Full Disk Access detection and UX flow in `src/cleaner/macos/permissions/`, including:

- read-based access checks on protected locations,
- System Settings deep-link/open flow,
- retry/recheck flow for pending category scans.
