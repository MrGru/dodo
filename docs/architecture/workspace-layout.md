# The workspace: which shape a new crate takes

The repo is a cargo workspace. It gained `[workspace]` on 2026-08-08, when the input-method engine
moved to `crates/dodo-ime-core/`, and `cargo metadata --no-deps` at the root now lists fifteen
packages. The root `Cargo.toml`'s header comments are the authority on the rules; this page is why
they say what they say.

## `crates/` or `tools/`: does dodo *link* it?

**A linked crate is a workspace member**, so there is one `Cargo.lock` and one `--locked`. A second
lockfile for a linked crate would be a second, silently divergent resolution of shared dependencies.

**A crate the release workflow merely *runs* stays standalone and excluded.** `tools/update-manifest`
is the only one: `exclude = ["tools/*"]` in `[package]` keeps it out of `cargo package`,
`workspace.exclude` keeps it out of the workspace, it carries its own `Cargo.lock`, and it costs the
binary zero bytes. Do not add it to the workspace, and do not give dodo a `[[bin]]`.

Two mechanical traps follow:

- **`default-members` is load-bearing.** Without a crate listed there, a bare `cargo test` or
  `cargo clippy --all-targets` silently stops covering it. **A crate goes into `members` and
  `default-members` in the same commit.**
- **`workspace.exclude` matches paths, not globs.** `tools/*` there excludes nothing, which is why
  it is spelled out.

## Kernel crates: used by everything, dependent on nothing

`dodo-i18n` (every user-visible string), `dodo-app-icon` (the icon set), `dodo-paths` (where dodo's
files live) and `dodo-dialog-slot` (one dialog at a time) came out of `src/` on 2026-08-15. Dozens
of modules use each and none of them needs the rest of dodo. Four rules govern them:

- **The graph stays wide and shallow.** They do not depend on each other, because a chain of small
  crates compiles more slowly than one big one and would defeat the point.
- **Purity is the deliverable, not the file count.** `dodo-i18n` exists so a pure model can hold a
  translated message without a windowing library, which is why its `Cargo.toml` has an empty
  `[dependencies]` and a comment saying so.
- **What cannot be pure stays in the binary.** The one read of `build_info::VERSION_INFO.target`
  lives in `main.rs`'s `paths` module, so no kernel crate needs a build script.
- **A gpui `Global` is identified by its type, so there can be exactly one of it**, and it has to
  sit where the binary and every feature crate can both name it. That is the whole argument for two
  of these four: the active-language global and `t()` moved into `dodo-i18n` behind an opt-in `gpui`
  feature the moment `dodo-cleaner` had to render a `Str` from outside the binary, and
  `src/dialog_slot.rs` had to become `crates/dodo-dialog-slot` the moment the updater left — a copy
  on each side of a crate boundary is *two* slots, and the duplicate-dialog defect it exists to
  prevent would have come straight back. `dodo-dialog-slot` is the one where the rule about *use*
  did not decide it; three users is still not many.

Two files were measured and deliberately **not** extracted: `src/assets.rs` embeds `./assets` by a
path relative to its crate root, and `src/build_info.rs` reads the `env!` variables `build.rs` sets.
That is also why the updater's `init` **takes** the two fields it needs rather than any crate growing
a build script to re-derive them.

## Feature crates: one whole tool, lifted out in one piece

A kernel crate is code many tools share; a feature crate is one tool of the app. There are nine.
Eight were extracted from the binary during 2026-08-15, and `crates/dodo-cleaner` is the worked
example — 93 files, 25,646 lines, the largest feature dodo has. The ninth, `crates/dodo-flow`, is
the first that was **born** a crate: by 2026-08-16 the eight above were themselves the architectural
reason to be one, so the seam test below was applied to a design rather than to existing code.

The larger ones share one internal shape: `models/` (plain data, no GPUI, unit tested), `services/`
(**the only layer that names outside-world crates**), `state/`, `components/` and `views/`. A new
feature crate should land in that shape unless it is small enough to be one file, as the JSON
formatter and Encoder/Decoder are.

