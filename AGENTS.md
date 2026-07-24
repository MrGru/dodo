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

**`src/docker/`** is the Docker/Podman module, and it is **feature-complete as of round 5**: four
list pages (Containers with compose grouping, filters, bulk actions; Images/Volumes/Networks),
background polling with incremental merges, keyboard navigation, row context menus, and a
read-only Inspect panel for all four resource types plus a basic container log viewer.
**`src/docker/mod.rs` is the authority** — it documents the layer split, what each round shipped,
and, for the features that are still deliberately disabled "Coming soon" placeholders (Exec/
Terminal, Create/Pull/Build, Stats beyond live CPU%, Favorites), exactly where each one plugs in.
Read it before changing anything here rather than inferring the structure from the files.

Four things about the module that are not obvious from any one file:

- **`services/` is the only place that may name `bollard`**, and the only place a **tokio runtime**
  lives. `bollard` is async, so `BollardEngine` drives every call with `Runtime::block_on` on the
  background executor, keeping the blocking-by-contract discipline `Transport` follows. Inspect
  responses cross that boundary as `serde_json::Value`, so the field extraction in
  `models/inspect.rs` stays testable without a daemon.
- **`docker::init` registers the module's key bindings** and must run from `main` after
  `gpui_component::init` — the same tie-break rule as `api_explorer::init`. Bindings are scoped to
  the `DockerList` key context; the actions themselves are declared in `src/docker/mod.rs`.
- **`docker::POLL_INTERVAL` is a constant, not a setting, on purpose** (5s). Exactly one visible
  page polls (`DockerView::should_poll`), and leaving the section calls `set_section_active(false)`
  (wired in `layout.rs`), so an idle cadence never runs.
- The sidebar's Docker section is dodo's only expandable/nested `SidebarMenuItem` group;
  `src/layout.rs` shows the `View` variants and page routing.

**dodo now persists one thing across restarts:** the API Explorer's collections, written by
`services::collection_store::DiskCollectionStore` to `~/Library/Application Support/dodo/`
(`data_dir()`). This is the first user data dodo saves, so the `dodo-theming-settings` skill's
"nothing is persisted across restarts" is now scoped to appearance/language settings only, not
collections. Persistence and initial load run on the background executor, never the UI thread.

**Build and release engineering lives in `docs/`**, and those two files are the authority for it:
`docs/build-optimization.md` (release profile, the measured before/after size table, linker
findings, the dependency report, startup review) and `docs/release.md` (CI, the release workflow,
packaging, verification, the application icon, future signing/notarisation placeholders). The rest
is `Cargo.toml`'s `[profile.*]` comments, `build.rs`, `scripts/` and `.github/`.

**The application icon is a committed pipeline, not a file someone dropped in.** `assets/branding/`
holds the original artwork and the 1024 RGBA master; `python3 scripts/generate-icons.py` derives
the macOS `.icns`, the Windows `.ico` and the Linux hicolor PNGs from it, and all of those are
committed because packaging must not depend on the host (`iconutil` is macOS-only). Read
"Application icon" in `docs/release.md` before touching any of it — it records why the Windows
`.exe` does not embed its icon, why GPUI's `WindowOptions::icon` is not set, and that a `.icns`
`iconutil` accepted can still render blank. **Do not confuse `assets/{branding,macos,windows,
linux}` with `assets/icons`**: only `icons/**/*.svg` and `themes/**/*.json` are embedded in the
binary (the `#[include]` filters in `src/assets.rs`), which is why the branding artwork costs zero
bytes. Anything new under `assets/` that must stay out of the binary has to stay outside those two
filters — measure the binary, do not assume.

Three things about build and release that catch people:

- **`fmt` and `clippy` are blocking jobs; keep them green.** Run `cargo fmt --all` and
  `cargo clippy --all-targets --locked -- -D warnings` before committing. The pre-existing debt
  (34 unformatted files, 12 warnings) is paid off; there is no crate-level `allow`, and the two
  surviving suppressions are `#[allow]`ed at their definition with the reason inline.
  `build (windows-x64)` still fails and `build (macos-x64)` is unverified — those rows are
  `experimental` and non-blocking on purpose. See the honesty note atop `.github/workflows/ci.yml`
  for what has actually run.
- **`Cargo.lock` really is the only possible pin on the four git dependencies.** Explicit
  `rev = "…"` pins were tried and cannot work here — upstream depends on itself through unpinned
  default-branch refs, and the three resulting cargo errors are recorded in
  `docs/build-optimization.md`. Hence `--locked` everywhere, and `cargo update` only ever as its
  own reviewed commit.
- **`dodo --version` / `--build-info`** print what `build.rs` embedded and exit before any window
  opens (`print_build_metadata_and_exit` in `src/main.rs`). That path is how CI proves a packaged
  binary runs at all — a GUI app cannot open a window on a headless runner — so keep it free of
  GPUI initialisation.

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
  them to upstream HEAD. Never run it as a side effect of another task. (Why an explicit `rev`
  pin cannot replace it: `docs/build-optimization.md`.)
- **The pinned `gpui-component` source is the reference for every widget question**, at
  `~/.cargo/git/checkouts/gpui-component-*/<rev>/crates/ui/src` (rev from `Cargo.lock`). Its
  `<checkout>/skills/` directory holds the upstream authors' own guidance, which is excellent on
  GPUI fundamentals and stale in a few places — `gpui-component-recipes` records which.

## Maintaining this file

Keep this file for knowledge useful to almost every future agent session in this project.
Do not repeat what the codebase already shows; point to the authoritative file or command instead.
Prefer rewriting or pruning existing entries over appending new ones.
When updating this file, preserve this bar for all agents and keep entries concise.
