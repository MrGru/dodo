# Architecture Constraints (Per-Commit Rules)

**Source:** `CLAUDE.md`
**Stage:** Phase 4 (Implementation)

**Cross-reference:** Mirrored (briefly) in `planning-artifacts/project-context.md`; this document
is the canonical place to read at commit time.

---

## The mandatory constraints

| # | Constraint | Detection |
|---|---|---|
| **AC-1** | `Cargo.lock` is the only pin on `gpui`/`gpui_platform`/`gpui-component`; `cargo update` is never run as a side effect of another task | `git diff Cargo.lock` should be empty unless the task *is* a dependency bump |
| **AC-2** | `cargo fmt --all` is clean | CI blocking job |
| **AC-3** | `cargo clippy --all-targets --locked -- -D warnings` is clean, with no new crate-level `#[allow]` | CI blocking job |
| **AC-4** | Every user-facing string routes through `Str` | The `view_code_draws_no_untranslated_literals` `cargo test` guard + the second i18n `cargo test` guard |
| **AC-5** | `src/database/` stays self-contained | `grep -rn '^use crate::' src/database/ \| grep -vE 'crate::(database,i18n,app_icon,paths)'` returns nothing |
| **AC-6** | No file read, network call, or database query happens on the UI thread | Anything touching disk/HTTP/Docker/DB runs on the background executor — check `services/file_picker.rs`, `services/http/upload.rs`, `docker/services/engine.rs`, `database/services/{postgres,sqlite}.rs`'s pattern is followed by any new I/O |
| **AC-7** | No `LIMIT` is ever injected into a user-written statement | `database/models/page.rs::PageBuffer` bounds at the sink only |
| **AC-8** | A modal is `window.open_dialog`, never a page-level scrim | `docker/views/detail.rs`'s module doc explains why a hand-rolled scrim doesn't block clicks |
| **AC-9** | Every request/response column in API Explorer carries `min_w_0` | `render_request_editor`/`render_response_viewer` |
| **AC-10** | A persisted file's versioning matches its existing pattern | `collections.json`: `#[serde(default)]` only. The other four: explicit `"version"`, refuse newer-than-known |
| **AC-11** | A stored secret (database password, request auth secret) is never hidden behind a false sense of security | No `keyring` dependency is introduced; the UI notice about plain-text storage is never removed or made conditional |
| **AC-12** | A script (API Explorer pre-request/post-response) cannot exceed its sandbox | 2 s deadline, 16 MiB heap, 256 KiB per-value cap, one fresh QuickJS runtime per run |
| **AC-13** | A stated "coming soon" placeholder isn't silently half-built | Building any piece of E3.8/E4.6/E5.7 updates the owning module's `mod.rs` doc and `sprint-status.yaml` in the same change |

## Repository consistency

- [ ] New `models/`-layer code in `api_explorer`/`docker`/`database` ships unit tests directly
      against it (no GPUI needed to test `models/`).
- [ ] `sprint-status.yaml` and each story's dossier agree on status.
- [ ] Story IDs stay stable; a newly discovered sub-task is appended, never inserted or renumbered.

## Security

- [ ] Path-escape checks on any new file-touching feature: a resolved path must be equal to or a
      descendant of what the feature is scoped to.
- [ ] No secret in a log line, an error message, or any persisted file other than the one that's
      documented to hold it.
- [ ] A destructive action (Docker Delete, any future Database Explorer edit) requires an explicit
      confirm step, never a silent one-click.

## Performance

- [ ] `PageBuffer`'s caps (rows/bytes/single-cell) are respected by any new query path.
- [ ] Docker's poll cadence stays at exactly 5 s, only while the section is active.

## Backward compatibility

- [ ] A persisted file written before this change still loads correctly.
- [ ] Default behavior for a user who never opens Settings is unchanged.

## Testing

- [ ] `cargo test` passes in full, not just the new/touched test module.
- [ ] No test assertion was weakened or deleted to make a failure disappear.

## UX

- [ ] For a UI-visible change, it was actually run per `dodo-build-validate`, not just
      type-checked.
- [ ] New affordances follow existing labeling/icon/keyboard-nav conventions.
