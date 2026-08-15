# Releasing dodo

How dodo is built, packaged, verified and published — and what each piece of
that is actually worth.

> **Read this first.** CI and release workflows now have real runs behind
> them. Release run `31655518790` produced and verified macOS arm64/x64, Linux
> x64 and Windows x64 artifacts and published v0.1.12. That proves native
> compilation, archive layout and the headless `--build-info` path; it does not
> prove GUI behaviour, Windows TSF registration/typing, or an in-app update on
> Windows. Experimental matrix flags remain evidence labels, not permission for
> a release to omit a platform: the publish gate requires all four.
>
> Those job names are from the old CI shape. `ci.yml`'s release matrix has since
> become a `check` matrix plus a single debug build, and the release-profile
> matrix moved to `release-profile.yml`; neither of those has run yet either.

---

## CI architecture

`.github/workflows/ci.yml`, on every push to `main` and every pull request.

| Job | Runner | What it establishes | Blocking |
|---|---|---|---|
| `fmt` | ubuntu | `cargo fmt --all --check` | Yes |
| `clippy` | macos-14 | `cargo clippy --all-targets -- -D warnings` | Yes |
| `test` | macos-14 | `cargo test --all-features`, plus `cargo check --no-default-features` | Yes |
| `check` | 4 platforms | `cargo check --locked --all-features --target <triple>`, natively | macOS arm64 only |
| `build-debug` | macos-14 | `cargo build --locked` and a `--build-info` smoke test | Yes |

**The `check` row runs natively, and it cannot be reproduced locally on macOS
for two of its four targets.** `cargo check --target x86_64-unknown-linux-gnu`
and `--target x86_64-pc-windows-msvc` from a Mac both die in the **build script
of `aws-lc-sys`** — a C library reached transitively through
`reqwest -> rustls -> rustls-platform-verifier -> aws-lc-rs`. Linux wants an
`x86_64-linux-gnu-gcc` that is not there; Windows wants `windows.h`. Neither is
a portability problem in dodo, and neither can be fixed by a cargo flag: the
crate needs a cross **C** toolchain, which is why CI compiles each target on its
own runner instead. The two Apple targets *do* cross-check locally
(`aarch64-apple-darwin` and `x86_64-apple-darwin`), and between them they cover
the arch-width differences that catch Rust code.

Two more traps if you try it anyway, both cost real time to find:

- Use rustup's toolchain **by absolute path**. If `rustc` on `PATH` is
  Homebrew's, it ships only the host std, so every target fails with
  `can't find crate for core` — and `rustup run stable …` does *not* fix it,
  because it prepends to a `PATH` that already has Homebrew first. Run
  `~/.rustup/toolchains/<toolchain>/bin/cargo` with `RUSTC` set to its sibling.
- Give it its own `CARGO_TARGET_DIR`. A different rustc version invalidates
  every fingerprint in `target/`, which throws away the warm cache that a
  release-size measurement needs.

**No `--release` build runs on a push.** That is a change from how this
repository started, and the reasoning is worth keeping:

- The four-platform `cargo build --release --locked` matrix was the most
  expensive thing this project did — fat LTO plus `codegen-units = 1` over
  ~800 crates, on the highest-billing runners — and it ran on every push.
- The signal it produced is almost never profile-dependent. The one real
  cross-platform failure so far (run 30106478178, `build (windows-x64)`) was a
  plain missing-function type error: `cargo check` catches that identically.
- So the per-push path is now `cargo check` per platform, which is what finds
  portability breaks, plus one debug build on macOS arm64, which is what proves
  the crate still links and the binary still runs.

**The accepted cost.** A failure that exists only under the release profile —
an LTO miscompile or ICE, `codegen-units = 1` exposing a bad `unsafe`, a linker
flag only that profile passes, `strip` misbehaving on one platform, code behind
`debug_assertions` — is **not** caught on the push that introduces it. It
surfaces later: see "Release-profile builds" below. Before tagging, run one on
purpose rather than discovering it mid-release.

Three further deliberate choices:

- **`fmt` and `clippy` are blocking.** They shipped advisory because 34 files
  predated `.rustfmt.toml` and 12 clippy warnings predated the workflow, all in
  `crates/dodo-docker/` and `src/encoder_decoder.rs`. Both debts were paid off in their
  own commits (`style: apply cargo fmt --all`, `fix(lint): clear the 12
  outstanding clippy warnings`) and `continue-on-error` was removed from both
  jobs. Two clippy lints remain deliberately suppressed —
  `enum_variant_names` on `GroupStatus` and `too_many_arguments` on
  `DetailPanel::open_inspect` — each `#[allow]`ed at its definition with the
  reason written beside it. There is no crate-level allow, so a new warning
  fails the build; keep it that way rather than re-adding `continue-on-error`.
- **`clippy` and `test` run on macOS, not ubuntu.** Both have to compile the
  full dependency graph, and macOS is the only platform dodo is known to build
  on. A lint job that fails because a system library is missing tells you
  nothing about the lint.
- **Non-macOS checks do not block a merge.** They are there to *find out*
  whether dodo compiles on Linux and Windows. Until one of them has passed, a
  failure is information, not a regression. The `check` matrix carries the same
  `experimental` flags the old `build` matrix did, unchanged: a `cargo check` is
  strictly weaker than the release build those runs performed, so no evidence
  was gained or lost when the job changed shape.
- **Each `check` row runs natively.** Cross-checking from macOS is not
  equivalent and does not work here anyway — see "Checking a Windows fix without
  a Windows machine" below, where `aws-lc-sys` needs the Windows SDK headers.
  On a real `windows-latest`/`windows-2022` runner those headers are present
  and a native `cargo check` is fine. Linux still needs
  `.github/actions/linux-build-deps` (build scripts compile C and call
  pkg-config even when nothing is linked), but it is invoked with `lld: "false"`
  there, since `cargo check` never runs a linker.

### Release-profile builds

`.github/workflows/release-profile.yml` is where the four-platform
`cargo build --release --locked` matrix went. It runs:

| Event | What happens |
|---|---|
| `schedule`, Mondays 07:00 UTC | full matrix + `--build-info` smoke test, result written to the run summary |
| `workflow_dispatch` | the same, on demand — do this before tagging |
| a `v*` tag | **not this workflow.** `release.yml` already builds the same four targets with the same profile and then packages and verifies them, which is strictly stronger; running both would pay twice for the weaker answer |

It is a separate file rather than event conditions inside `ci.yml` because the
three things a workflow fixes — its trigger set, its concurrency group and its
cancellation policy — all differ from CI's. It must never be cancelled by a
newer commit (a scheduled run exists to finish and report), it is not attached
to a pull request, and putting `schedule:` in `ci.yml` would make the file whose
job list answers "what happens when I push" answer several other questions too.

Weekly bounds how long a release-only breakage can sit undetected at seven days.
That is the number the trade-off above was priced at; change the cron and you
change the trade-off.

