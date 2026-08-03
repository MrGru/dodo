# Architecture Spine — dodo

**Source:** `CLAUDE.md`, `src/main.rs`, `src/layout.rs`, and each module's `mod.rs`
**Stage:** Phase 3 (Solutioning)

---

## 3.1 System context

```
+---------------------------------------------------------------------+
|                              DodoApp (src/app.rs)                    |
|                                                                       |
|   +----------------------------------------------------------------+ |
|   |                    Layout (src/layout.rs)                       | |
|   |   Sidebar (collapsible)              Main pane (View enum)      | |
|   |   - Json formatter                                              | |
|   |   - Encoder / Decoder                                           | |
|   |   - API Explorer         -----> src/api_explorer/               | |
|   |   - Docker                -----> src/docker/                    | |
|   |   - Database Explorer     -----> src/database/                  | |
|   |   (footer: Settings dialog, "Check for updates" -> src/updater/) | |
|   +----------------------------------------------------------------+ |
+---------------------------------------------------------------------+
        |                    |                    |
        v                    v                    v
   reqwest (rustls)     bollard (tokio)      postgres / rusqlite
   -> target API        -> Docker/Podman     -> target database
                            Engine API           (own private runtime
                                                   per client, no shared
                                                   tokio)
```

Every tool is reached only through the sidebar's `View` variant; the sidebar and `layout.rs` know
nothing about a tool's internals. Five persisted JSON files live under `data_dir()`
(`src/paths.rs`), each behind a store trait, loaded/saved on the background executor only.

## 3.2 Component / responsibility map

| Component | Responsibility | Touched by |
|---|---|---|
| `src/main.rs` | Startup sequence, `gpui_component::init`, module `init` calls, `--version`/`--build-info` path, `attach_parent_console`, `QuitMode::LastWindowClosed` | **E0** |
| `src/app.rs` | `DodoApp` — top-level view holding the `Layout` | **E0** |
| `src/layout.rs` | Sidebar + main pane, sidebar collapse, `pane_title` | **E0** |
| `src/json_formatter.rs` | Pretty-print + inline diagnostic | **E1** |
| `src/encoder_decoder.rs` | Base64/URL/Hex both directions + JWT inspector | **E2** |
| `src/api_explorer/` (`models/`, `services/`, `state/`, `components/`, `views/`) | HTTP client: send pipeline, scripting, consent, codegen, curl import, collections | **E3** |
| `src/docker/` (same 5-layer split) | Docker/Podman manager: 4 list pages, polling, detail dialog | **E4** |
| `src/database/` (same 5-layer split) | Connection tree, query execution, export, cancel/explain | **E5** |
| `src/updater/` (same 5-layer split) | Check/ask/download/verify/install/restart | **E6** |
| `src/settings.rs`, `src/i18n.rs` | Settings dialog, theme registry, language switching, `Str` | **E7** |
| `src/paths.rs` | `data_dir()` per platform, classified from `build_info::VERSION_INFO.target` | **E8** |
| `src/app_icon.rs`, `src/assets.rs` | `AppIcon` enum, embedded-SVG asset source | **E0/E7** |
| `src/i18n_lint.rs` | The two i18n `cargo test` guards | **E7** |
| `src/build_info.rs` | Build metadata embedded by `build.rs` | **E0/E9** |
| `docs/`, `.github/workflows/`, `scripts/generate-icons.py`, `tools/update-manifest/` | CI, release workflow, icon pipeline, update manifest generator | **E9** |

## 3.3 The five-layer split (api_explorer, docker, database)

Each of the three modules that outgrew a single file shares this shape, documented per-module in
its own `mod.rs`:

- **`models/`** — plain data, no GPUI, unit-tested directly.
- **`services/`** — the trait that is the *only* place naming the outside-world crate
  (`reqwest` for API Explorer's `Transport`, `bollard` for Docker's `Engine`, `postgres`/`rusqlite`
  for Database's `Driver`). This is what makes a fake implementation possible in tests and what
  keeps a second backend (a new database driver, say) a one-file addition.
- **`state/`** — per-tab or per-view mutable state (`state/tab.rs` in API Explorer,
  `state/tree.rs` in Database).
- **`components/`** — small reusable render pieces.
- **`views/`** — the actual GPUI render methods.

## 3.4 Epic catalog (E0-E9)

See `epics/E*.md` for the full per-epic story tables. Ten epics, matching dodo's real module
boundaries rather than a hackathon's competition-scoped feature list:

| Epic | Title | Status |
|---|---|---|
| **E0** | Core shell & app lifecycle | Done |
| **E1** | JSON Formatter | Done |
| **E2** | Encoder / Decoder | Done |
| **E3** | API Explorer | Done, one placeholder (OAuth2) |
| **E4** | Docker / Podman module | Done, four disabled placeholders |
| **E5** | Database Explorer | Done (round 2), many stated cuts |
| **E6** | In-app Updater | Done |
| **E7** | Theming, Settings & i18n | Done |
| **E8** | Persistence & `data_dir()` | Done |
| **E9** | Build, Release & Licensing engineering | Done, one open question (GPL distribution) |

## 3.5 Dependency graph

See `dependency-graph.md`. Unlike a sprint plan racing toward a deadline, these epics do not form
a strict build-order DAG today — they are mostly already shipped — but the *architectural*
dependencies still hold and matter for future work: E3/E4/E5 each depend on E0 (the shell) and E8
(persistence); E9 depends on all of them existing before it can package/release them; E7
(theming/i18n) is a cross-cutting dependency of every view in every other epic.

## 3.6 What this spine deliberately does not cover

Per `product-brief.md` §1.6: Docker's Exec/Terminal/Create/Pull/Build/Stats/Favorites, the
Database Explorer's editing/CRUD/autocomplete/second-backend set, API Explorer's OAuth2, and a
settled GPL-3.0 distribution answer. These are not missing from this document by oversight — they
are missing from the *product* by explicit, recorded decision, and reintroducing any of them is a
new epic with its own ADR, not a gap-fill.
