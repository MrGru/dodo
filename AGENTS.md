# Project agent memory

`dodo` is a Rust desktop app: a single window with a collapsible sidebar, where each sidebar
entry swaps the main pane to a self-contained developer tool (JSON formatter, Encoder/Decoder,
API Explorer) plus a Settings dialog. It is built on GPUI (Zed's UI framework) and the `gpui-component` widget
library, both pulled from git and pinned only by `Cargo.lock`. See `README.md` for the user-facing
description and `Cargo.toml` for exact dependency sources.

Read `src/main.rs` for the startup sequence and `src/layout.rs` for the view model; the doc
comments there are the authority on structure. This file is only a map.

`src/main.rs` also owns **app lifecycle**, and both halves are counter-intuitive enough to name
here: a release Windows build is a **GUI-subsystem** binary (no console window behind the app),
which costs it valid standard handles, so `attach_parent_console` buys them back on the
`--version` / `--build-info` path alone — and every Windows smoke test must therefore *capture*
the output or the shell will not even wait for the process. Closing the single window quits the
app through **`QuitMode::LastWindowClosed`** (GPUI's own check, run after the window is removed,
not a callback that force-quits) plus a macOS-only `cmd-w` binding, needed because dodo installs
no menu bar for that shortcut to hang off. The doc comments there carry the reasoning;
`docs/release.md` records that the Windows half has never run on a Windows host.

Most tools are a single `src/<tool>.rs`. **`src/api_explorer/` and `src/docker/` are the
exceptions** and the pattern to copy when a tool outgrows one file: `models/` (plain data, no GPUI,
unit tested), `services/` (the trait that is the only place naming the outside-world crate),
`state/`, `components/`, `views/`. Each `mod.rs` doc comments explain the split and where later
phases plug in. `api_explorer` is also the only tool that registers a key binding
(`api_explorer::init`, called from `main` after `gpui_component::init`, same ordering rule as
`settings::init`).

Thirteen things about **`src/api_explorer/`** that no single file makes obvious:

- **The whole send pipeline lives in `services/send.rs`, in this order:**
  `pre-request script → resolve {{name}} → prepare → Transport::execute → post-response script`.
  It is one blocking function over trait objects, which is what makes the *ordering* unit-testable
  with a fake transport and a fake engine and no `Window`. Its module doc records why the script
  runs **before** substitution (the scripting plan said the opposite and was wrong: the shipped
  `pm.variables.set("timestamp", …)` template promises the value resolves in that same request)
  and why `prepare` nonetheless stays **last** (a header or URL a script wrote still goes through
  dodo's own validation). `models/interpolate.rs` owns substitution itself — the escape rule
  (`\{{`), the recursion guard and the decision that an unresolved reference *fails* the request
  rather than being sent literally or blanked; it is pure and exhaustively table-tested.
- **The two hooks fail in opposite directions, deliberately.** A failed *pre-request* script stops
  the send (a half-configured request produces a response nobody can reason about). A failed
  *post-response* script **must not lose the response** — the request already happened, and the
  response is the evidence the user needs to fix the script — so it becomes a Console line plus the
  Tests tab's error banner while `SendOutcome::result` stays `Ok`. Both are argued in
  `services/send.rs`'s module doc and both are tested.
- **Test results attach to `ResponseState`, never to `Exchange`** (`models/test_result.rs`).
  `Exchange` is protocol-neutral and must not learn about scripting; and a *pre-request* script can
  define tests for a request that then never got a response, which `Exchange` could not hold at
  all. History keeps only a `TestSummary` — it is capped by count, not bytes, and a row has space
  for one badge. `Failed` and `Errored` stay distinct because they tell the user whether their API
  or their script is wrong.
- **Sending is one background job — script, `prepare` and the request together.** `prepare` used
  to run on the UI thread; it does not any more, because encoding a body may read a file (a
  multipart file part, a binary body), and a script may loop for its whole 2 s budget.
  `state/tab.rs::send` spawns the lot, so a validation error lands one task hop after the click.
  **No file is ever read on the UI thread**: `services/file_picker.rs` runs the picker and its
  `stat` off-thread, and `services/http/upload.rs` is the only place bytes are read — reached
  solely from `prepare`, with a stated size cap. Do not reintroduce a synchronous `prepare` call
  from a view.
- **`services/script/` is the only module that may name `rquickjs`**, and `quickjs.rs`'s doc is
  the authority on the sandbox: the positive intrinsic allowlist, the 2 s deadline / 16 MiB / 256
  KiB caps, one fresh runtime per run, and why `pm.sendRequest` is denied by *not existing* while
  still failing with a message that names it. Read it before changing anything about what a script
  can reach. One correction it records, because it cost a build to find: **the `Eval` intrinsic
  cannot be left out.** `JS_AddIntrinsicEval` registers no global — it sets `ctx->eval_internal`,
  which `JS_EvalInternal` refuses to run without — so omitting it breaks dodo's own `ctx.eval`,
  not just the script's. `eval` confers no capability the script lacks; what it costs is
  legibility, and any future static API scan has to say so.
- **A script's provenance is a property of the request, not of the text.**
  `RequestSnapshot::script_origin` is set to `Imported` by `services/collection_import.rs` and
  never changed by editing, so an imported script cannot be laundered past the consent gate.
  Editing changes the *content hash*, which re-arms the gate — the two halves of a
  `models/script_consent.rs::ConsentKey`. **One key covers both hooks** (they are hashed together):
  an approval given when only the pre-request hook ran said nothing about a post-response script,
  so honouring it for one would be exactly the laundering the gate exists to stop. That module owns
  the whole policy; approvals persist in a third `data_dir()` file.
- **`pm.test` and `pm.expect` are written in JavaScript, not in Rust bindings** — `PRELUDE` in
  `services/script/quickjs.rs`, handed its two host functions as *arguments* so no `__`-prefixed
  global exists for a script to find. Chai's chain words are self-references and `.not` is a getter
  (a plain property would recurse forever). The matcher set is exactly `report.md` §3.2's and no
  more: an unsupported matcher must fail as a missing function rather than quietly pass.
- **The script editors' syntax check uses the engine that will run the script**
  (`ScriptEngine::check` → `Module::declare`, QuickJS's compile-only flag), debounced in
  `state/tab.rs`. That is the whole point: a second parser could underline code the engine then
  runs happily. Module goal accepts a little more than script goal (top-level `await`, `import`),
  which makes the residual disagreements *false negatives* — the safe direction. The **Format**
  action beside it is deliberately *not* a JavaScript formatter: `models/script_format.rs` only
  re-indents and normalises blank lines, and its doc records why (a real one, `dprint-plugin-
  typescript`, measured at +2.8 MB — 12.7% — on a binary that had already grown for the engine).
  Lines inside a template literal or block comment are emitted byte-for-byte.
- **Code generation and cURL parsing are separate modules on purpose.**
  `services/curl.rs` reads one language; `services/codegen/` writes four (cURL, `fetch`, `axios`,
  `XMLHttpRequest`). They share no code and are shaped nothing alike — a tokenizer plus an option
  table against four pure emitters over a single normalized form. **`services/codegen/normalize.rs`
  is the piece to understand first**: it flattens a `RequestSnapshot` into method / one absolute URL
  / one header list with auth folded in / one body, following `prepare`'s exact deference order and
  reusing `auth::apply`, `effective_pairs` and `request_body::form_escape` rather than re-deriving
  them. Four ad-hoc walks would have given "does the API key ride in the query" four answers. It
  validates nothing and reads no file — both are stated there with why. The two directions are
  joined by a **round-trip property test** in `services/codegen/curl.rs`: generate, hand it to
  `curl::parse`, and require both to normalize to the same `NormalizedRequest`. The equivalence is
  over the *wire request*, not the snapshot (an Auth-tab entry comes back as a header), and the three
  cases where even that cannot hold are named in that module's doc.
- **Generated code withholds a `secret` variable and says so; it withholds nothing else.** A
  reference to a variable marked `secret` is emitted as the literal `{{name}}`, via
  `VariableSet::with_secrets_masked` — which sets the masked value to `\{{name}}` so the substituter's
  *existing* escape rule does the work, covering nesting and making recursion impossible. A token or
  password typed straight into the Auth tab **is** in the copied text, because it has no name to
  stand in for it; the dialog's notice is never absent and says exactly that, and a
  **Resolve secret variables** toggle resolves them behind a danger-coloured warning. The whole
  policy and its reasoning live in `services/codegen/mod.rs`'s module doc — read it before changing
  what reaches the clipboard. What JavaScript cannot express (a form-data FILE row, a binary body)
  becomes an **undeclared identifier** with a comment naming the path, so running the snippet
  unchanged throws rather than silently sending an incomplete request; `services/codegen/
  javascript.rs` argues that against the two alternatives.
- **Pasting a cURL command into the URL box rebuilds the whole request** (`services/curl.rs`,
  which is pure and heavily table-tested). Two guards keep it from firing while somebody types the
  word "curl": `state::request::is_bulk_change` (a paste is not a keystroke) and a parse that must
  yield a URL. A parsed command **opens a new tab and restores the field it was pasted into**,
  reusing the current tab only when it is untouched — so it can never overwrite unsaved work.
- **A tab's auto-derived title is the URL's *path*** (`models/tab_title.rs`, `/api/123`, not the
  host), with the host as the fallback for an empty path and the raw text for anything that does
  not parse. An explicitly named tab still wins. The rules live in that module's doc and its tests;
  `state::request::display_name` only chooses between it and the "Untitled" wording.
- **The request and response columns need `min_w_0`, and it is load-bearing.** A flex item
  defaults to `min-width: auto`, so without it the *widest child of the widest tab* sets the
  column's width and everything else — the Send button first — is pushed off the right of the
  window. It is invisible until some pane grows: the Scripts tab's sandbox notice made it show up
  at 1280px. `render_request_editor` and `render_response_viewer` carry the `min_w_0` with the
  reason inline; put one on any new pane rather than trimming the text that exposed it.

**`src/docker/`** is the Docker/Podman module, and it is **feature-complete as of round 6**: four
list pages (Containers with compose grouping, filters, bulk actions; Images/Volumes/Networks),
background polling with incremental merges, keyboard navigation, row context menus, and a
read-only detail dialog — Inspect for all four resource types plus a container log viewer behind
a second tab. **`src/docker/mod.rs` is the authority** — it documents the layer split, what each
round shipped, and, for the features that are still deliberately disabled "Coming soon"
placeholders (Exec/Terminal, Create/Pull/Build, Stats beyond live CPU%, Favorites), exactly where
each one plugs in. Read it before changing anything here rather than inferring the structure from
the files.

Six things about the module that are not obvious from any one file:

- **Engine discovery is hand-rolled per platform because bollard's is not enough.**
  `services/engine.rs::connect()` carries the numbered order (DOCKER_HOST → `/var/run/docker.sock`
  → macOS `podman machine` → bollard's Podman defaults) and the reasons inline; read it before
  touching connection behaviour. The two traps it encodes: `/var/run/docker.sock` is often a
  *dangling* symlink on a Mac that once ran Docker Desktop, and `connect_with_podman_defaults()`
  only probes Linux paths, so it never finds the per-user
  `$TMPDIR/podman/podman-machine-default-api.sock` a macOS `podman machine` actually listens on.
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
- **The sidebar is flat — every tool, Docker included, is one top-level `SidebarMenuItem` with
  no children.** Docker used to be a nested group, and that made its four pages unreachable:
  an icon-collapsed sidebar renders no children at all. The four pages now live on Docker's own
  vertical tab rail (`DockerView::render_rail`), so `View` has a single `View::Docker` variant and
  `DockerPage` owns the page identity — its `title`/`icon`/`ALL`/`DEFAULT`. Do not reintroduce
  nesting here. `layout.rs::pane_title` is why the main-pane heading still names the page
  ("Containers") while the sidebar row names the tool ("Docker").
- **A modal overlay is a `window.open_dialog`, never a scrim in the page's own tree.** The Docker
  detail surface was the second attempt at hand-rolling one and it could not block clicks: a
  `div().absolute().inset_0()` scrim only covers the *page* (not the rail or the sidebar), and a
  no-op `on_mouse_down` closure registers a listener that swallows nothing — gpui keeps
  dispatching to every hitbox under the cursor unless a listener calls `cx.stop_propagation()` or
  the element sets `HitboxBehavior::BlockMouse` (`.occlude()`). `settings::open` had it right all
  along; `views/detail.rs`'s module doc records the diagnosis, and `docker::init` deliberately
  binds no `escape` because the library `Dialog` owns dismissal. Two costs of following that
  pattern are written down there too: the dialog body must be an **entity** (a dialog layer does
  not repaint on the page's `cx.notify()`), and its width must be **stated** rather than `w_full`
  (a percentage width resolves to `auto` inside the dialog's wrappers and content-sizes the body).

**dodo persists three things across restarts**, all under `~/Library/Application Support/dodo/`
(`data_dir()`) and each behind a trait so the state layer never learns where they live:
`collections.json` (`services::collection_store`), `environments.json`
(`services::variable_store`) and `script-consent.json` (`services::consent_store`, the imported
scripts the user has approved). The `dodo-theming-settings` skill's "nothing is persisted across
restarts" is therefore scoped to appearance/language settings only — including the new
**Run scripts** setting, which is a `ScriptPolicy` global and deliberately starts each launch at
the cautious `Ask for imported`. Persistence and initial load run on the background executor,
never the UI thread.

The files version differently, and the difference is deliberate. A `RequestSnapshot` inside
`collections.json` is versioned only by `#[serde(default)]`, which copes with *added* fields and
nothing else. `environments.json` and `script-consent.json` carry an explicit `"version"` from
their very first write, and their `parse_document` **refuses** a file whose version is higher
rather than half-reading it. Copy that pattern for any new file; do not copy `collections.json`'s.

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

Six things about build and release that catch people:

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
  (34 unformatted files, 12 warnings) is paid off; there is no crate-level `allow`, and the two
  surviving suppressions are `#[allow]`ed at their definition with the reason inline.
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