`.github/workflows/analysis.yml` is separate and entirely advisory:
`cargo-audit`, `cargo-deny`, `cargo tree -d` and `cargo-bloat`, on a weekly
schedule plus any PR that touches the dependency graph. Two of those depend on
an advisory database that changes without anyone touching this repository, so
they must never gate a merge — a green PR would turn red overnight for a reason
its author cannot fix.

---

## Release architecture

`.github/workflows/release.yml`, on a `v*` tag.

```
tag v0.1.0 pushed
        │
        ▼
   ┌─────────┐   validates the tag against Cargo.toml's version and
   │  meta   │   pins SOURCE_DATE_EPOCH to the tagged commit's timestamp
   └────┬────┘
        │
        ▼
   ┌─────────────────────────────────────────────┐
   │  build (matrix: macos arm64/x64, linux x64, │
   │         windows x64)                        │
   │                                             │
   │  cargo build --release --locked             │
   │        ↓                                    │
   │  scripts/package.sh | scripts/package.ps1   │
   │        ↓                                    │
   │  scripts/verify-release.sh                  │
   │        ↓                                    │
   │  upload-artifact                            │
   └────┬────────────────────────────────────────┘
        │
        ▼
   ┌─────────┐   downloads every artifact, writes SHA256SUMS,
   │ publish │   creates the GitHub Release with `gh release create`
   └─────────┘
```

The `meta` job exists so that three things are decided exactly once: the
version string, whether this is a pre-release, and `SOURCE_DATE_EPOCH`. A
matrix job computing any of those itself would eventually disagree with its
siblings.

### What "verified" means

`scripts/verify-release.sh` runs against every archive on the runner that built
it, and checks, in order:

1. the `.sha256` sidecar matches the archive;
2. the archive unpacks, and its contents are printed into the log;
3. the binary is present and kept its executable bit through packaging, and
   `LICENSE` and `THIRD-PARTY-NOTICES.md` are inside the archive and non-empty;
4. the binary **runs** — `dodo --build-info` exits 0;
5. the embedded metadata is real: version equals the tag, the commit is not
   `unknown` and does not end in `-dirty`, `build_time` and `target` are set;
6. macOS app archives contain the executable input method at
   `dodo.app/Contents/Helpers/Dodo Vietnamese.app/Contents/MacOS/DodoVietnamese`,
   with a valid plist and nested/outer signatures on macOS; Windows ZIPs contain
   `input-method/dodo_ime_windows.dll` at that exact archive-relative path;
7. archive and binary sizes are reported.

**What step 4 does and does not prove.** dodo is a GUI application and a CI
runner has no display, so the window cannot be opened there. `--version` /
`--build-info` return before any GPUI or window code runs (see
`print_build_metadata_and_exit` in `src/main.rs`). Executing that path proves
the file is a valid executable for its platform, that its dynamic libraries
resolve, and that `build.rs` embedded the right metadata. **It does not prove
the UI renders.** That check is manual: download the archive on a real desktop
and open it. Do that before announcing a release.

**On Windows that step is load-bearing in a second way.** A release build is a
GUI-subsystem binary — `#![cfg_attr(not(debug_assertions), windows_subsystem =
"windows")]` in `src/main.rs`, so launching dodo shows the app window and no
console window behind it. The price is that such a process starts with **no
valid standard handles** unless its parent handed it some, which would send
`--version` / `--build-info` nowhere; `attach_parent_console` in the same file
buys them back, on the CLI path only, and its doc comment is the authority on
why.

**The consequence for CI, and the one that cost v0.1.5 its Windows archive: a
shell does not wait for a GUI-subsystem process at all.** `&` — PowerShell's
call operator — returns as soon as the process starts. PowerShell then tears
down the pipe it was capturing into, and the child dies writing to it:

```
thread 'main' panicked at library/std/src/io/stdio.rs:
failed printing to stdout: The pipe is being closed. (os error 232)
```

`$info` is left empty and the next line throws on a null. That is what happened
in run `30539731759` (attempt 1, job `90861436501`): the *build* and *packaging*
succeeded, the **verify** step failed, its upload step was therefore skipped, and
the release published with no Windows archive at all.

Capturing into a variable does **not** fix this — an earlier version of this
document and of the workflow comments claimed it did, and both were wrong.
`release.yml` was already capturing when it failed. The fix is to use a form
that waits regardless of subsystem:

```powershell
$proc = Start-Process -FilePath $exe -ArgumentList '--build-info' `
  -RedirectStandardOutput $infoFile -NoNewWindow -Wait -PassThru
if ($proc.ExitCode -ne 0) { throw "--build-info exited $($proc.ExitCode)" }
$info = Get-Content $infoFile
if (-not $info) { throw "--build-info printed nothing" }
```

`-RedirectStandardOutput` also hands the child a real file handle, so
`attach_parent_console` correctly declines to attach a console. Both
`release.yml` and `release-profile.yml` now use this shape;
`release-profile.yml` had the identical latent bug and had simply never run
against a windowless binary — its last green Windows run (`30259360264`,
2026-07-27) predates commit `4e7cc53`, which is what made release builds
GUI-subsystem.

A debug build is deliberately left on the console subsystem, so `cargo run` on
Windows still prints normally. The corrected `Start-Process -Wait` path passed
on Windows in release run `31655518790`; interactive GUI behaviour remains a
captain runtime check.

### Licence files in the packaged output

Every archive ships `LICENSE` (dodo's own MIT terms) and
`THIRD-PARTY-NOTICES.md` (the dependency licences, including the
GPL-3.0-or-later crates reached through `gpui`, and the open question about
distributing built binaries). Where they land:

| Archive | Paths |
|---|---|
| `dodo-v<v>-<platform>-<arch>.tar.gz` | `<name>/LICENSE`, `<name>/THIRD-PARTY-NOTICES.md`, alongside `README.md` and the binary |
| `dodo-v<v>-windows-<arch>.zip` | the same, written by `scripts/package.ps1` |
| `dodo-v<v>-macos-<arch>-app.tar.gz` | `dodo.app/Contents/Resources/LICENSE` and `.../THIRD-PARTY-NOTICES.md` |

Three things about that which are deliberate:

- **The `.app` bundle carries them inside `Contents/Resources/`, not next to
  the bundle.** That archive contains nothing but `dodo.app`; a file beside it
  would be a second object to drag, and would be lost the moment the app was
  moved to `/Applications`. Inside `Resources/` the terms travel with the
  application.
- **A missing file is a hard error, not a thinner archive.** `package.sh`,
  `package.ps1` and `macos-app-bundle.sh` all `die`/`throw` rather than skipping
  — the previous best-effort `for doc in README.md LICENSE LICENSE.md ...` glob
  would have shipped a binary with no notice and said nothing.
- **`verify-release.sh` re-checks it after the fact**, by `find`ing both names
  in the unpacked archive (which is why the bundle's nested layout needs no
  special case) and asserting they are non-empty. Packaging and verification
  failing independently is the point.

Neither file is embedded in the binary: `src/assets.rs`'s `#[include]` filters
cover only `icons/**/*.svg` and `themes/**/*.json`, so these cost zero bytes.
`dodo --build-info` does not print licence information.

