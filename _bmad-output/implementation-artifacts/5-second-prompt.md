# 5-Second Prompt — One-Liner for the Implementing Agent

**Stage:** Phase 4 (Implementation)

---

If an agent needs the absolute minimum to start work on dodo, this is it:

```
You are implementing a story for dodo, a Rust/GPUI desktop app (single window, collapsible
sidebar, several self-contained developer tools).

Read in this order:
1. CLAUDE.md at the repo root — the actual authority on how the code works.
2. _bmad-output/implementation-artifacts/AGENT_BRIEF.md — flat story index.
3. _bmad-output/implementation-artifacts/sprint-status.yaml — current state.
4. _bmad-output/implementation-artifacts/stories/<story-id>.md — the story you're working on.
5. Whichever skill project-context.md's trigger table points to for this story.

Per-commit rules (non-negotiable, see architecture-constraints.md):
  cargo fmt --all
  cargo clippy --all-targets --locked -- -D warnings
  cargo test
  Never run `cargo update` as a side effect of this task.
  Every user-facing string goes through `Str`.
  No file/network/DB I/O on the UI thread.

If touching src/database/: grep -rn '^use crate::' src/database/ | grep -vE 'crate::(database,i18n,app_icon,paths)'
  must return nothing after your change.

If blocked, see blocked-decision-tree.md. If the story is a stated placeholder (OAuth2 auth type,
Docker's Exec/Create/Stats/Favorites, or the Database Explorer's editing/CRUD/autocomplete set),
build only what its owning module's mod.rs doc says was deferred — nothing broader.
```

Unlike the predecessor of this document, there is no slash-command automation (`/dodo-run-story`
etc.) installed in this repo, no multi-developer standup cadence, and no submission deadline — the
prompt above is complete as written.
