# Completion Criteria — When a Story Is "Done"

**Stage:** Phase 4 (Implementation)

---

dodo has no single "the plan is done" moment — see `PRD.md` §2.3 for the per-story Definition of
Done, reproduced here with the checklist an agent actually runs:

## Code-side completion

- [ ] Every AC bullet in the story's dossier is met.
- [ ] `cargo fmt --all` is clean.
- [ ] `cargo clippy --all-targets --locked -- -D warnings` is clean, with no new crate-level
      `#[allow]`.
- [ ] `cargo test` passes, including both i18n guard tests if any user-facing text changed.
- [ ] If the story touches `src/database/`: the self-contained-module grep still returns nothing.
- [ ] No file read, network call, or database query was added on the UI thread.
- [ ] If a persisted file's shape changed: the correct versioning pattern was used (added-field
      tolerance only for `collections.json`; an explicit `"version"` + refuse-on-newer for the
      other four).

## Documentation-side completion

- [ ] If the story adds a new tool, setting, or behavior CLAUDE.md doesn't yet describe: CLAUDE.md
      itself was updated (it is the authority; this folder is a derived summary of it).
- [ ] If the story fills in a stated placeholder (E3.8/E4.6/E5.7): the owning module's `mod.rs` doc
      comment was updated to say so, and this folder's `sprint-status.yaml`/epic file were updated
      to match.

## Manual verification (UI-visible stories only)

- [ ] The feature was actually run — per `dodo-build-validate` — not just type-checked.
- [ ] Any theming/i18n implication was checked against `dodo-theming-settings` if a new setting was
      involved.

## What "done" explicitly does not require

- A release tag, a rehearsed demo, or a submission package — dodo has none of the hackathon
  deliverables the predecessor of this document described. A story is done when the checklist
  above is green; shipping happens through the normal release process in `docs/release.md`.