### Windows TSF artifact

`scripts/package.ps1` treats `input-method/dodo_ime_windows.dll` as a required
release artifact beside `dodo.exe`; it fails rather than publishing a ZIP whose
Native TSF button cannot work. Both the matrix verifier and the post-download
publish gate require that exact path, so a recursively found same-name DLL does
not pass. The updater replaces the packaged sidecar with the executable and
rolls both back if either replacement fails; it deliberately does not touch the
registered `%APPDATA%` copy. `cargo build` reaches the DLL through the workspace
`default-members`, and CI explicitly compiles and runs the TSF host's
class-factory harness on Windows. User installation, recovery, and the manual
runtime matrix are in [`windows-input-method.md`](windows-input-method.md).

### Checking a Windows fix without a Windows machine

`build (windows-x64)` is the row most likely to break from a macOS-only desk, so
it is worth knowing exactly how far a local cross-check can go.

`cargo check --target x86_64-pc-windows-msvc` **cannot** be run on the whole
crate here. It gets as far as `aws-lc-sys`, whose build script compiles C that
`#include`s `<windows.h>`; without the Windows SDK headers (an `xwin`-style
setup) that fails, and the failure has nothing to do with dodo's own code.
Note also that on a machine where `rustc` is Homebrew's, cargo picks `rustc` off
`PATH` even under `rustup run`, so a cross-check needs the rustup toolchain
forced explicitly (`RUSTC=~/.rustup/toolchains/<tc>/bin/rustc`) or it fails with
a misleading "can't find crate for `core`".

What does work, and is what proved the `#[cfg(unix)]` / `#[cfg(windows)]` split
in `crates/dodo-docker/src/services/engine.rs`: copy the platform-split function into a
throwaway crate that depends only on the crate in question (here `bollard`), and
`cargo check --target x86_64-pc-windows-msvc` that. It compiles the real
dependency's real Windows `impl` blocks, so it catches a connector that does not
exist on the target and any `unused` warning the inactive `cfg` arm leaves
behind — run it with `RUSTFLAGS="-D warnings"`, since clippy is blocking. It
does not prove the rest of the crate builds on Windows; only CI does that.

---

## Application icon

dodo's artwork is a dark squircle tile carrying the dodo bird and a "DODO"
wordmark. Two files in `assets/branding/` are the whole source of truth:

| File | What it is |
|---|---|
| `dodo-artwork-source.png` | the original supplied artwork, 1254×1254, **opaque**, tile on a black canvas. Never edited. |
| `dodo-1024.png` | the 1024×1024 RGBA master every icon is derived from: the same art with everything outside the tile's rounded border cut to full transparency. |

`assets/branding/dodo-256.png` sits beside them and is **not** a source: it is
derived from the master like everything else in the next table, and lives here
only because it is the one derived icon that is not tied to a single platform.
Never hand-edit it.

### Regenerating

```sh
python3 scripts/generate-icons.py            # master -> every derived artifact
python3 scripts/generate-icons.py --remaster # also rebuild the master from the
                                             # original artwork first
python3 scripts/generate-icons.py --check    # diff against what is committed
```

Both scripts are stdlib-only Python 3 — no Pillow, no ImageMagick, nothing in
`Cargo.toml` — because this is a once-per-artwork-change chore and neither tool
is present on the machine dodo is developed on. `scripts/make-icon-master.py`
carries a small hand-rolled PNG codec and explains why the transparent-corner
cut is derived from the artwork's own outline rather than a fitted superellipse
(a fitted curve that is a pixel off clips one edge and leaves a black sliver on
the other). The only external tool is `iconutil`, for the `.icns`; when it is
absent the script writes everything else, says exactly what it could not build,
and exits non-zero. It never emits a placeholder.

### What is generated, and where it goes

All of it is **committed**, because packaging must not depend on the host:
`iconutil` exists only on macOS, so a Linux runner could never build the
`.icns`.

| Artifact | Sizes | Shipped as |
|---|---|---|
| `assets/macos/dodo.icns` | 16/32/128/256/512 at 1× and 2× | `dodo.app/Contents/Resources/dodo.icns`, named by `CFBundleIconFile` |
| `assets/windows/dodo.ico` | 16/32/48/64/128/256 | **compiled into `dodo.exe`** by `build.rs`, and also a loose file next to it in the ZIP |
| `assets/linux/hicolor/<n>x<n>/apps/dodo.png` | 16/24/32/48/64/128/256/512 | `share/icons/hicolor/…` in the tar.gz |
| `assets/linux/dodo.desktop` | — | `share/applications/dodo.desktop` in the tar.gz (hand-written, not generated) |
| `assets/branding/dodo-256.png` | 256 | **embedded in the binary** by `src/window_icon.rs`, macOS and Linux only |

The Linux tar.gz lays those out under `share/` exactly as they must end up on
disk, so installing is `cp -r share/ ~/.local/` (or `/usr/local/`) with no
renaming, and a future `.deb`/AppImage job can copy the tree wholesale.

**Only the last row is embedded in the binary, and not through `rust-embed`.**
`src/assets.rs` embeds `assets/` with explicit `#[include]` filters —
`icons/**/*.svg` and `themes/**/*.json` — and every path in the table falls
outside both, which is why the branding artwork and the packaged icons cost the
binary nothing. Confirmed by measurement when they were added:
`target/release/dodo` was 20,513,488 bytes before and after, byte for byte.
Anything new under `assets/` that must stay out of the binary has to stay
outside those two filters; check the size, do not assume.

`dodo-256.png` reaches the binary by a different mechanism — an
`include_bytes!` in `src/window_icon.rs`, under
`#[cfg(any(target_os = "macos", target_os = "linux"))]` — so unlike everything
above it, it does add to the macOS and Linux binaries: roughly the size of the
committed file, which is why the generator derives 256 and not 512. A Windows
build does not carry it at all; `build.rs` has already put a `.ico` in the
resource table, which is where Windows looks.

### Windows icon: embedded

`dodo.exe` carries an `RT_GROUP_ICON` resource built from
`assets/windows/dodo.ico`, which is what Explorer, the taskbar and Alt-Tab
read. Three pieces:

- `winresource` as a **target-scoped build-dependency** in `Cargo.toml`;
- `embed_windows_icon` at the end of `build.rs`;
- the resulting `Cargo.lock` entry, landed as its own reviewed commit.

The `.ico` still ships loose in the ZIP as well, for shortcuts and any future
MSI.

