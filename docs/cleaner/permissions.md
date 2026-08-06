# Cleaner permissions (current groundwork)

## Typed model

- `MacPermission::FullDiskAccess`
- `PermissionState`
- `PermissionRequirement`
- `PermissionService` trait

## Current behavior

- macOS now has a concrete permission service under `src/cleaner/macos/permissions/`.
- Full Disk Access is checked by attempting real access to protected roots such as:
  - `~/Library/Mail`
  - `~/Library/Safari`
  - `~/Library/Containers`
- Cleaner shows a permission banner for categories that are expected to need Full Disk Access.
- Mail Files is the first real category gated by that permission state.
- The banner can:
  - recheck access,
  - open the macOS Full Disk Access settings page,
  - reveal the Dodo application bundle for easier manual approval.
- No fake permission success is reported.
- No TCC bypass/escalation is attempted.
- Non-macOS remains explicitly unsupported in the Cleaner UI.

## Still missing

- retrying a blocked real scanner immediately after approval,
- differentiation between "granted but restart required" and ordinary denial.