**The seam is what qualifies a feature, not its size.** Before extracting, measure outbound
`crate::…` edges and binary consumers; extract only when every outbound edge is already a crate and
the inbound surface is small. The Cleaner's outbound edges were exactly `app_icon`, `i18n` and
`paths`; the JSON formatter and Encoder/Decoder each had only `i18n`. A `use crate::…` inside a doc
comment is not an edge.

The conventions:

- **Both sides keep their spelling.** `main.rs` does `use dodo_cleaner as cleaner;` and the crate
  does `use dodo_app_icon as app_icon;` / `use dodo_i18n as i18n;`, so every existing path is
  unchanged. Rewriting `crate::cleaner::` to `crate::` was the entire source change for the Cleaner;
  91 of its 93 files are byte-identical to their `src/` versions once rustfmt has re-wrapped the
  shortened `use` lines.
- **`pub mod` becomes `pub(crate) mod`, and that is not a narrowing** — inside a binary, `pub`
  already meant "dodo and nowhere else". Leave the modules `pub` and the crate suddenly exports its
  internals, which is also what makes `clippy::new_without_default` start firing on constructors
  that were never public before. But **the rule is what the binary names, not a uniform sweep**:
  `dodo-database` keeps `models` public because `quick_nav` reads `models::uri`, and
  `dodo-api-explorer` keeps both `models` and `services` public for the same reason.
- **What was impure in the binary needs a seam in the crate.** `paths::current()` reads a target
  triple a library is not handed, so each crate names the platform with `cfg!` instead — and
  `main.rs` carries the test asserting the two spellings are one answer. The seam only needs what
  the crate actually asks: nothing in `dodo-docker` writes a file, so its `paths` exposes
  `current()` and no `data_dir()`.
- **The pure/UI split is a contract.** 90 of the Cleaner's 93 files name no UI framework; only the
  three under `views/` may `use gpui`. Each crate's `Cargo.toml` says why every dependency is there.
- **A feature crate takes its outside-world dependencies with it.** `bollard` and `tokio` left the
  root manifest with Docker (dodo the binary now names no async runtime at all); `rquickjs` left
  with the API Explorer; the four database drivers, `sqlformat` and two rustls lines left with the
  Database Explorer; `percent-encoding` with the Encoder/Decoder; and **`reqwest` left with the
  updater**, so dodo names no HTTP client either.
- **`src/i18n_lint.rs` reaches across** with `include_str!("../crates/<crate>/src/views/…")`, so a
  moved view is still scanned for untranslated literals from the binary's tests.
- **A feature crate earns a launcher, and the launcher is an `examples/` target.** Seven of the nine
  have one — the four stateful tools mount the real view against the real machine and the real
  `data_dir()`; the JSON formatter and Encoder/Decoder are stateless, and the updater and input
  method have none, one being a dialog over the app and the other reading a file another process
  owns. `examples/` rather than `[[bin]]` so nothing a shipped build compiles can reach it, and
  every launcher-only dependency is a `[dev-dependency]`, so `cargo tree -p dodo --edges
  normal,build` is byte-identical before and after. The launcher invents nothing: no arguments, no
  fixtures, no second copy of a setting. Two pieces of `src/app.rs` are genuinely required and are
  the whole file — `Root` plus `Root::render_dialog_layer` under it, or the crate's dialogs open in
  state and never paint, and an asset source, or every `icons/<name>.svg` resolves to nothing. Keep
  it to a screenful. README's "Running one feature on its own" has the exact commands.

### The nine, and what each one proved

