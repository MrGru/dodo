# Agent Execution Loop — Per-Story Cycle

**Stage:** Phase 4 (Implementation)

---

For each story an agent picks up (a forward story from `AGENT_BRIEF.md`, or a defect fix against an
already-`done` story):

```
1. LOAD the story's dossier: implementation-artifacts/stories/e<epic>.<n>-*.md
   └─► Confirms: full AC list, which files it touches, which skill applies.

2. CHECK sprint-status.yaml: confirm the story's `deps` (if any) are all `done`.
   └─► dodo's forward stories (E3.8, E4.6, E5.7, E9.4) have no unresolved deps —
       any one can be picked up in any order.

3. LOAD the relevant skill (see project-context.md's trigger table) before writing
   any code — do not guess a gpui-component API or an i18n step.

4. WRITE THE CODE.
   - Follow the module's existing five-layer split if touching api_explorer/docker/database
     (models/ → services/ → state/ → components/ → views/).
   - No file read or network/DB call on the UI thread.
   - Every user-facing string through `Str`.

5. WRITE OR UPDATE TESTS.
   - Unit tests for anything in `models/`.
   - If the story touches user-facing text, run the two i18n `cargo test` guards.

6. RUN THE PER-COMMIT CHECKS:
   cargo fmt --all
   cargo clippy --all-targets --locked -- -D warnings
   cargo test
   (if touching src/database/) grep -rn '^use crate::' src/database/ | grep -vE 'crate::(database,i18n,app_icon,paths)'

7. If the story is UI-visible, actually run it per `dodo-build-validate` before
   calling it done — type-checking is not the same as verifying the feature works.

8. COMMIT with the story ID in the message, following this repo's existing commit style
   (see `git log` — short, imperative, `type(scope): summary`).

9. UPDATE sprint-status.yaml: mark the story `done`.

LOOP BACK to step 1 for the next story.
```

Unlike the predecessor of this document (which modeled a two-developer, 11-day sprint with a
hand-off/review step between parallel streams), dodo has no fixed cadence and no named second
reviewer recorded anywhere in the repository — each story is a self-contained unit of work checked
against `cargo fmt`/`clippy`/`test` and the module's own architectural rules, not against a sprint
calendar.
