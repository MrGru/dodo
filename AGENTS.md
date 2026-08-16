# Project agent memory

`dodo` is a Rust desktop app: a single window with a collapsible sidebar, where each sidebar entry
swaps the main pane to a self-contained developer tool (JSON formatter, Encoder/Decoder, API
Explorer, Docker, Database Explorer, Cleaner, and on macOS and Windows an Input method) plus, in
the sidebar footer, a Settings dialog and a **Check for updates** dialog. It is built on GPUI
(Zed's UI framework) and the `gpui-component` widget library, both pulled from git and pinned only
by `Cargo.lock`. `README.md` is the user-facing description and `Cargo.toml` names the exact
dependency sources.

**The source is the authority here, and it is written to be.** `src/main.rs` owns the startup
sequence and carries, beside every crate alias, the reason that crate exists; `src/layout.rs` owns
the view model; `src/tools.rs` is the tool table and adding a tool is one row in it; each crate's
`lib.rs` doc comment is the authority on that crate. Nearly every module in dodo carries a `//!`
block stating the decisions behind it. Read those rather than a summary of them — this file is a
map, and the map is deliberately smaller than the territory.

## Loading discipline

This file is loaded at the start of every session, so it holds only what is true for every session
regardless of what is being touched. Everything else sits behind a trigger, and the triggers are
the router below.

- **Do not read the docs, the crate `AGENTS.md` files or the skills up front**, and do not
  recursively scan `docs/` or `crates/*/AGENTS.md` to "get oriented". A session that only touches
  the JSON formatter must not pay for the Cleaner's internals or the input-method stack.
- **Load exactly what the router points at for the task in hand**, at the moment the task reaches
  it. That is normally one row and one file.
- **Load an architecture doc only when the change is genuinely cross-cutting** — it crosses a
  crate boundary, changes a public API, touches shared persisted state, or touches a platform
  abstraction. An edit that stays inside one crate does not qualify.
- If the router has no row for what you are doing, read the module's own `//!` docs. That is the
  intended fallback and it is usually the whole answer.

## Invariants

These hold everywhere in dodo, whatever you are touching.

- **`Cargo.lock` is the only pin on the four git dependencies**, and `cargo update` silently jumps
  them to upstream HEAD. Never run it as a side effect of another task; only ever as its own
  reviewed commit. An explicit `rev = "…"` pin was tried and **cannot** replace it — upstream
  depends on itself through unpinned default-branch refs, and the three resulting cargo errors are
  recorded in `docs/build-optimization.md`. Hence `--locked` on every cargo invocation.

- **Every string a user reads goes through `dodo-i18n`**, never a bare literal in view code. Load
  `dodo-i18n-text` before writing or changing one; two `cargo test` guards enforce it, and a
  failing guard means the code is wrong rather than the test.

- **Cheap `render` bodies are a contract, not an optimisation.** "`render` only runs when something
  changed" is false in gpui: a dirty view marks its whole *ancestor* path dirty, and an ancestor
  re-rendering sets `Window::refreshing`, which bypasses the element cache for every *descendant* —
  so a child view scrolling, a progress tick, or a redraw anywhere above re-runs your `render` with
  nothing of its own changed. A `render` that copies a whole collection pays that copy per frame,
  however well its rows are virtualized. Stamp a revision where the data is mutated and compare it
  before re-copying; `crates/dodo-cleaner/AGENTS.md` has the worked pattern and the measurements.
  Relatedly: **never use a prepaint callback to mutate and `notify` a view for the next frame** —
  `WindowInvalidator::invalidate_view` schedules a redraw only in `DrawPhase::None`, so a notify
  during layout/prepaint records state without dirtying the window and waits for an unrelated
  event.

- **A platform-conditional answer is a value chosen by `HostOs` or `cfg!`, not an item behind
  `#[cfg]`, wherever that is possible.** Two of dodo's four release targets cannot be built from a
  Mac at all, so an answer expressed as a `#[cfg]` item is an answer nobody here can compile or
  test; expressed as a `const fn` over `cfg!` or as a pure function taking a `HostOs`, every
  platform's answer is asserted from any machine. A gate in front of a genuinely platform-only API
  still has to be an attribute — that is the line.

- **`cargo fmt --all` and `cargo clippy --all-targets --locked -- -D warnings` are blocking CI jobs.**
  Run both before committing. `cargo build` alone does not prove the tree is green, and there is no
  crate-level `allow` in dodo; `dodo-build-release-internals` owns the suppression rules.

- **The pinned `gpui-component` source is the reference for every widget question**, at
  `~/.cargo/git/checkouts/gpui-component-*/<rev>/crates/ui/src` (rev from `Cargo.lock`). Its
  `<checkout>/skills/` directory holds the upstream authors' own guidance, which is excellent on
  GPUI fundamentals and stale in a few places — `gpui-component-recipes` records which.

## Router

One index, covering the skills (`.claude/skills/<name>/SKILL.md`, invoked by name), the crate-local
`AGENTS.md` files, and `docs/`. Load a row when its trigger fires, and not before.

| When you are… | Load |
|---|---|
| Running `cargo` for the first time this session; adding tests; a build or `cargo test` failing oddly; asked whether a UI change actually works | skill `dodo-build-validate` |
| Writing or changing **any** text a user reads — label, title, placeholder, description, error, dropdown option — or an `i18n` / `i18n_lint` test fails | skill `dodo-i18n-text` |
| Writing or editing a `render` / `new` that builds a gpui-component widget, adding a key binding, or a widget will not compile / builds but does not appear | skill `gpui-component-recipes` |
| Adding, renaming, reordering or removing a sidebar tool; a new sidebar entry is blank; a tool page is unreachable at a small window | skill `dodo-tool-view` |
| Adding or changing a setting, a theme or a language, or a settings change does not apply until restart | skill `dodo-theming-settings` |
| Touching `crates/dodo-api-explorer/` | skill `dodo-api-explorer-internals` |
| Touching `crates/dodo-docker/` | skill `dodo-docker-internals` |
| Touching `crates/dodo-database/` | skill `dodo-database-internals` |
| Touching `crates/dodo-flow/` — the Flow Canvas engine, still being built and not yet in the sidebar | `crates/dodo-flow/src/lib.rs`'s doc comment, then `crates/dodo-flow/src/budgets.rs` before changing anything that paints |
| Touching `crates/dodo-updater/`, `.github/workflows/`, `Cargo.toml`'s dependencies, `scripts/`, `tools/update-manifest/`, `deny.toml` or `THIRD-PARTY-NOTICES.md`; preparing or debugging a release; the application-icon pipeline; the CI platform matrix and its cross-check traps | skill `dodo-build-release-internals` |
| Touching `crates/dodo-cleaner/` | `crates/dodo-cleaner/AGENTS.md` |
| Touching `crates/dodo-ime-core/` — the Vietnamese engine and the shared key vocabulary | `crates/dodo-ime-core/AGENTS.md` |
| Touching `crates/dodo-input-method/` — dodo's own end of the input methods | `crates/dodo-input-method/AGENTS.md` |
| Touching `crates/dodo-ime-macos/`, installing or enabling the bundle, or the two files the two processes exchange | `docs/macos-input-method.md` |
| Touching `crates/dodo-ime-windows/` or the Keyboard Hook fallback | `docs/windows-input-method.md` |
| Signing or notarisation, on any platform | `docs/macos-signing.md` |
| Cleaner scanner, safety, privacy or limitation detail beyond the crate file | `docs/cleaner/` |
| Startup, app lifecycle, window close/quit, or the shape of `src/` itself | `docs/architecture/app-shell.md` |
| Reading or writing anything under `data_dir()`; adding a persisted file; session restore, window geometry, or the sidebar's tool list | `docs/architecture/persistence.md` |
| Adding a crate, extracting a feature out of the binary, or moving code across a crate boundary | `docs/architecture/workspace-layout.md` |
| Quick navigation — pasting into whichever tool can read it, normal mode, `Esc` | `src/quick_nav/mod.rs` and `src/quick_nav/models/detect.rs` doc comments |
| The menu bar / notification-area item | `src/tray/mod.rs`, `src/tray/icon.rs` and `src/tray/menu.rs` doc comments |
| Binary size, the release profile, or whether a crate split will speed up builds | `docs/build-optimization.md` |

## Maintaining this file

This file is the top tier of four, and the tiering is the point:

1. **Root `AGENTS.md`** — global rules, invariants, and the router. Every session pays for it, so
   it stays small. Nothing crate-specific belongs here.
2. **`crates/<crate>/AGENTS.md`** — knowledge local to one crate that its own `lib.rs` docs cannot
   hold because it spans several files. Only three crates have one; do not add a fourth unless a
   crate's knowledge is genuinely stranded and no skill covers it.
3. **`docs/`** — focused feature, platform and architecture knowledge, loaded on demand.
4. **`.claude/skills/<name>/SKILL.md`** — procedural knowledge behind a trigger, unchanged in shape.

**One owner per fact.** Every fact lives in exactly one file and everything else links to it. If a
fact only matters when touching one crate, it belongs in that crate's `AGENTS.md`, its `lib.rs`
docs or its skill — never here. If the source already states it, point at the source instead of
copying it: keep the *why*, the decision and the trap, and drop the *what*. Prefer rewriting or
pruning an existing entry over appending a new one, and when you add a row to the router, check
that nothing else already claims it.
