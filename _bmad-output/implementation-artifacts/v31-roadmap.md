# Roadmap — What's Genuinely Next

**Stage:** Phase 4 (Implementation)

> Kept under this filename (`v31-roadmap.md`) per `bmad.config.yaml`'s existing manifest key
> (`roadmap`); the "v3.1" framing itself was specific to the predecessor project's competition
> version numbering and does not apply to dodo.

---

## What's next, in no particular required order

- **E3.8** — API Explorer's OAuth2 auth type (`AuthType::OAuth2`, currently `later_step()`).
- **E4.6** — Docker's Exec/Terminal, Create/Pull/Build, deeper Stats, and Favorites, each
  independently.
- **E5.7** — Database Explorer's object detail/DDL tabs, editing/CRUD, favorites, pinned queries,
  persisted history/tab restore, autocomplete, global search, a second backend (MySQL/Redis), and
  column sorting.
- **E9.4** — Resolving the GPL-3.0-or-later distribution question one way or another.

## What's explicitly not on this roadmap

Per `product-brief.md` §1.6, none of the following are planned, deferred, or "eventually": a
process-level `run_command`-style sandbox (dodo has no such tool to begin with — this is inherited
language from the predecessor project and does not apply), a workspace/project concept, cloud sync,
telemetry, or accounts. Adding any of these would be a new product decision with its own product
brief, not an item quietly picked off a roadmap.

## Revisiting a "Won't" decision

If a stated cut (Docker's placeholders, the Database Explorer's cuts, OAuth2) needs to be
reconsidered at a larger scope than what its owning module's `mod.rs` describes, that's a new ADR
and a new epic — not an expansion of an existing forward story.