**The two `cfg`s involved are different questions with different answers, and
this is the part that will catch the next person.** Cargo evaluates a
*target*-scoped **build**-dependency against the **target**
(rust-lang/cargo#4932), so `winresource` is in the graph exactly when something
is being built for Windows. Verified rather than assumed:

```
$ for t in aarch64-apple-darwin x86_64-apple-darwin \
           x86_64-unknown-linux-gnu x86_64-pc-windows-msvc; do
    cargo metadata --format-version 1 --filter-platform $t | grep -c winresource
  done
# present for x86_64-pc-windows-msvc only
```

The `#[cfg(windows)]` inside `build.rs`, by contrast, is the **host**, because
a build script is compiled for the machine that runs it. The two agree on a
native Windows build — which is the only kind `.github/workflows` does — and
disagree otherwise: a Windows binary cross-built from a Mac would get no icon
(the `#[cfg(not(windows))]` arm emits a `cargo:warning` rather than letting
that be silent), and a non-Windows target built *on* Windows would fail to
compile `build.rs`. Nothing dodo does hits the second; a target-scoped
build-dependency cannot express "host", so it is recorded instead of guarded.

**Embedding failure panics**, which is the one exception to `build.rs`'s
"never fail the build" rule and is called out at the top of that file. The
reasoning: a packaging step that quietly does nothing is exactly how the
generic icon survived this long. If a runner ever turns up without `rc.exe`,
the one-line retreat is to replace the `.expect(...)` in `embed_windows_icon`
with a `cargo:warning` — but do that knowing the icon is then unverifiable
again.

**What has actually been checked, and what has not.** Checked from macOS:
`winresource` 0.1.31's API against the exact call (`cargo check` of a
throwaway crate carrying the same `build.rs` body, per
[Checking a Windows fix without a Windows machine](#checking-a-windows-fix-without-a-windows-machine));
the four-target `cargo metadata` result above; and that the committed `.ico`
decodes as a well-formed 6-entry `ICONDIR` — 16/32/48/64 as 32-bit DIBs,
128/256 as PNG, every offset in bounds. **Not checked: that `rc.exe` finds and
links it, and that Explorer draws it.** dodo has never been built or run on
Windows. The first person with a Windows machine should run
`cargo build --release --locked` and confirm the `.exe` shows dodo's artwork in
Explorer; until then this row is written-but-unverified, exactly like the rest
of the Windows packaging.

### The bare-binary cases

The rows above all assume the OS can find a *file*: a bundle's `Info.plist`, an
installed `.desktop` entry, a resource table. Two launches have none of that,
and both are what a developer does every day.

**macOS, run directly** (`cargo run`, `target/release/dodo`). A bare Mach-O has
no `Info.plist` and no `Contents/Resources`, so there is nothing for
`CFBundleIconFile` to name and the Dock draws the generic executable tile. This
is expected by design and is *not* the same bug as a broken bundle — the
bundled `.app` has always been right. `src/window_icon.rs` answers it at
runtime with `-[NSApplication setApplicationIconImage:]`, which is the only
route available: GPUI exposes no dock-icon API at all.

**A bundled `.app` is deliberately skipped**, detected by the executable's path
shape (`<name>.app/Contents/MacOS/<exe>`) rather than by an `NSBundle` call, so
the decision is unit-tested on any host. The reason to skip is quality, not
safety: `dodo.icns` carries hand-built 16 and 32 pt variants that a downscaled
256 px PNG cannot match. If that check ever regresses, the visible result is the
same artwork slightly softer at small sizes — not a generic icon.

**Linux, every session type.** GPUI leaves `WindowOptions::app_id` at `None`,
and it turns out that alone was enough to break the icon everywhere: with no
app id, a Wayland `xdg_toplevel` reports nothing for the compositor to match
`dodo.desktop` against, and GPUI's X11 backend never calls `set_app_id`, so the
window carries **no `WM_CLASS` at all** and `StartupWMClass=dodo` matches
nothing. The desktop file's own comment predicted this. `window_icon::APP_ID`
is now set on every window, and a unit test fails if the constant and the
desktop entry's filename, `StartupWMClass` and `Icon=` drift apart.

`WindowOptions::icon` is set too, which covers one further case: an **X11**
session running a binary whose `.desktop` was never installed. It is X11-only —
GPUI's Wayland backend implements no equivalent (`xdg-toplevel-icon-v1` is not
wired up), so a bare binary under Wayland is not reachable from inside dodo and
needs the tarball's `share/` tree installed.

None of the Linux half can be checked from here; see the honesty note under
"Verified, not assumed".

### Verified, not assumed

A `.icns` that `iconutil` accepted can still render as a blank generic document
— exit 0 proves nothing here. What was actually checked on macOS arm64:

- the master's corner pixels have alpha 0 and its centre alpha 255, with the
  edge fading over ~1px and edge pixels carrying the tile's own border colour
  rather than black (no halo);
- `sips` decodes the `.icns` back to recognisable artwork;
- `iconutil` produced all ten `ic**` entries including `ic10` (1024);
- **`dist/dodo.app` shows the dodo icon in Finder and in the Dock at normal
  size**, screenshotted, after `lsregister -f` on the freshly built bundle.

Repeat at least the last one after any artwork change.

**That list was, and is, about one case out of five, and reading it as a
verdict on "the icon" is what let four broken ones sit unnoticed.** It says
nothing about the bare binary, about Windows or about either Linux session
type. Corrected scope:

| Case | Status |
|---|---|
| macOS, bundled `.app` | **seen working**, and re-confirmed by the captain on 2026-08-05 — the four bullets above |
| macOS, bare binary | fixed at runtime; the fix compiles and its bundle-detection is unit-tested, but **nobody has looked at the Dock yet** |
| Windows | fixed at build time; API call and `.ico` structure checked from macOS, **never built on Windows** |
| Linux, Wayland | `app_id` now set; **never built or run on Linux** |
| Linux, X11 | `app_id` + `_NET_WM_ICON` now set; **never built or run on Linux** |

The rule that produced the first row is the one to keep: *look at the pixels*.
Everything else in this section is a proxy.

#### Checking the macOS cases without guessing

The Dock caches aggressively, and a stale Launch Services registration is the
usual reason a "fix" looks like it did nothing. Build and inspect from a clean
state:

```sh
cargo dist                                          # target/release/dodo
scripts/macos-app-bundle.sh --binary target/release/dodo
/System/Library/Frameworks/CoreServices.framework/Frameworks/LaunchServices.framework/Support/lsregister \
    -f dist/dodo.app                                # re-register, do not trust the cache
```

Then:

- **bundled** — open `dist/dodo.app` from Finder. Its Dock tile comes from
  `Contents/Resources/dodo.icns`; `src/window_icon.rs` deliberately does not
  touch it. Confirm nothing regressed.
- **bare** — run `target/release/dodo` (or `cargo run`) from a terminal. Its
  Dock tile now comes from the embedded 256 px PNG. This is the case that was
  broken.

Non-visual checks that need no launch, and are what CI or a scripted check can
do: `plutil -p dist/dodo.app/Contents/Info.plist` must show
`CFBundleIconFile => dodo`, `dist/dodo.app/Contents/Resources/dodo.icns` must
exist, and `sips -g pixelWidth dist/dodo.app/Contents/Resources/dodo.icns` must
decode it.

---

## Creating a new release

1. **Decide the version** (see semantic versioning below) and set it in
   `Cargo.toml`. Nothing else stores a version number; `build.rs` and the
   packaging scripts both read that one.
2. `cargo build --release --locked` and `cargo test --locked` locally. CI will
   do it too, but a failing release workflow is a worse place to find out.
3. Commit: `chore(release): v0.2.0`. The tag must point at a commit whose
   `Cargo.toml` already carries the new version — the `meta` job refuses a tag
   that disagrees.
4. Tag and push:
   ```sh
   git tag -a v0.2.0 -m "dodo v0.2.0"
   git push origin main
   git push origin v0.2.0
   ```
5. Watch the run. When it finishes, the Release exists with one archive per
   platform, each with a `.sha256` sidecar, plus a combined `SHA256SUMS`.
6. **Download the macOS archive on a real machine and open the app.** CI cannot
   do this for you.

To rehearse the whole pipeline without publishing anything, run the workflow
manually (`workflow_dispatch`) from a branch: it builds, packages and verifies,
and stops before creating a Release.

### Semantic versioning

dodo is pre-1.0, so the practical reading of semver here is:

- **0.x.y → 0.x.(y+1)** — bug fixes, internal changes, dependency bumps that do
  not change what the user sees.
- **0.x.y → 0.(x+1).0** — a new tool in the sidebar, a new page, a changed
  keybinding, a settings change, anything that alters the UI or persisted data
  (today: the API Explorer collections under `~/Library/Application Support/dodo/`).
- **1.0.0** — when the persisted data format and the tool set are ones we are
  willing to keep compatible. Not yet.

Pre-releases use a suffix: `v0.2.0-rc.1`. The workflow detects the `-` and marks
the GitHub Release as a pre-release automatically.

### Tagging strategy

- Tags are `v` + the exact `Cargo.toml` version: `v0.2.0`. The `meta` job
  enforces the match.
- Annotated tags (`git tag -a`), not lightweight ones: the tag object carries
  who made the release and when.
- Tags are immutable. A broken release gets `v0.2.1`, never a moved `v0.2.0` —
  the archives, the checksums and the metadata embedded in the binary all name
  the commit, and moving the tag makes all three lie.
- The tagged commit's committer timestamp becomes `SOURCE_DATE_EPOCH`, so
  re-running the workflow for an existing tag rebuilds the same `build_time`.

---

## Automatic updates

**Scope.** This section describes both halves: what a release *publishes*
(`update.json`, generated by `tools/update-manifest`, which is not part of dodo)
and what the application *reads* (`crates/dodo-updater/src/`, which is). The publishing half
came first and is unchanged by the second; the reading half is described under
[The in-app updater](#the-in-app-updater) below.

### What a release publishes

Per platform, unchanged: one archive and its `.sha256` sidecar, plus a second
`-app.tar.gz` archive on each macOS platform. New, at the release root:

| Asset | What it is |
|---|---|
| `update.json` | The manifest an updater reads: one entry per platform, with URL, SHA-256, byte size and a reserved signature field. |
| `SHA256SUMS` | Every archive in the release, in the layout `sha256sum -c` reads. Now written by the generator, which hashes each file itself, rather than by concatenating the sidecars. |

```json
{
  "manifest_version": 1,
  "channel": "stable",
  "version": "0.2.0",
  "notes": "## dodo v0.2.0\n…",
  "published_at": "2026-07-30T12:11:03Z",
  "files": {
    "macos-arm64": {
      "url": "https://github.com/MrGru/dodo/releases/download/v0.2.0/dodo-v0.2.0-macos-arm64-app.tar.gz",
      "sha256": "1faaa4c3…",
      "size": 11567151,
      "signature": null
    }
  }
}
```

Three fields are there for the future and are cheap now only because they were
put in from the first release:

- **`manifest_version`** — a client that only understands version 1 can refuse a
  version 2 document instead of mis-parsing it. Impossible to retrofit: by the
  time you need it, manifests without it are already in the wild.
- **`signature`**, per file — reserved for Ed25519/minisign. It is always
  `null`, **nothing verifies it**, and no signing is implemented. Adding it later
  becomes populating a field rather than a schema break.
- **`channel`** — written into the document even though `stable` is the only
  channel that works today. See [Channels](#channels).

**The macOS entry points at the `-app.tar.gz` bundle, never the bare binary,**
because an installer replaces `dodo.app`. That is selected by exact filename;
if the bundle is missing, the release fails rather than silently falling back to
the bare archive.

### How `update.json` is generated

By `tools/update-manifest`, a standalone crate. It is **not** part of dodo:
`exclude = ["tools/*"]` in the root `Cargo.toml` keeps it out of the package, it
has its own `Cargo.lock` and four dependencies (`serde`, `serde_json`, `sha2`,
`semver`), and it is named in `workspace.exclude` as well, so it stays outside
the workspace that `crates/dodo-ime-core` joined. It costs the shipped binary
**zero bytes** and the app's test and clippy runs zero time.

`cargo metadata --no-deps` at the repo root listed exactly one package when that
was written; it lists two now — `dodo` and `dodo-ime-core` — and still not
`update-manifest`. The difference is the point: dodo *links* the engine crate and
does not link the generator, so the engine shares dodo's `Cargo.lock` and the
generator keeps its own. See the `[workspace]` comment in the root `Cargo.toml`.

The publish job runs it *before* creating the release:

```sh
cargo run --release --locked --manifest-path tools/update-manifest/Cargo.toml -- \
  --version 0.2.0 --channel stable \
  --dir artifacts --repo MrGru/dodo --tag v0.2.0 \
  --notes-file "$NOTES" --published-at 2026-07-30T12:11:03Z \
  --out artifacts/update.json --sums-out artifacts/SHA256SUMS \
  --expect-platform macos-arm64 --expect-platform macos-x64 \
  --expect-platform linux-x64  --expect-platform windows-x64
```

It exits non-zero, having written nothing, on any of: an expected platform with
no artifact; a file in `--dir` it does not recognise (including an archive from a
*different* version); a computed hash that disagrees with the `.sha256` sidecar;
a missing sidecar. Hashes are streamed, never read into memory. `published_at`
comes from `SOURCE_DATE_EPOCH`, so re-running the job for the same tag produces a
byte-identical manifest.

### Why a missing platform blocks the whole release

**Any missing platform fails the release, including the experimental ones.**
Three of the four build matrix rows are `experimental: true` and therefore
`continue-on-error: true`, and the publish job gates on
`needs.build.result == 'success'` — which a `continue-on-error` failure still
satisfies. So the pipeline was free to publish a release with a platform missing,
and did: **v0.1.5 shipped eleven assets and no Windows archive**, with nothing
anywhere recording that one had been expected.

The alternative — omit the missing platform from the manifest and publish
anyway — was rejected. A manifest is a promise to a client that is going to act
on it unattended: a Windows user on an older version would poll, find no
`windows-x64` entry, and be told they are up to date. That failure is silent,
indefinite, and invisible from the release page, which looks complete. A failed
release is loud, happens in front of the person who triggered it, and is
repairable in minutes now that publishing is re-runnable.

**When the block is the wrong answer** — a platform is genuinely broken and the
release must go out without it — the fix is to state that in the workflow by
dropping its `--expect-platform` line, in a commit someone reviews. Leaving a
platform out of the manifest should be a decision with a diff attached, not a
side effect of a red matrix row nobody noticed.

### Publishing is re-runnable

`gh release create` fails with *"a release with the same tag name already
exists"*, so a partially-published tag could never be repaired by re-running,
and tags here are immutable (see [Tagging strategy](#tagging-strategy)) so
deleting and re-tagging is not an option either. This is not hypothetical:
v0.1.5's attempt 1 published, and the re-run (attempt 2, job `90886356082`) died
on exactly that error.

The publish step now probes with `gh release view` and either creates the
release (keeping `--verify-tag`, which correctly refuses to invent a missing
tag) or `gh release edit`s it and re-uploads with `--clobber`. Re-running the
job converges on the intended state instead of failing.

### Channels

`--channel` is written into the document from the first release, so a client can
always tell which stream a manifest describes.

Only **stable** is wired up. Its URL is the one the app will default to:

```
https://github.com/MrGru/dodo/releases/latest/download/update.json
```

**The trap:** `latest` excludes pre-releases. Since the workflow marks any tag
with a suffix (`v0.2.0-rc.1`) as a pre-release, a beta or nightly manifest
published this way is unreachable at that URL — it will always resolve to the
newest *stable* release, and a beta client would silently be handed stable's
manifest.

To add a channel, the generator needs no change (pass `--channel beta`); what
needs deciding is the **publication path**. The recommendation, and the reason
the shape above does not foreclose it:

- **Serve channel manifests from GitHub Pages**, one stable path per channel —
  `https://mrgru.github.io/dodo/updates/{stable,beta,nightly}.json` — with the
  release assets themselves still living on the GitHub Release. The path is
  mutable by design, so it does not fight the immutable-tag rule; it is static,
  cacheable and needs no API token or rate limit; and it lets `stable` keep its
  `releases/latest/download/update.json` URL as-is.
- **Not** a moving `channel-beta` tag: that contradicts
  [Tagging strategy](#tagging-strategy) outright.
- **Not** querying the releases API and filtering on `prerelease`: it needs a
  token, it is rate-limited, and it turns a static fetch into a paginated one.

### Verifying a manifest by hand

Everything in the manifest is checkable with no special tooling:

```sh
# 1. fetch the manifest and the archives it names
gh release download v0.2.0 --repo MrGru/dodo --dir artifacts

# 2. the combined checksum file covers every archive
cd artifacts && sha256sum -c SHA256SUMS

# 3. the manifest agrees with the archives, per platform
python3 - <<'PY'
import hashlib, json, os
m = json.load(open("update.json"))
assert m["manifest_version"] == 1, m["manifest_version"]
for key, f in m["files"].items():
    name = f["url"].rsplit("/", 1)[1]
    body = open(name, "rb").read()
    got = hashlib.sha256(body).hexdigest()
    assert got == f["sha256"], f"{key}: {got} != {f['sha256']}"
    assert len(body) == f["size"], f"{key}: size"
    assert f["signature"] is None
    print(f"{key:12} {name}  ok")
PY
```

Two things worth eyeballing while you are there: each `macos-*` URL ends in
`-app.tar.gz`, and every platform you expected is present as a key.

### Binary size cost

**Zero bytes**, measured rather than assumed, per the standing policy in
`docs/build-optimization.md`.

`cargo build --release --locked` on macOS arm64, both from a genuine compile
(the baseline was forced with `touch src/main.rs`, so cargo could not simply
declare it up to date):

| Tree | Bytes | SHA-256 |
|---|---|---|
| Pristine `main` | 22,507,024 | `ad5413e7c9fef795…` |
| With `tools/update-manifest` + `exclude` | 22,507,024 | `ad5413e7c9fef795…` |

**Byte-identical**, so the delta is exactly 0 — not "too small to see". That is
what a separate crate buys: `cargo metadata --no-deps` at the repo root lists
`dodo` as the only package and one workspace member, `cargo test --locked
--all-features` still reports 636 tests, and `cargo clippy --all-targets` still
lints only dodo. Reverting the `exclude` key did not even cause cargo to
rebuild, which is the same fact from the other direction: `exclude` is packaging
metadata and is not part of the build fingerprint.

The generator's own dependencies (`serde`, `serde_json`, `sha2`, `semver`) are
locked in `tools/update-manifest/Cargo.lock` and never enter dodo's graph.

---

## The in-app updater

`crates/dodo-updater/` is the consumer of everything above. Its `lib.rs` is the
authority on the module's structure; this section records the decisions a
future release engineer needs and the things that were *proved* rather than
assumed.

### The flow

```
launch ──10s──► silent check ──► parse ──► compare semver ──► nothing? stop
                                                     │
                                                  newer? ──► dialog opens
                                                                 │
                                            user presses Download │
                                                                 ▼
                                   stream to a temp file, hashing as it goes
                                                                 ▼
                                       re-read from disk: size, then SHA-256
                                                                 ▼
                                     install (swap) ──► "Restart to update"
```

**Check silently, ask before downloading.** The background check opens nothing
and says nothing unless it finds something. Nothing is downloaded until the
user presses a button, and that is structural rather than remembered:
`services::pipeline::check` is handed no `Downloader`, so it *cannot* fetch an
archive however it is called.

### Configuration — dodo's first persisted setting

`updater.json`, beside the other three files under `data_dir()`. It follows
`script-consent.json`'s discipline (explicit `"version"` from the first write, a
parser that refuses a higher one, a missing file meaning first run, an atomic
temp-then-rename write) and pointedly not `collections.json`'s.

| Key | Default | Meaning |
|---|---|---|
| `auto_update` | `true` | Master switch for **checking**. It has never meant unattended installing; there is no setting that does. |
| `channel` | `"stable"` | See [Channels](#channels). Deserialized leniently — an unrecognised value falls back to `stable` rather than failing the whole file. |
| `manifest_url` | the `latest` URL above | Refused at fetch time unless it is `https://`. |
| `check_on_startup` | `true` | The one check ~10s after launch. |
| `check_interval_hours` | `24` | Re-check cadence while running; clamped to 1–672. |
| `skipped_version` | `null` | What **Skip this version** recorded. This key is why the file has to exist at all: a skip that expired every launch would make the button a lie. |

### `data_dir()` had to be fixed first

It knew only macOS's `~/Library/Application Support/dodo` and fell back to a
`.dodo` folder in the **current working directory** everywhere else — and on
Windows `HOME` is normally unset, so that fallback *was* the Windows branch.
`src/paths.rs` now has real `%APPDATA%` and `$XDG_CONFIG_HOME`/`~/.config`
branches, with the macOS path unchanged so no existing installation is orphaned.
It classifies the platform from `build_info::VERSION_INFO.target` rather than
`#[cfg]`, so all three branches are unit tested from a Mac that cannot compile
two of them.

### Verification is not a signature check

The updater checks the archive's **size and SHA-256 against the manifest**, and
the manifest arrives over HTTPS from the same origin as the archive. That is an
integrity check: it catches a corrupt or truncated transfer and a mirror serving
the wrong bytes. It is **not** a defence against someone who controls the
release itself — that is what the manifest's reserved `signature` field is for,
and nothing populates or verifies it yet.

Two properties hold regardless: a mismatch **discards the file** and never
reaches an installer, and **a downloaded file is never executed**. Extraction
runs the operating system's own `tar` with the archive as its *input*; the
installed binary runs only after verification has passed and only when the user
presses Restart.

### The three installers, and what "atomic" actually means

`services/installers/` carries `#[cfg(target_os)]` in exactly one place — the
`platform_installer()` factory. All three compile and are **tested on every
host**, because nothing in them is a platform API; that is deliberate insurance
against the failure mode this repo has already had (a `#[cfg(unix)]`-only
bollard connector that failed `build (windows-x64)` on its one real run).

- **macOS** replaces `dodo.app`: extract beside it, `xattr -dr
  com.apple.quarantine`, swap, relaunch with `open -n`.
- **Windows** cannot delete a running `.exe` but can rename one: the running
  binary moves to `dodo.exe.dodo-old`, the new one takes its path, and the
  **next launch** deletes the stale file (`sweep_stale`, run from
  `updater::init`). That is the "schedule for deletion", and it needs no Win32
  call.
- **Linux** replaces in place when the directory is writable. AppImage is
  explicitly out of scope: it has its own update protocol, dodo publishes none,
  and a running AppImage's `current_exe` is inside a read-only mount, so this
  reports a location problem and leaves the archive — the right answer, reached
  without pretending to understand the format.

**The swap is not one atomic operation, and the word is worth not using
loosely.** It is two renames — old aside, new into place — each atomic on its
own, with a rollback if the second fails; the window in which neither exists is
one `rename(2)`. macOS does offer a genuinely atomic exchange (`renamex_np` with
`RENAME_SWAP`) and it is *not* used, because reaching it needs a direct `libc`
dependency for one call on one platform.

**Refusing to install is a normal outcome, not a failure.** Running as a bare
binary, an unwritable `/Applications`, a read-only volume: each ends with the
archive downloaded, verified, and the user told where it is.

### What has actually been proved

Run against the **live v0.1.6 release** on macOS arm64, with the running
version temporarily lowered to 0.1.5 so an update would be found:

| Step | Result |
|---|---|
| Fetch `releases/latest/download/update.json` | 1,725 bytes |
| Parse | `manifest_version=1`, `channel=stable`, `version=0.1.6` |
| Compare `0.1.5 → 0.1.6` on the stable channel | `Offer` |
| Stream `dodo-v0.1.6-macos-arm64-app.tar.gz` | 11,569,143 bytes; streamed digest `0a404f82…d1c3` |
| Verify from disk | size and SHA-256 both match the manifest |
| Install over a fabricated `dodo.app` | `Installed` |
| Run the swapped-in binary | `dodo 0.1.6 (e0829ee2 2026-07-30T15:03:24Z)` — the released build |
| Previous bundle | kept as `dodo.app.dodo-old`, then removed by `sweep_stale` |

Separately, the **background check** was run from a real launch of the GUI app,
twice: as 0.1.5 it found 0.1.6 and opened the dialog; as 0.1.6 it printed
nothing at all, which is the "silent" half of the behaviour.

**What has not been proved.** No screenshot of the rendered dialog exists: the
shell this was built in has neither Screen Recording permission (`screencapture`
returns a black frame) nor Accessibility permission (System Events refuses
synthetic clicks), so the dialog's *appearance* and the button clicks inside it
are unverified. The dialog's width behaviour at narrow and wide viewports is
covered by unit tests over `card_size_for` instead, which is the specific defect
class that matters — `Dialog` computes `left` from the width it is handed, so an
over-wide card is pushed off both edges rather than clipped. The Windows and
Linux installers' *sequences* are tested on this Mac; the platforms' own
behaviour (whether Windows really permits renaming a running `.exe`) is not, for
the same reason nothing else here has ever run on Windows.

### Where the rest plugs in

- **Signature verification.** `ManifestFile::signature` is already read and
  already `Option<String>`. Requiring it is a check in
  `models::manifest::validate_file` plus a verifier; the schema does not change.
- **A Settings page for the updater.** The dialog carries one checkbox
  (**Check for updates automatically**, which writes `auto_update`). Channel,
  interval and manifest URL are file-only today; they are ordinary
  `SettingField::dropdown`/`input` additions in `settings.rs` reading
  `Updater::config`.
- **Delta updates.** The manifest shape does not foreclose them — a second
  entry per platform beside `url` — but a 12 MB download is not yet a problem
  worth a patch format.

---

## Required GitHub Secrets

**None today.** The release workflow uses only `${{ github.token }}`, which
Actions provides automatically, and needs `contents: write` — granted narrowly
on the `publish` job rather than workflow-wide.

The secrets below are for the future-readiness items in the next section. None
of them is referenced by any workflow yet; adding one is what turns the
corresponding commented-out step on. For the macOS rows,
[docs/macos-signing.md](macos-signing.md) says where each value comes from, how
to produce it (a `.p12` becomes `MACOS_CERTIFICATE` through `base64 -i`), and
which three further names are needed if notarisation uses an App Store Connect
API key instead of an app-specific password.

| Secret | For | Notes |
|---|---|---|
| `MACOS_CERTIFICATE` | macOS signing | Developer ID Application cert, base64 `.p12` |
| `MACOS_CERTIFICATE_PWD` | macOS signing | password for that `.p12` |
| `MACOS_NOTARY_APPLE_ID` | notarisation | Apple ID with the Developer Program |
| `MACOS_NOTARY_TEAM_ID` | notarisation | 10-character team identifier |
| `MACOS_NOTARY_PASSWORD` | notarisation | app-specific password for `notarytool` |
| `WINDOWS_CERTIFICATE` | Windows signing | base64 `.pfx` |
| `WINDOWS_CERTIFICATE_PWD` | Windows signing | password for that `.pfx` |
| `SYMBOL_UPLOAD_TOKEN` | crash symbolication | whichever service ends up used |

---

## Future readiness

Structured for, not implemented. Each entry says where the change goes.

**macOS code signing and notarisation.** [docs/macos-signing.md](macos-signing.md)
is the authority and is written for the moment the decision is taken: what the
repo owner must personally buy and create, the secrets by exact name and how to
produce each value, the entitlements (dodo needs none, and neither will the
input-method bundle), the ordering constraints, and what breaks. In summary:
`scripts/macos-app-bundle.sh` ends with the `codesign` / `notarytool` /
`stapler` sequence as a comment, and that is where it happens — **inside**
packaging, before `scripts/package.sh` tars the bundle and checksums it, not
"between packaging and upload" as this section used to say (the published
SHA-256 is computed from that archive). The `release.yml` guard cannot read
`secrets` in an `if:`; it reads an `env:` set from the secret at job level.
Until then archives are unsigned and Gatekeeper quarantines them; the generated
release notes tell users to run `xattr -dr com.apple.quarantine`. Signing is a
user-experience purchase — an unsigned dodo, and an unsigned input method, both
run today.

**Windows code signing.** Same shape, in `scripts/package.ps1` — sign the
`.exe` *before* zipping it.

**MSI.** Would be built from the signed `.exe` with WiX or `cargo-wix`, as an
extra asset alongside the ZIP, never as a replacement for it.

**Linux packages (.deb, .rpm, AppImage).** Not started. `cargo-deb` and
`cargo-generate-rpm` both read metadata from `Cargo.toml`, so the natural first
step is a `[package.metadata.deb]` section plus one more matrix step. The
desktop entry and icons an AppImage or `.deb` needs now exist and are already
staged in the tar.gz under `share/` — see
[Application icon](#application-icon).

**Automatic updates.** Both halves are built — see
[Automatic updates](#automatic-updates) for what a release publishes and
[The in-app updater](#the-in-app-updater) for what the app does with it. What
remains is **signing**: the manifest's `signature` field is read, is always
`null`, and nothing verifies it, so the integrity check is against a digest
served from the same origin as the archive and not against a key. That is the
next thing to build here, and it is a schema-compatible addition rather than a
break.

The question that used to sit in this slot — whether a developer tool should
phone home at all — was answered by the shape rather than by a policy: the check
is one unauthenticated `GET` of a static file on the release page, it sends no
identifier beyond a User-Agent naming the version, and `auto_update: false`
turns it off entirely. Nothing is downloaded without a button press.

**Crash reporting and symbol upload.** The shipped binary is `strip =
"symbols"`, so a crash report from it is addresses only. The other half of that
trade is the `release-debug` profile in `Cargo.toml`: identical code and
optimisation, plus full debug info in a separate `.dSYM` (`split-debuginfo =
"packed"`). The intended flow, when a crash reporter exists, is:

1. build the release artifact with `release`;
2. build the same commit with `release-debug` (`cargo dist-debug`) and keep
   `target/release-debug/dodo.dSYM` — 277 MB, so archive it, do not attach it
   to the Release;
3. upload the `.dSYM` to the symbol server keyed by its UUID (`dwarfdump
   --uuid`, which matches the shipped binary's UUID even after stripping —
   verified locally);
4. symbolicate incoming reports against it.

The alternative — shipping symbols in the binary — costs every user the
download and buys nothing they can use.

**Telemetry.** Not implemented and not scaffolded. This is a local developer
tool; the burden of proof is on adding it.

**Application icons.** Done on every platform and every launch method — see
[Application icon](#application-icon). What remains is not code: **somebody has
to look at Windows and Linux.** Both are written-but-unverified, and the table
under "Verified, not assumed" says exactly which claim is standing on what.

**A Wayland window icon for a bare binary.** The one case still genuinely
unreachable. `WindowOptions::icon` is X11-only in the pinned GPUI, and its
Wayland backend implements no equivalent — the `xdg-toplevel-icon-v1` protocol
that would provide one is not wired up there. So a Linux binary run without its
`share/applications/dodo.desktop` installed shows a generic icon under Wayland
and there is nothing dodo can do about it from inside. Revisit if GPUI adds the
protocol; until then, the answer is to install the tarball's `share/` tree,
which is one `cp -r`.

---

## Local testing

The release path can be exercised end to end on macOS without GitHub:

```sh
# 1. build exactly what CI builds
cargo build --release --locked          # or: cargo dist

# 2. package it (adds --app-bundle for the .app archive)
scripts/package.sh --app-bundle

# 3. verify what came out
scripts/verify-release.sh dist/dodo-v0.1.0-macos-arm64.tar.gz \
    --expect-version 0.1.0
```

`scripts/package.sh` and `scripts/verify-release.sh` are the same scripts CI
runs; nothing about the release is workflow-only. Note that a local build from
a modified working tree embeds a `-dirty` commit, and `verify-release.sh`
rejects it — that is deliberate, and the reason a local rehearsal should start
from a clean tree.

All three steps above have been run on macOS arm64 against a clean tree: both
archives verify green, the `Info.plist` passes `plutil -lint`, and the
generated `dodo.app` launches with `open`. That is the part of the release
pipeline with real evidence behind it.

To check the workflows themselves without pushing:

```sh
actionlint .github/workflows/*.yml      # with shellcheck on PATH, it also
                                        # lints every `run:` block
shellcheck -S warning scripts/*.sh
```

Both were run against this tree — actionlint 1.7.7 with shellcheck 0.10.0 —
and both are clean. That validates syntax, expression references and the shell
inside each step. It does not validate that the jobs *work*: no runner has ever
executed them.

---

## Troubleshooting

**`tag vX.Y.Z does not match Cargo.toml version A.B.C`** — the tag was created
before `Cargo.toml` was updated, or on the wrong commit. Delete the tag
(`git tag -d`, `git push --delete origin`), fix the version, commit, re-tag.

**`commit is 'unknown'`, from `verify-release.sh`** — the build could not see
git. In CI that means `actions/checkout` did not run or ran without history;
locally it means the build happened outside a git checkout. `build.rs` never
fails for this, by design, so it surfaces here instead.

**`built from a modified working tree` (`-dirty`)** — exactly what it says. In
CI it should be impossible and means something in the job modified a tracked
file after checkout.

The inverse is worth knowing locally: `build.rs` only re-runs when git HEAD or
one of the `GITHUB_*` variables changes, so an incremental local build after
editing a source file can still report the *clean* commit it was last stamped
with. That is deliberate — otherwise `build_time` would churn on every edit —
and it is never wrong in CI, which builds from a fresh checkout. `touch
build.rs` forces a re-stamp if you need one.

**A `cargo` step fails with "the lock file needs to be updated"** — something
changed `Cargo.toml` in a way that changes resolution, and `--locked` refused
to rewrite `Cargo.lock`. Run `cargo build` locally, review the resulting lock
diff carefully (it may have moved a git dependency to a new upstream commit —
see `docs/build-optimization.md`), and commit it deliberately.

**The Linux build fails on a missing system library.** Expected; nobody has
built dodo on Linux. Add the package to
`.github/actions/linux-build-deps/action.yml` and note it there.

**`cargo-audit` fails while *loading* the advisory database** — an outdated
cargo-audit cannot parse advisories that use CVSS 4.0. This happens with locally
installed copies; the CI job installs a current one. It is a tool problem, not a
finding about dodo.

**The macOS download will not open** ("dodo is damaged and can't be opened") —
the binaries are unsigned, so Gatekeeper quarantines them:
`xattr -dr com.apple.quarantine dodo.app`.
