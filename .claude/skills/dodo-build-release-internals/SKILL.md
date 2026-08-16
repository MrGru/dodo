---
name: dodo-build-release-internals
description: How dodo is built, packaged, licensed and released - the in-app updater's check/ask/download/verify/install/restart pipeline and its macOS two-rename swap, the application icon pipeline, the update.json manifest and its create-or-update publish step, why two of four cargo check targets can't run natively from a Mac, why fmt/clippy are blocking with no crate-level allow, why no --release build runs on push, the open GPL-3.0 distribution question, and the rusqlite/sqlx conflict. Load when touching crates/dodo-updater/src/, .github/workflows/, Cargo.toml's dependencies, docs/release.md, docs/build-optimization.md, scripts/generate-icons.py, tools/update-manifest/, deny.toml, or THIRD-PARTY-NOTICES.md, or when preparing or debugging a release.
---

**Build and release engineering lives in `docs/`**, and those three files are the authority for
it: `docs/build-optimization.md` (release profile, the measured before/after size table, linker
findings, the dependency report, startup review, and the measured verdict on splitting the binary
into crates), `docs/release.md` (CI, the release workflow, packaging, verification, the application
icon, the in-app updater) and `docs/macos-signing.md` (the authority on signing and notarisation:
what the repo owner had to buy or create, the secrets by exact name, the entitlements — dodo needs
none, and neither does the input-method bundle — the ordering, and §8's list of what is still owed).
The rest is `Cargo.toml`'s `[profile.*]` comments, `build.rs`, `scripts/` and `.github/`.

**macOS signing and notarisation are implemented; Windows and Linux are still unsigned.** The six
`MACOS_*` secrets exist on `MrGru/dodo` and the plumbing is in the tree — but **no release run has
exercised it**, so treat every claim about the signed path as unverified until one does. Three
things that would have cost a release each: signing happens **inside** `scripts/package.sh` /
`macos-app-bundle.sh`, before the tar, because the published SHA-256 and `update.json` entry are
computed from that archive; a workflow `if:` **cannot read `secrets`**, it reads an `env:` set from
one at job level; and `codesign --deep` is deprecated for *signing* while remaining correct for
verifying, so nested bundles are signed inside-out, one call each. Two consequences worth knowing
before touching either script: with no secrets everything ad-hoc signs exactly as before (that
path must stay working — it is what a fork gets), and a signed `-app.tar.gz` is **no longer
byte-reproducible** across runs of the same tag, by design.

## The in-app updater

**`crates/dodo-updater/src/`** is the in-app updater — check, ask, download, verify, install, restart — and
the consumer of the `update.json` the release publishes. Same five-layer split as `api_explorer`/
`docker`/`database`; **`crates/dodo-updater/src/lib.rs` is the authority** on the structure and "The in-app
updater" in `docs/release.md` on the behaviour, what was proved against the live release and what
was not. Six things worth knowing before touching it:

- **Check silently, ask before downloading**, and the second half is *structural*:
  `services::pipeline::check` is handed no `Downloader`, so it cannot fetch an archive however
  it is called. Only a button reaches `download_and_install`.
- **`services/installers/` carries `#[cfg(target_os)]` in exactly one place** — the
  `platform_installer()` factory, mirroring `docker::services::default_engine`. All three
  installers therefore compile *and are tested* on every host, which is deliberate insurance
  against the `#[cfg(unix)]`-only failure that broke `build (windows-x64)`. Nothing in them is a
  platform API. Do not add a `cfg` anywhere else in the module.
- **The macOS swap is two renames with a rollback, not one atomic operation.** `swap.rs`'s doc
  says so and says why `renamex_np(RENAME_SWAP)` is not used (a direct `libc` dependency for one
  call on one platform). Do not upgrade the word to "atomic" without upgrading the code.
- **Refusing to install is a success**, surfaced as "downloaded, install manually" with the
  archive's path — a bare binary, an unwritable `/Applications`, a read-only volume. Only a
  broken archive or a half-failed rename is an `Err`.
- **Verification is integrity, not authenticity.** The digest comes from the same HTTPS origin as
  the archive; `signature` is read, always `null`, and verified by nothing. A mismatch discards
  the file, and **a downloaded file is never executed** — extraction runs the system's `tar` with
  the archive as input.
- **`models/sha256.rs` and `models/version.rs` are hand-written rather than `sha2` and `semver`**,
  and both module docs argue it: `sha2` is only a *build* dependency today, so taking it would be
  a genuinely new runtime dependency and a `Cargo.lock` edit. Both are exhaustively table-tested
  (NIST FIPS 180-4 vectors; SemVer §11 precedence).

## The application icon

**The application icon is a committed pipeline, not a file someone dropped in.** `assets/branding/`
holds the original artwork and the 1024 RGBA master; `python3 scripts/generate-icons.py` derives
the macOS `.icns`, the Windows `.ico`, the Linux hicolor PNGs and one 256px PNG from it, and all of
those are committed because packaging must not depend on the host (`iconutil` is macOS-only). Read
"Application icon" in `docs/release.md` before touching any of it — it is the authority. Five
things it records that are not obvious from the files:

- **The question is answered in three unrelated places.** `build.rs` compiles the `.ico` into
  `dodo.exe` as an `RT_GROUP_ICON`; `scripts/macos-app-bundle.sh` puts the `.icns` in the bundle;
  `src/window_icon.rs` covers at runtime what no file can — a bare macOS binary's Dock tile
  (`-[NSApplication setApplicationIconImage:]`, since GPUI exposes no dock-icon API) and the Linux
  `app_id`. Changing one of the three fixes one launch path.
- **`WindowOptions::app_id` was the whole Linux bug.** GPUI leaves it `None`, and with no app id a
  Wayland toplevel is unmatchable against `dodo.desktop` *and* an X11 window carries no `WM_CLASS`
  at all — so `StartupWMClass=dodo` matched nothing either. `window_icon::APP_ID`, the desktop
  file's name, its `StartupWMClass` and its `Icon=` must all stay equal; a unit test enforces it.
- **`WindowOptions::icon` is X11-only and the pinned GPUI means it.** Wayland has no equivalent
  there, so a bare Linux binary under Wayland is not fixable from inside dodo.
- **The `winresource` build-dependency is target-scoped, and cargo resolves that against the
  *target*** (rust-lang/cargo#4932) while `#[cfg(windows)]` inside `build.rs` is the *host*. They
  agree only on a native Windows build. `embed_windows_icon` documents both mismatches.
- **"Verified, not assumed" speaks for the macOS bundle alone.** That is the only one of the five
  launch paths anyone has looked at. Do not read it as a verdict on the icon generally — doing so
  is how four broken cases survived.

**Do not confuse `assets/{branding,macos,windows,linux}` with `assets/icons`**: only
`icons/**/*.svg` and `themes/**/*.json` are embedded in the binary through `rust-embed` (the
`#[include]` filters in `src/assets.rs`), which is why the packaged icon artwork costs zero bytes.
The one exception is `assets/branding/dodo-256.png`, which `src/window_icon.rs` pulls in with
`include_bytes!` — a different mechanism the filters do not govern. Anything new under `assets/`
that must stay out of the binary has to stay outside those two filters *and* out of an
`include_bytes!`.

## The update manifest

**Every release publishes an `update.json` manifest**, generated by
`tools/update-manifest` — a **standalone crate that is not part of dodo**. `exclude = ["tools/*"]`
in `[package]` keeps it out of `cargo package` and `workspace.exclude` keeps it out of the
workspace (`docs/architecture/workspace-layout.md` owns the rule for which shape a new crate takes,
and the trap in spelling that exclusion).
It carries its own `Cargo.lock` and four dependencies, and it is built only by the release workflow
through `--manifest-path`. It costs the binary zero bytes. Do not add it to the workspace, and do
not give dodo a `[[bin]]`. "Automatic updates" in `docs/release.md` is the
authority: the manifest shape and why `manifest_version` / `signature` / `channel` exist, the
hand-verification recipe, and the channel design. Three things that are
decisions rather than details: the manifest points at macOS's **`-app.tar.gz` bundle** selected by
exact filename (an installer swaps the `.app`); **any missing platform fails the release**,
experimental ones included, because a silently absent platform means those users are never offered
an update; and the publish step is **create-or-update**, because `gh release create` cannot repair
a tag that already exists and tags here are immutable. `crates/dodo-updater/src/` is what reads it.

## CI, licensing and the dependency graph

Seven more things about build and release that catch people:

- **Two of the four `cargo check` targets cannot be run from this Mac at all.**
  Linux and Windows both die in `aws-lc-sys`'s C build script (no cross C toolchain, no
  `windows.h`) — not a portability problem in dodo, and not fixable by a cargo flag. The two Apple
  targets do cross-check locally. "The `check` row runs natively" in `docs/release.md` has the
  detail, including the two traps that cost time: Homebrew's `rustc` shadows rustup's and ships
  only the host std (`rustup run` does **not** fix it — use the toolchain's absolute path), and a
  cross-check needs its own `CARGO_TARGET_DIR` or it invalidates the warm cache a size
  measurement depends on. Two sharper corollaries, each of which has cost a session: the
  toolchain's absolute **`cargo`** is not enough on its own, because cargo resolves `rustc` from
  `PATH` — set `RUSTC=<toolchain>/bin/rustc` too, or the cross build fails with "can't find crate
  for `core`" — and never let two toolchains share `target/`, which poisons it with `E0514`
  ("compiled by an incompatible version of rustc") until the affected packages are
  `cargo clean -p`'d.
  **The `crates/dodo-ime-*` crates *do* cross-check for Windows from here**
  (`cargo check --locked --target x86_64-pc-windows-msvc -p dodo-ime-core -p dodo-ime-ipc -p
  dodo-ime-windows --all-targets`) because they link no TLS; only the `dodo` crate itself is
  unreachable. So a Windows-only mistake in **`src/`** — the classic being a
  `#[cfg(target_os = "macos")]` item called from an `any(macos, windows)` block, which is exactly
  how `InputMethod::refresh_status` broke the Windows build — cannot be caught locally at all, and
  a change to a `cfg`-shaped part of `crates/dodo-input-method/`, `src/tray/` or `src/layout.rs` is
  worth auditing by hand before it reaches the captain's Windows machine.
- **CI compiles all four targets, and the non-Mac rows are `experimental: true`**, so their
  failures are `continue-on-error` and do not block a merge — which is how three platform breakages
  reached `main` on 2026-08-15 alone: a platform arm returning the wrong type (`reveal_label`), the
  same in a module the first sweep missed (`input_method_view`), and an **ungated `use` of a
  `#[cfg]`-gated item** (`settings::general::start_with_os_field`, imported by `settings/pages.rs`
  without the gate its declaration and its use site both carry — a split artifact, since before
  `settings.rs` became a folder there was no import to miss it). Flipping a row to
  `experimental: false` is the captain's call and the only *sound* fix; `ci.yml`'s own comment says
  so.

  **Do not reach for a source-level lint for the import shape instead.** It was built and measured:
  the repo has 115 platform-gated item declarations, only 15 of which are imported at all (137
  import sites, 117 of them from inside the gated module itself), so deciding a site is wrong needs
  a module tree, `use`-path resolution, gates inherited from ancestor modules *and* enclosing gated
  `fn` bodies, and cfg-implication over a finite world — four layers, each of which had to be added
  to kill a false-positive class. Even then it cannot follow the two paths that matter most:
  `pub use` re-exports and macro-generated references. `src/tools.rs` reaches
  `views::InputMethodView` through **both**, so such a lint would stay green over dodo's only
  platform-conditional tool and its green would mean nothing.
- **`fmt` and `clippy` are blocking jobs; keep them green.** Run `cargo fmt --all` and
  `cargo clippy --all-targets --locked -- -D warnings` before committing. The pre-existing debt
  (34 unformatted files, 12 warnings) is paid off, and **there is no crate-level `allow`** — every
  suppression is `#[allow]`ed at the item it applies to (or, where a whole module is the pending
  unit, as an inner attribute under that module's `//!` docs) with the reason and the condition for
  removing it written next to it. Copy that shape; never widen an `allow` to quieten a lint.
  Dead-code warnings in a module under construction are **scaffolding, not defects**: annotate, do
  not delete. `.githooks/pre-push` runs `fmt`, `clippy` and `cargo test --locked` and refuses the
  push if any fails; it is opt-in per clone with `git config core.hooksPath .githooks` (see
  "Pre-push checks" in `README.md` for its cost and the `--no-verify` bypass).
  Note that `cargo build` alone does **not** prove the tree is green — `dodo-i18n`'s per-area sample
  tables are exhaustive over each area's `Text`, so new strings break `cargo test` while the app
  still builds.
  The original `build (windows-x64)` failure was a `#[cfg(unix)]`-only bollard connector;
  the platform split in `crates/dodo-docker/src/services/engine.rs` fixed it, and release run `31655518790`
  later built and smoke-tested both Windows x64 and macOS x64. Those rows remain
  `experimental` and non-blocking in ordinary CI; the release publish gate still requires every
  platform. See the honesty note atop `.github/workflows/ci.yml` for what has actually run.
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
- **`dodo --version` / `--build-info`** print what `build.rs` embedded and exit before any window
  opens (`print_build_metadata_and_exit` in `src/main.rs`). That path is how CI proves a packaged
  binary runs at all — a GUI app cannot open a window on a headless runner — so keep it free of
  GPUI initialisation.

(The `Cargo.lock`-as-only-pin rule that used to be repeated here lives once, in the root
`AGENTS.md`'s "Invariants" section — it is a global rule, not a build/release-specific one. The CI,
signing, workspace-exclusion and `fmt`/`clippy` material above was folded in from that file on
2026-08-15, when it stopped carrying per-area narrative.)
