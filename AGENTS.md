# Project agent memory

`dodo` is a Rust desktop app: a single window with a collapsible sidebar, where each sidebar
entry swaps the main pane to a self-contained developer tool (JSON formatter, Encoder/Decoder,
API Explorer) plus a Settings dialog. It is built on GPUI (Zed's UI framework) and the `gpui-component` widget
library, both pulled from git and pinned only by `Cargo.lock`. See `README.md` for the user-facing
description and `Cargo.toml` for exact dependency sources.

Read `src/main.rs` for the startup sequence and `src/layout.rs` for the view model; the doc
comments there are the authority on structure. This file is only a map.

Most tools are a single `src/<tool>.rs`. **`src/api_explorer/` and `src/docker/` are the
exceptions** and the pattern to copy when a tool outgrows one file: `models/` (plain data, no GPUI,
unit tested), `services/` (the trait that is the only place naming the outside-world crate),
`state/`, `components/`, `views/`. Each `mod.rs` doc comments explain the split and where later
phases plug in. `api_explorer` is also the only tool that registers a key binding
(`api_explorer::init`, called from `main` after `gpui_component::init`, same ordering rule as
`settings::init`).

**`src/docker/`** is the Docker/Podman module. Containers is the built-out page (round 2 added
compose grouping, the filter popover and bulk actions on top of round 1's table); Images/Volumes/
Networks are still placeholder pages. Read `src/docker/mod.rs` — it is the authority — and note the
round-2 pure-logic modules `state/grouping.rs` (compose partition + group status) and
`state/filters.rs` (the multi-filter predicate), both unit-tested without GPUI. Two things unique
to the module:
`services/` is the only place that may name `bollard` (the Docker Engine API client) and the only
place a **tokio runtime** lives — `bollard` is async, so `BollardEngine` drives every call with
`Runtime::block_on` on the background executor, keeping the blocking-by-contract discipline
`Transport` follows. The sidebar's Docker section is the only expandable/nested `SidebarMenuItem`
group; `src/layout.rs` shows the `View` variants and page routing.

**dodo now persists one thing across restarts:** the API Explorer's collections, written by
`services::collection_store::DiskCollectionStore` to `~/Library/Application Support/dodo/`
(`data_dir()`). This is the first user data dodo saves, so the `dodo-theming-settings` skill's
"nothing is persisted across restarts" is now scoped to appearance/language settings only, not
collections. Persistence and initial load run on the background executor, never the UI thread.

## Skills

Detailed, verified knowledge lives in `.claude/skills/<name>/SKILL.md`. Load one when its trigger
fires — they are written to be read at the moment of need, not up front.

| Skill | Load it when |
|---|---|
| `gpui-component-recipes` | Writing or editing any `render` / `new` that builds a gpui-component widget (input, code editor, diagnostics, select, dialog, settings panel, sidebar, button, icon); a widget call will not compile; or a widget builds but nothing appears on screen. |
| `dodo-tool-view` | Adding, renaming, reordering or removing a sidebar tool; a new sidebar entry does not appear or renders blank. |
| `dodo-i18n-text` | Writing or changing **any** text a user reads — a label, title, placeholder, description, error, dropdown option; or an `i18n` / `i18n_lint` test fails. |
| `dodo-theming-settings` | Adding or changing a setting, adding or removing a theme or a language, or a settings change does not apply until restart. |
| `dodo-build-validate` | First `cargo` invocation of a session, adding tests, a build or `cargo test` failing oddly, or being asked whether a UI change actually works. |

Two things that catch everyone and belong here rather than behind a trigger:

- **`Cargo.lock` is the only pin on the four git dependencies.** `cargo update` silently jumps
  them to upstream HEAD. Never run it as a side effect of another task.
- **The pinned `gpui-component` source is the reference for every widget question**, at
  `~/.cargo/git/checkouts/gpui-component-*/<rev>/crates/ui/src` (rev from `Cargo.lock`). Its
  `<checkout>/skills/` directory holds the upstream authors' own guidance, which is excellent on
  GPUI fundamentals and stale in a few places — `gpui-component-recipes` records which.

## Maintaining this file

Keep this file for knowledge useful to almost every future agent session in this project.
Do not repeat what the codebase already shows; point to the authoritative file or command instead.
Prefer rewriting or pruning existing entries over appending new ones.
When updating this file, preserve this bar for all agents and keep entries concise.
