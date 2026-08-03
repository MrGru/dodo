# Project Context — Rules for the Dev Agent

**Source:** `CLAUDE.md` (the authority — read it in full before touching code)
**Stage:** Phase 3 (Solutioning)

Every implementing agent must read `../../../CLAUDE.md` first. This file is a compressed pointer
into it, not a replacement.

---

## Hard constraints (do not relax without recording why)

- **`Cargo.lock` is the only pin** on `gpui`, `gpui_platform`, `gpui-component`. Never run
  `cargo update` as a side effect of another task.
- **`fmt` and `clippy` are blocking.** `cargo fmt --all` and
  `cargo clippy --all-targets --locked -- -D warnings` must be clean; no crate-level `#[allow]`.
- **Every user-facing string goes through `Str`.** Load the `dodo-i18n-text` skill before writing
  or changing any label, title, placeholder, description, error, or dropdown option.
- **`src/database/` is self-contained.** `grep -rn '^use crate::' src/database/ | grep -vE
  'crate::(database,i18n,app_icon,paths)'` must return nothing.
- **No file read or network/database call on the UI thread.** Anything that touches disk, an HTTP
  API, the Docker Engine API, or a database runs on the background executor.
- **A stated cut is not a bug.** Docker's Exec/Terminal/Create/Pull/Build/Stats/Favorites, the
  Database Explorer's editing/CRUD/autocomplete/second-backend/column-sorting set, and API
  Explorer's OAuth2 auth type are each a literal disabled control with a reason recorded in the
  owning module's `mod.rs` or component doc — do not "fix" them without a new epic and ADR.
- **A modal is `window.open_dialog`, never a hand-rolled scrim.** See `docker/views/detail.rs`'s
  module doc for why a `div` scrim and a no-op mouse handler don't actually block clicks in GPUI.
- **Widget questions go to the pinned `gpui-component` source**, at
  `~/.cargo/git/checkouts/gpui-component-*/<rev>/crates/ui/src` (rev from `Cargo.lock`), and to the
  `gpui-component-recipes` skill — not to guessing an API.

## Which skill to load, and when

| Trigger | Skill |
|---|---|
| Writing/editing any `render`/`new` building a gpui-component widget | `gpui-component-recipes` |
| Adding, renaming, reordering, or removing a sidebar tool | `dodo-tool-view` |
| Writing or changing any text a user reads | `dodo-i18n-text` |
| Adding/changing a setting, theme, or language | `dodo-theming-settings` |
| First `cargo` invocation of a session, or verifying a UI change actually works | `dodo-build-validate` |

## Glossary and further reading

- `glossary.md` — domain terms.
- `ARCHITECTURE-SPINE.md` — component map.
- `architecture-decisions.md` — the ADRs behind each non-obvious choice.
- `epics/E*.md` — per-epic story tables.
- `../implementation-artifacts/architecture-constraints.md` — the per-commit checklist.

## One-page summary

**What** dodo — a single-window Rust/GPUI desktop app bundling several small developer tools
(JSON Formatter, Encoder/Decoder, API Explorer, Docker, Database Explorer) behind one sidebar, plus
Settings and an in-app updater.

**Why this shape** GPUI + gpui-component give a native, no-web-view UI with a ready widget set;
both are git-only dependencies pinned solely by `Cargo.lock`. Each tool is self-contained so adding
or changing one never touches another.

**What's built** E0-E9 are all substantially shipped (see `sprint-status.yaml`) — this is not a
plan racing toward a first release, it is a live, working app with a handful of clearly labeled,
deliberately unbuilt controls.

**What's explicitly not built** See `product-brief.md` §1.6 — each cut is a disabled UI control or
a stated absence with a reason, not a silent gap.

**Owner** No named team is recorded anywhere in the repository; treat every change as reviewed
against `CLAUDE.md` and this folder, not against an org chart that doesn't exist here.

**Now what** Read `CLAUDE.md`, then `implementation-artifacts/AGENT_BRIEF.md`, then
`implementation-artifacts/sprint-status.yaml` to see what's actually open, then a story under
`implementation-artifacts/stories/`.
