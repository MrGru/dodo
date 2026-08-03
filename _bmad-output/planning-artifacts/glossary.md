# Glossary — Domain Terms

**Source:** `CLAUDE.md`
**Stage:** Phase 3 (Solutioning)

---

- **GPUI** — Zed Industries' GPU-rendered UI framework dodo is built on; pulled from git, pinned
  only by `Cargo.lock`.
- **gpui-component** — Third-party widget library (sidebar, buttons, icons, theming, tables,
  dialogs) also pulled from git; its pinned-revision source under `~/.cargo/git/checkouts/` is the
  reference for widget questions.
- **`View`** — The enum in `src/layout.rs` naming which tool the main pane currently shows.
- **Transport / Engine / Driver** — The one trait per module (`api_explorer`, `docker`,
  `database` respectively) that is the sole place naming the outside-world crate (`reqwest`,
  `bollard`, `postgres`/`rusqlite`).
- **`Exchange`** — API Explorer's protocol-neutral request/response pair; deliberately does not
  know about scripting.
- **`ResponseState`** — Where test results attach, because `Exchange` cannot (a pre-request script
  can define tests for a request that never got a response).
- **`ConsentKey`** — The hash covering both a request's pre-request and post-response script hooks
  together, used by the script-consent gate.
- **`script_origin`** — A property of the request snapshot (`Imported` vs. not), not of the script
  text, so an imported script can't be laundered past consent by re-typing it.
- **`PageBuffer`** — The Database Explorer's memory bound on a query result page (rows, bytes, or
  single-cell size), never a `LIMIT` injected into the user's SQL.
- **`Forest` / `CatalogTree` / `RowRef`** — The Database Explorer's one-tree-many-roots model: one
  `CatalogTree` per connection inside a `Forest`, with every tree row id qualified by its
  connection.
- **`data_dir()`** — The per-platform persisted-data directory (`src/paths.rs`), classified from
  `build_info::VERSION_INFO.target` rather than `#[cfg]`.
- **`QuitMode::LastWindowClosed`** — GPUI's own post-window-removal check that quits dodo when its
  one window closes; not a callback that force-quits.
- **`later_step()`** — The shared "not built yet" placeholder component (`components/later_step.rs`);
  as of today its only remaining caller is API Explorer's OAuth2 auth type.
- **`Str`** — dodo's i18n wrapper; every user-facing string must go through it, enforced by a
  `cargo test` guard.
- **`ScriptPolicy`** — The global controlling whether an imported script's hooks run automatically;
  starts every launch at the cautious "Ask for imported" regardless of what was chosen last session.
- **Coming soon (placeholder)** — A literal disabled UI control with a tooltip, not an absent
  feature — Docker's Exec/Terminal/Create/Pull/Build/Stats/Favorites are this, not stubs.
