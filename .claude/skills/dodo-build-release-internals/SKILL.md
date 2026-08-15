---
name: dodo-build-release-internals
description: How dodo is built, packaged, licensed and released - the in-app updater's check/ask/download/verify/install/restart pipeline and its macOS two-rename swap, the application icon pipeline, the update.json manifest and its create-or-update publish step, why two of four cargo check targets can't run natively from a Mac, why fmt/clippy are blocking with no crate-level allow, why no --release build runs on push, the open GPL-3.0 distribution question, and the rusqlite/sqlx conflict. Load when touching crates/dodo-updater/src/, .github/workflows/, Cargo.toml's dependencies, docs/release.md, docs/build-optimization.md, scripts/generate-icons.py, tools/update-manifest/, deny.toml, or THIRD-PARTY-NOTICES.md, or when preparing or debugging a release.
---

**Build and release engineering lives in `docs/`**, and those two files are the authority for it:
`docs/build-optimization.md` (release profile, the measured before/after size table, linker
findings, the dependency report, startup review) and `docs/release.md` (CI, the release workflow,
packaging, verification, the application icon, the in-app updater, future signing/notarisation).
The rest is `Cargo.toml`'s `[profile.*]` comments, `build.rs`, `scripts/` and `.github/`.

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
a tag that already exists and tags here are immutable. `crates/dodo-updater/src/` is what reads it.

## CI, licensing and the dependency graph

Six more things about build and release that catch people:

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
  The original `build (windows-x64)` failure was a `#[cfg(unix)]`-only bollard connector;
  the platform split in `docker/services/engine.rs` fixed it, and release run `31655518790`
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

(The `Cargo.lock`-as-only-pin rule that used to be repeated here lives once, in `CLAUDE.md`'s
"Skills" section — it is a global rule, not a build/release-specific one.)
