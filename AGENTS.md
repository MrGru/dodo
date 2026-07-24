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

**`src/docker/`** is the Docker/Podman module. All four pages are built out: Containers (round 2 —
compose grouping, filter popover, bulk actions) and, since round 3, Images/Volumes/Networks — real
list pages with search, Refresh, per-row Delete (confirm) and the loading/empty/error+retry states,
sharing round 1–2's `components/`. Read `src/docker/mod.rs` — it is the authority. The three round-3
pages share one generic store, `state/resource.rs`'s `ResourceState<T>` (only Containers needs the
selection/grouping/filter machinery); their pure logic lives in `models/{image,volume,network,
size,usage}.rs`, all unit-tested without GPUI — note `models/usage.rs`, the "containers using"
derivation the three pages count against (from the container set, not the engine's own counters).
The round-2 pure-logic modules `state/grouping.rs` (compose partition + group status) and
`state/filters.rs` (the multi-filter predicate) remain Containers-only. Inspect, Logs, Terminal and
a Create/Build/Pull flow are still placeholders for a later round (the round-4 context menu shows
them disabled under a "Coming soon" label). Two things unique to the module:
`services/` is the only place that may name `bollard` (the Docker Engine API client) and the only
place a **tokio runtime** lives — `bollard` is async, so `BollardEngine` drives every call with
`Runtime::block_on` on the background executor, keeping the blocking-by-contract discipline
`Transport` follows. The sidebar's Docker section is the only expandable/nested `SidebarMenuItem`
group; `src/layout.rs` shows the `View` variants and page routing.

**Round 4 (background polling, keyboard nav, context menus).** The active Docker page
auto-refreshes off the UI thread on `docker::POLL_INTERVAL` (a documented constant, default 5s —
not a setting on purpose). `DockerView` gates it: `should_poll(section_active, active_page, page)`
means exactly one visible page polls; leaving for another tool calls `set_section_active(false)`
(wired in `layout.rs`) and pauses it. Each page's `set_polling`/`start_poll_loop` re-lists and
**merges incrementally** — `ContainersState::merge_rows` / `ResourceState::merge` return whether
anything changed so an unchanged tick skips `cx.notify()`, and selection/scroll/expanded/search all
survive; the pure diff helpers (`state/diff.rs`, CPU carry-forward + change detection) are
unit-tested. An unreachable engine degrades via `set_poll_error` (transition-only re-render, no
spam). Containers runs its CPU sweep inline in the same loop so sweeps never overlap. Keyboard nav
and the right-click menus route through GPUI **actions** defined in `src/docker/mod.rs` and bound by
`docker::init` (called from `main` after `gpui_component::init`, same tie-break rule as
`api_explorer::init`), scoped to the `DockerList` key context: `up`/`down` move the highlighted row
(`state/focus.rs`), `space`/`x` toggle selection (Containers), `cmd-r` refreshes. Row context menus
use `gpui_component`'s `ContextMenuExt::context_menu`; the right-clicked row's key is stashed in the
view's `context_target` on right mouse-down before the menu builds. The shared `components/toolbar.rs`
now `flex_wrap`s and each table's inner list is `w_full` + `min_w` (flex on wide, scroll on narrow).

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
