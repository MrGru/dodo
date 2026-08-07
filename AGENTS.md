# Project agent memory

`dodo` is a Rust desktop app: a single window with a collapsible sidebar, where each sidebar
entry swaps the main pane to a self-contained developer tool (JSON formatter, Encoder/Decoder,
API Explorer, Docker, Database Explorer, Cleaner) plus, in the sidebar footer, a Settings dialog and a
**Check for updates** dialog. It is built on GPUI (Zed's UI framework) and the `gpui-component`
widget library, both pulled from git and pinned only by `Cargo.lock`. See `README.md` for the
user-facing description and `Cargo.toml` for exact dependency sources.

Read `src/main.rs` for the startup sequence and `src/layout.rs` for the view model; the doc
comments there are the authority on structure. This file is only a map — for anything below the
map's resolution, load the matching skill from the table below rather than reading a whole module
cold.

**`_bmad/`, `_bmad-output/` and `bmad.config.yaml` are tracked, and are not the authority for
anything.** They are bmad scaffolding kept for contributors who work that way; the repo owner does
not, and decided on 2026-08-05 that they stay — so their presence is settled, not an oversight.
Authority is this file, the skills it indexes, each module's `mod.rs` doc comments and `docs/`.
`_bmad-output/` reads authoritative and is not: its PRD, epics, per-story files and
`sprint-status.yaml` are not kept in step with what actually lands. A session that is not
deliberately running a bmad workflow should not read, follow or update any of it — in particular
it must not mark a story or `sprint-status.yaml` to reflect work it just did.

`src/main.rs` also owns **app lifecycle**, and both halves are counter-intuitive enough to name
here: a release Windows build is a **GUI-subsystem** binary (no console window behind the app),
which costs it valid standard handles, so `attach_parent_console` buys them back on the
`--version` / `--build-info` path alone — and **no shell waits for a GUI-subsystem process at
all**, so every Windows smoke test must run it through `Start-Process -Wait`, never PowerShell's
`&`. Capturing into a variable does *not* make the shell wait; believing it did is what cost
v0.1.5 its Windows archive, and the same wrong claim sat in three comments and one doc before
being corrected. "What 'verified' means" in `docs/release.md` has the panic, the run IDs and the
fix. Closing the single window quits the
app through **`QuitMode::LastWindowClosed`** (GPUI's own check, run after the window is removed,
not a callback that force-quits) plus a macOS-only `cmd-w` binding, needed because dodo installs
no menu bar for that shortcut to hang off. The doc comments there carry the reasoning;
`docs/release.md` records that the Windows half has never run on a Windows host.

Most tools are a single `src/<tool>.rs`. **`src/api_explorer/`, `src/docker/` and
`src/database/` are the exceptions** and the pattern to copy when a tool outgrows one file:
`models/` (plain data, no GPUI, unit tested), `services/` (the trait that is the only place naming
the outside-world crate), `state/`, `components/`, `views/`. Each module's own `mod.rs` doc
comments are the authority on its split and what shipped when; the matching skill below is where
the non-obvious parts of each are written down — load it before changing anything in one of these
three modules rather than inferring the design from the files cold.

**`src/cleaner/` is the same shape but started unfinished, and rounds have been landing since.**
Its `mod.rs` doc comment is the authority on what has shipped; do not assume it is still round 1
without checking. `core/` still carries `#[allow(dead_code)]` on items ahead of what constructs
them — **those are pending work, not dead code to delete** — but the allow comes off as each
producer lands: `core::safety` now has a real `validate_path` and a moved-to-trash cleanup path
(`macos::cleanup::cleanup_items`), so its allow is already gone. `core::permissions` is still the
one whole-module allow marking an area that does not exist at all yet.

**dodo persists nine things across restarts**, all under `data_dir()` (`src/paths.rs`) and each
behind a trait so the state layer never learns where they live: `collections.json`
(`api_explorer::services::collection_store`), `environments.json` (`services::variable_store`),
`script-consent.json` (`services::consent_store`, the imported scripts the user has approved),
`updater.json` (`updater::services::config_store`), `connections.json`
(`database::services::connection_store`, which also holds database passwords in plain text — see
`dodo-database-internals`), `query-data.json` (`database::services::query_store`, saved queries
plus bounded query history, with query text intentionally stored as plain text), `quick-nav.json`
(`quick_nav::services::config_store`), `cleaner-ignored-items.json`
(`cleaner::services::ignore_store`, the orphan-detection candidates the user has marked "Keep",
keyed by absolute path string rather than a `CleanableItemId` since that id is a session-local
hash with no promise of surviving a restart) and `session.json`
(`session::services::session_store`). Persistence and initial load run on the background executor,
never the UI thread.

**`session.json` is what makes "nothing is persisted across restarts" obsolete**, and any doc still
saying it — including the `dodo-theming-settings` skill — is stale rather than describing a
decision. The captain asked for session restoration on 2026-08-06, and `src/session/mod.rs` is the
authority: theme, font size, border radius, language, the window's rectangle **and mode**, the open
tool, the sidebar's collapsed state, and **which tools the sidebar lists at all and in what order**.
The other eight files persist something `session.json` does not attempt: *what the user typed or
decided about one specific thing* — an approved script, a saved query, a cleaner path marked
"Keep", a skipped update version, an edited quick-nav pattern — which cannot expire each launch
without becoming a lie. **The one exception is `Run scripts`**, a `ScriptPolicy`
global that still starts each launch at the cautious `Ask for imported` — it is the gate in front of
running code that arrived inside someone else's collection file, not a preference, and its approvals
are persisted per script in `script-consent.json` instead. Do not quietly start persisting it; that
is the captain's call.

Two things about that file that are decisions rather than details. **Every field is an `Option` and
absent means *never chosen*** — writing `"Default Light"` into a fresh file merely because that was
on screen would freeze system-appearance following for everyone who never opened the dialog. And
**restoring window geometry opts out of gpui's own placement care**: `Window::new` only cascades and
clamps in `default_bounds`, the branch it takes when `window_bounds` is `None`, so a supplied
rectangle goes to the platform unexamined. `session::models::geometry` is the replacement — clamp to
a display that still exists, honour `layout::window_min_size`, centre on the preferred display when
the saved one is gone — and it is a pure function so all of it is tested without a frame. It is
paired with a saved display **UUID**, because on macOS the rectangle alone cannot say which monitor
it meant: every coordinate the pinned gpui reports there is display-*local* (`MacDisplay::bounds`
returns `(0, 0)` for every display). `models::document::WindowRecord` names the four functions that
prove it. Do not "simplify" that pairing away.

**The Features settings page is why `View::ALL` is no longer the sidebar's order.** The captain
asked on 2026-08-06 for per-tool on/off plus drag reordering, persisted; `session::models::features`
is the authority and is pure, so every rule is a unit test rather than something found by looking at
the app. Four of them matter outside that file: a stored entry naming a tool this build lacks is
**dropped** and a tool the file never names comes back **beside its default neighbour**, enabled;
**at least one tool always stays visible**, enforced in the model and drawn as a disabled switch with
the reason beside it; the tool on screen is always a listed one, which is the single function
`Features::active` answering both "the remembered tool was switched off" and "the open tool was just
switched off"; and **a switched-off tool is not a quick-navigation route** — `layout` drops its
detectors before `detect_among` runs, so a pasted `curl` with the API Explorer off falls through
rather than reopening it. The last one is the trap: `detect_among`'s allowed list is a **membership
test and never an order**, because `Detector::ORDER` is a correctness property (most specific first)
and the sidebar's order is a preference. `Layout::features` is the live list, `View::for_detector` is
the single mapping both `apply_route` and `allowed_detectors` read, and `settings::features_page`
builds the rows by hand because a `SettingItem` cannot carry a position. Adding the field is also
what took `session.json` to **schema version 2**; an older build would have read it, dropped the key
and written it back pruned.

**`src/quick_nav/` is the one feature that is not a tool**: a vim-shaped normal mode where
`Cmd+V` / `Ctrl+V` / `p` sends the clipboard to whichever tool can read it, and `Esc` leaves a
focused input. Its module docs are the authority and are worth reading before touching key
handling anywhere — three things there are counter-intuitive. **Normal mode is a key-binding
context, not a flag**: the bindings carry `Dodo && !Input`, gpui evaluates `!` against the whole
dispatch path, and that is the only definition — so `p` still types a `p` inside every text field
and ordinary paste is untouched. **The pane holds a focus handle for this**, because with nothing
focused gpui's dispatch path is the window root alone and carries none of the pane's context.
And **`Esc` is bound at the pane, deliberately shallower than every library `Escape`**: gpui tries
matched bindings deepest-first until one stops propagating, so a dialog, a popover, a select and
an input's own completion popup all still win, and this fires only once they have declined.
`models/detect.rs` carries the detection order (most-specific first: cURL → database URI → JWT →
JSON → Base64) and the rule that resolves the captain's editable-regex request against dodo's
existing parsers — *a pattern selects candidates, the parser confirms*. `layout.rs`'s
`apply_route` is the single place a detected route meets a `View`.

`data_dir()` lives in `src/paths.rs`, not under `api_explorer/` any more, and it knows all
three platforms: `~/Library/Application Support/dodo`, `%APPDATA%\dodo`, `$XDG_CONFIG_HOME` or
`~/.config`. The macOS path is frozen — changing it orphans every existing installation's saved
collections. It classifies the platform from `build_info::VERSION_INFO.target` rather than
`#[cfg]`, which is what lets all three branches be unit tested from a Mac that cannot compile two
of them; copy that trick rather than a `cfg` split for anything else platform-shaped and pure.

The files version differently, and the difference is deliberate. A `RequestSnapshot` inside
`collections.json` is versioned only by `#[serde(default)]`, which copes with *added* fields and
nothing else. `environments.json`, `script-consent.json`, `updater.json`, `connections.json`,
`query-data.json`, `quick-nav.json` and `cleaner-ignored-items.json` carry an explicit
`"version"` from their very first write, and their `parse_document` **refuses** a file whose
version is higher rather than half-reading it. Copy that pattern for any new file; do not copy
`collections.json`'s.

**Build and release engineering lives in `docs/`**, and those two files are the authority for it:
`docs/build-optimization.md` (release profile, the measured before/after size table, linker
findings, the dependency report, startup review) and `docs/release.md` (CI, the release workflow,
packaging, verification, the application icon, the in-app updater, future signing/notarisation). The rest
is `Cargo.toml`'s `[profile.*]` comments, `build.rs`, `scripts/` and `.github/`.

**The application icon is a committed pipeline, not a file someone dropped in.** `assets/branding/`
holds the original artwork and the 1024 RGBA master; `python3 scripts/generate-icons.py` derives
the macOS `.icns`, the Windows `.ico`, the Linux hicolor PNGs and one 256px PNG from it, and all of
those are committed because packaging must not depend on the host (`iconutil` is macOS-only). Read
"Application icon" in `docs/release.md` before touching any of it — it is the authority on all five
launch paths, on which of them anyone has actually looked at (one: the macOS bundle), and on the
fact that a `.icns` `iconutil` accepted can still render blank. **The icon is answered in three
unrelated places**, which is the part that catches people: `build.rs` compiles the `.ico` into
`dodo.exe`, `scripts/macos-app-bundle.sh` puts the `.icns` in the bundle, and `src/window_icon.rs`
covers at runtime what no file can — a bare macOS binary's Dock tile, and the Linux `app_id` that
`dodo.desktop` is matched against.

**Do not confuse `assets/{branding,macos,windows,linux}` with `assets/icons`**: only
`icons/**/*.svg` and `themes/**/*.json` are embedded in the binary through `rust-embed` (the
`#[include]` filters in `src/assets.rs`), which is why the packaged icon artwork costs zero bytes.
The one exception is `assets/branding/dodo-256.png`, which `src/window_icon.rs` pulls in with
`include_bytes!` — a different mechanism the filters do not govern. Anything new under `assets/`
that must stay out of the binary has to stay outside those two filters *and* out of an
`include_bytes!`.

**Every release publishes an `update.json` manifest**, generated by
`tools/update-manifest` — a **standalone crate that is not part of dodo**. `exclude = ["tools/*"]`
in the root `Cargo.toml` keeps it out of the package (`cargo metadata --no-deps` lists exactly one
package), it carries its own `Cargo.lock` and four dependencies, and it is built only by the
release workflow through `--manifest-path`. It costs the binary zero bytes. Do not add it to a
workspace, and do not give dodo a `[[bin]]`. "Automatic updates" in `docs/release.md` is the
authority: the manifest shape and why `manifest_version` / `signature` / `channel` exist, the
hand-verification recipe, and the channel design. Three things that are
decisions rather than details: the manifest points at macOS's **`-app.tar.gz` bundle** selected by
exact filename (an installer swaps the `.app`); **any missing platform fails the release**,
experimental ones included, because a silently absent platform means those users are never offered
an update; and the publish step is **create-or-update**, because `gh release create` cannot repair
a tag that already exists and tags here are immutable. `src/updater/` is what reads it.

Seven things about build and release that catch people:

- **Two of the four `cargo check` targets cannot be run from this Mac at all.**
  Linux and Windows both die in `aws-lc-sys`'s C build script (no cross C toolchain, no
  `windows.h`) — not a portability problem in dodo, and not fixable by a cargo flag. The two Apple
  targets do cross-check locally. "The `check` row runs natively" in `docs/release.md` has the
  detail, including the two traps that cost time: Homebrew's `rustc` shadows rustup's and ships
  only the host std (`rustup run` does **not** fix it — use the toolchain's absolute path), and a
  cross-check needs its own `CARGO_TARGET_DIR` or it invalidates the warm cache a size
  measurement depends on.
- **`fmt` and `clippy` are blocking jobs; keep them green.** Run `cargo fmt --all` and
  `cargo clippy --all-targets --locked -- -D warnings` before committing. The pre-existing debt
  (34 unformatted files, 12 warnings) is paid off, and **there is no crate-level `allow`** — every
  suppression is `#[allow]`ed at the item it applies to (or, where a whole module is the pending
  unit, as an inner attribute under that module's `//!` docs) with the reason and the condition for
  removing it written next to it. Copy that shape; never widen an `allow` to quieten a lint.
  Dead-code warnings in a module under construction are **scaffolding, not defects**: annotate,
  do not delete. `.githooks/pre-push` runs `fmt`, `clippy` and `cargo test --locked` and refuses
  the push if any fails; it is opt-in per clone with `git config core.hooksPath .githooks`
  (see "Pre-push checks" in `README.md` for its cost and the `--no-verify` bypass).
  Note that `cargo build` alone does **not** prove the tree is green — `src/i18n.rs`'s test module
  is exhaustive over `Str`, so new strings break `cargo test` while the app still builds.
  `build (windows-x64)` failed on its one real run (a `#[cfg(unix)]`-only bollard connector; fixed
  by the platform split in `docker/services/engine.rs`, not yet confirmed green) and
  `build (macos-x64)` is unverified — those rows are
  `experimental` and non-blocking on purpose. See the honesty note atop `.github/workflows/ci.yml`
  for what has actually run.
- **No `--release` build runs on a push any more.** `ci.yml` does `cargo check` per platform plus
  one debug build; the four-platform release matrix lives in
  `.github/workflows/release-profile.yml` (weekly + manual) and, for a tag, in `release.yml`. The
  accepted cost — release-only failures surface up to a week late — is stated at the top of both
  `ci.yml` and "CI architecture" in `docs/release.md`. Do not quietly re-add a release build to
  the push path.
- **dodo's source is MIT (`LICENSE`), and that does not settle how binaries may be distributed.**
  `gpui -> sum_tree -> ztracing -> zlog` pulls GPL-3.0-or-later into every build.
  `THIRD-PARTY-NOTICES.md` is the authority: it records the verified chain and keeps the
  distribution question explicitly **open**. `deny.toml` deliberately carries no `allow` or
  `exceptions` entry for those crates so `cargo deny` keeps reporting them — do not silence it,
  and do not write a conclusion about that question into the repo.
- **`rusqlite` and `sqlx` cannot be in the same graph, even switched off.** Both declare
  `links = "sqlite3"` through `libsqlite3-sys` — at versions that do not overlap (`rusqlite 0.40`
  needs `0.38`, `sqlx 0.9` needs `>=0.30.1, <0.38`) — and cargo refuses to resolve a graph
  containing two packages linking the same native library, `optional = true` or not. The error
  names `libsqlite3-sys` and says nothing about which of your dependencies wanted it. This rules
  out a "sqlx for the network backends, rusqlite for SQLite" mix unless the versions are pinned to
  a compatible pair; it cost the design round one failed build and is recorded here so it costs
  the next one none.
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
fires — they are written to be read at the moment of need, not up front, so a session that never
touches a module never pays for its internals. This table is the single index; the four
`dodo-*-internals` skills hold what used to be inlined in this file as a per-module wall of text.

| Skill | Load it when |
|---|---|
| `dodo-api-explorer-internals` | Touching anything under `src/api_explorer/` — the send pipeline, scripting/sandbox, consent gating, codegen/curl, collections, or tab/column layout. |
| `dodo-docker-internals` | Touching anything under `src/docker/` — engine discovery, the four list pages, polling, the detail dialog, or a "Coming soon" placeholder. |
| `dodo-database-internals` | Touching anything under `src/database/` — the connection tree, query execution, the `Driver` trait, or result-grid layout. |
| `dodo-build-release-internals` | Touching `src/updater/`, `.github/workflows/`, `Cargo.toml`'s dependencies, `docs/release.md`, `docs/build-optimization.md`, `scripts/generate-icons.py`, `tools/update-manifest/`, `deny.toml`, or `THIRD-PARTY-NOTICES.md`; preparing or debugging a release. |
| `gpui-component-recipes` | Writing or editing any `render` / `new` that builds a gpui-component widget (input, code editor, diagnostics, select, dialog, settings panel, sidebar, button, icon); a widget call will not compile; a widget builds but nothing appears on screen; or a code editor draws uncoloured text. |
| `dodo-tool-view` | Adding, renaming, reordering or removing a sidebar tool; a new sidebar entry does not appear or renders blank. |
| `dodo-i18n-text` | Writing or changing **any** text a user reads — a label, title, placeholder, description, error, dropdown option; or an `i18n` / `i18n_lint` test fails. |
| `dodo-theming-settings` | Adding or changing a setting, adding or removing a theme or a language, or a settings change does not apply until restart. |
| `dodo-build-validate` | First `cargo` invocation of a session, adding tests, a build or `cargo test` failing oddly, or being asked whether a UI change actually works. |

Two things that catch everyone and belong here rather than behind a trigger:

- **`Cargo.lock` is the only pin on the four git dependencies.** `cargo update` silently jumps
  them to upstream HEAD. Never run it as a side effect of another task. An explicit `rev` pin
  cannot replace it — upstream depends on itself through unpinned default-branch refs, and the
  three resulting cargo errors are recorded in `docs/build-optimization.md`. Hence `--locked`
  everywhere, and `cargo update` only ever as its own reviewed commit.
- **The pinned `gpui-component` source is the reference for every widget question**, at
  `~/.cargo/git/checkouts/gpui-component-*/<rev>/crates/ui/src` (rev from `Cargo.lock`). Its
  `<checkout>/skills/` directory holds the upstream authors' own guidance, which is excellent on
  GPUI fundamentals and stale in a few places — `gpui-component-recipes` records which.

## Maintaining this file

Keep this file for knowledge useful to almost every future agent session in this project.
Do not repeat what the codebase already shows; point to the authoritative file or command instead.
If a fact only matters when touching one module, it belongs in that module's skill (table above),
not here — this file is the map, not the territory. Prefer rewriting or pruning existing entries
over appending new ones. When updating this file, preserve this bar for all agents and keep
entries concise.