| Crate | Size | Inbound surface | What left the root manifest |
|---|---|---|---|
| `dodo-cleaner` | 93 files, 25,646 lines | `CleanerView`, `paths` | — |
| `dodo-docker` | 43 files | `DockerPage`, `DockerView`, `init`, `paths` | `bollard`, `tokio` (`futures-util` stayed: the tray and input method also await a stream) |
| `dodo-database` | 52 files | `DatabaseView`, `init`, `models::uri` | the four drivers, `sqlformat`, two rustls lines |
| `dodo-api-explorer` | 77 files | `ApiExplorer`, `init`, `ScriptPolicy`, `ConsentPolicy`, `RequestSnapshot`, `services::curl` | `rquickjs` |
| `dodo-updater` | — | `init`, the dialog | `reqwest` |
| `dodo-input-method` | — | `InputMethod`, `init`, `load`, `views::InputMethodView` | — |
| `dodo-json-formatter` | 1 file | the tool table | — |
| `dodo-encoder-decoder` | 1 file | the tool table, `Format` | `percent-encoding` (`base64` stayed: quick navigation's detector uses it) |
| `dodo-flow` | in progress | none yet — the tool table row lands once the canvas is usable | — (born a crate) |

Three of them are worth a sentence beyond the table. **`dodo-database` is where the "what the binary
names" rule was decided** rather than a uniform `pub(crate)` sweep. **`dodo-api-explorer` has the
widest inbound surface of them**, which is what leaves both `models` and `services` public, and
extracting it took the binary's own test count from 976 to 477. And **`dodo-updater` is the one that
shows what to do when a feature needs something only a binary is given**: `init` *takes*
`VERSION_INFO.version` and `.target`, its own `build_info` module falls back to `CARGO_PKG_VERSION`
and `cfg!` under `cargo test`, and the three assertions that were really about the *binary* moved to
`main.rs` beside a guard that the two spellings classify to one `PlatformKey`.

### The two edges the Cleaner's shape did not cover

`crates/dodo-input-method` is the only feature whose edges were not all outbound, and its two moves
are the ones to copy before reaching for anything cleverer:

- **An inbound edge becomes a handed-in `fn`.** `src/tray` reads the input method *and* the input
  method told the tray — a cycle no crate boundary can hold. The *notification* inverted:
  `observe_languages` takes a plain `fn` pointer, `main.rs` hands over `tray::set_active_languages`,
  and a platform with no tray registers nothing and is called back never.
- **An unreachable constant becomes a mirrored one with a test.** The pane's test names
  `quick_nav::{KEY_CONTEXT, NORMAL_MODE}`, which a crate cannot read, so the crate mirrors both as
  `pub const`s and `src/quick_nav`'s tests assert the two spellings stay one answer. A drift there
  would not fail to compile.

### Platform gates survive the move, and the move is when to check them

Value choices use `cfg!` so every answer type-checks from a Mac — the modifier glyphs and the tool's
host availability are the visible examples. Platform API calls and platform-only types still need
attributes. **A gate that picks a value becomes `cfg!`; a gate in front of a platform API stays an
attribute.**

## Extracting a crate does not make the build faster

This was measured, twice, with interleaved A/B rounds, and the answer is no: the clean build moved
−1.2%, touching a leaf file *inside* the extracted crate got **7.9% slower**, and touching
`src/layout.rs` got 5.3% faster — deltas of a couple of tenths of a second, pointing in opposite
directions, against a noise floor that machine load alone moved by 32%.

`cargo build --timings` says why, and it generalises: removing 25,646 lines from the binary did not
change the binary's own rebuild time by a measurable amount, because that time is dependency
fingerprinting, final codegen and linking ~800 rlibs — none of which a crate boundary removes — and
the extracted crate's compile is then *added* on top. rustc was already recompiling only the codegen
units a one-line edit touched.

**So extract a feature for boundaries, testability and a public surface you can see — never for
build speed**, and do not re-measure hoping for a different answer without changing what dominates
(the link). "Splitting the binary into crates" in `docs/build-optimization.md` is the authority: the
method, both tables, the per-unit breakdown, and why a non-interleaved A/B of build times lies.
