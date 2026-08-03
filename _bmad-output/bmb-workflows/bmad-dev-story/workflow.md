---
name: bmad-dev-story
description: "Per-story execution loop for dodo — implements one story from _bmad-output/implementation-artifacts/stories/, following agent-execution-loop.md step by step."
type: workflow
date: 2026-08-03
stage: "Phase 4 (Implementation)"
---

# bmad-dev-story — Per-Story Execution Loop

> This file previously described a Python-scripted, two-developer ("Dev A"/"Dev B") automation
> pipeline for an unrelated project (`scripts/bmad/next_story.py`, a 13-item PySide6-specific grep
> gate, a Cursor slash command, `pytest`). None of that tooling exists in dodo, and this repo has
> no automated story selector or commit linter installed. This version describes the loop that
> actually applies: a single agent working through `agent-execution-loop.md`.

**Full procedure**: `_bmad-output/implementation-artifacts/agent-execution-loop.md` — this file is
a short pointer into it, not a second copy of the steps.

---

## 1. Pick a story

dodo has no selector script. Read
`_bmad-output/implementation-artifacts/AGENT_BRIEF.md`'s flat index and
`_bmad-output/implementation-artifacts/sprint-status.yaml`; pick any story whose `deps` (if any)
are all `done`. As of this writing, the four open stories (E3.8, E4.6, E5.7, E9.4) are mutually
independent — pick whichever is wanted next.

## 2. Load the dossier

`_bmad-output/implementation-artifacts/stories/<id>.md` — its AC list and "Where it lives" section
are the scope for this iteration. Load whichever skill `project-context.md`'s trigger table points
to before writing code.

## 3. Code, within the story's stated scope

Edit only what the story's "Where it lives" section names, plus whatever new files its ACs require.
Follow the module's existing five-layer split if touching `api_explorer`/`docker`/`database`.

## 4. Gate before committing

```bash
cargo fmt --all
cargo clippy --all-targets --locked -- -D warnings
cargo test
# only if src/database/ was touched:
grep -rn '^use crate::' src/database/ | grep -vE 'crate::(database,i18n,app_icon,paths)'
```

All must be clean/empty. If a UI-visible change, also actually run it per `dodo-build-validate`.

## 5. Commit and update status

Commit following this repo's existing message style (see `git log` — no fixed template like the
predecessor's `AC:` tag convention is enforced here). Update the story's row in
`sprint-status.yaml` to `done`.

## If blocked

See `blocked-decision-tree.md` — it covers dodo's actual failure modes (a `gpui-component` API
question, an unrelated test failure, a `clippy`/`fmt` failure, the self-containment grep failing,
ambiguity about how much of a placeholder to build, Windows/Linux release-path uncertainty), not
the predecessor's `FCI_API_KEY`/PyInstaller-specific list.
