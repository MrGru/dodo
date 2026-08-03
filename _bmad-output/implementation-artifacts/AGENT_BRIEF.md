# Agent Execution Brief — THE ONE FILE TO READ FIRST (after `CLAUDE.md`)

**Source:** `CLAUDE.md`, `planning-artifacts/epics/E*.md`
**Stage:** Phase 4 (Implementation)

> Read `../../CLAUDE.md` in full first — it is the actual authority. This brief is the flat,
> quick-scan index into what this folder adds on top of it.

---

## What this folder is for

dodo is not a from-scratch build racing a deadline; E0-E9 are already substantially shipped. This
folder tracks the same epics/stories shape BMAD expects, but most rows in
`sprint-status.yaml` are already `done` — an agent's real job here is almost always one of:

1. Implement one of the few forward stories (a placeholder becoming real, per
   `planning-artifacts/product-brief.md` §1.6).
2. Fix a defect in already-shipped behavior.
3. Extend an existing tool with something genuinely new (a new epic, with its own ADR).

## Flat story index

| Epic | Stories | Status |
|---|---|---|
| E0 — Core shell & app lifecycle | E0.1-E0.3 | All done |
| E1 — JSON Formatter | E1.1-E1.2 | All done |
| E2 — Encoder / Decoder | E2.1-E2.2 | All done |
| E3 — API Explorer | E3.1-E3.7 done; **E3.8 (OAuth2) planned** | 7/8 done |
| E4 — Docker / Podman module | E4.1-E4.5 done; **E4.6 (Exec/Create/Stats/Favorites) deliberately not built** | 5/6 done |
| E5 — Database Explorer | E5.1-E5.6 done; **E5.7 (editing/CRUD/autocomplete/etc.) deliberately not built** | 6/7 done |
| E6 — In-app Updater | E6.1-E6.3 | All done |
| E7 — Theming, Settings & i18n | E7.1-E7.2 | All done |
| E8 — Persistence & `data_dir()` | E8.1-E8.2 | All done |
| E9 — Build, Release & Licensing engineering | E9.1-E9.3 done; **E9.4 (GPL-3.0 distribution) open question, not a defect** | 3/4 done |

Full per-story detail: `implementation-artifacts/stories/e*.md`. Current state:
`implementation-artifacts/sprint-status.yaml`.

## Which skill to load per story

See `planning-artifacts/project-context.md`'s trigger table — do not guess a `gpui-component` API
or an i18n step; load the matching skill first.
