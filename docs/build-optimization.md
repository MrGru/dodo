# Build optimization

What the release build does, why, and what each decision was actually worth.

**Every number in this document was measured**, on the machine below, by
running the build. Where something was not measured — runtime performance,
anything on Linux or Windows — it says so instead of guessing.

| | |
|---|---|
| Machine | Apple Silicon (aarch64-apple-darwin), macOS 25.5 |
| Toolchain | rustc 1.96.0 (ac68faa20 2026-05-25), Homebrew |
| Commit | `9f88c698` (Docker round 5) plus the changes this document describes |
| Method | clean `target/release`, `cargo build --release --locked`, wall clock via `/usr/bin/time` |

Not measured: startup time, frame time, memory at runtime, or anything on
Linux or Windows. dodo has never been built on either.

---

## Binary size: before and after

| Configuration | Bytes | MiB | vs baseline | Build | Crates compiled |
|---|---:|---:|---:|---:|---:|
| Baseline — cargo's stock `release` (no profile section) | 31,237,488 | 29.79 | — | 3m 06s | 496 |
| `opt-level=3`, `codegen-units=1`, `strip="symbols"`, `lto="thin"` | 22,370,896 | 21.34 | **−28.4%** | 4m 29s | 496 |
| **Shipped** — the same with `lto="fat"` | **20,513,488** | **19.56** | **−34.3%** | 5m 42s | 496 |
| Shipped + `panic="abort"` (rejected, see below) | 17,040,784 | 16.25 | −45.4% | 4m 26s | 401 |
| Shipped + `--no-default-features` (no syntax highlighting) | 19,998,320 | 19.07 | −36.0% | n/a | 2 |

The baseline row is cargo's default release profile: `opt-level = 3` but
`codegen-units = 16`, no LTO and no stripping. The 8.87 MB the first step
removes is `strip`, `codegen-units = 1` and thin LTO together — their
individual contributions were not isolated, and the symbol table is expected to
dominate.

The "crates compiled" column is there because two of the build times are not
directly comparable and it would be dishonest to present them as if they were.
The first three rows each recompiled the whole graph. `panic="abort"` skipped
the 95 host-only crates (proc macros and build scripts, which the setting does
not affect). `--no-default-features` recompiled only `gpui-component` and
`dodo` and simply left the grammar crates out of the link, so its wall clock
says nothing about a clean build and is omitted.

`cargo build --release --locked` from a warm `target/` — only dodo's own crate
changed — takes 2m 43s, essentially all of it fat LTO plus the link.

---

## Release profile

`[profile.release]` in `Cargo.toml` carries a comment per setting; this is the
reasoning behind the two that are not obvious.

### `lto = "fat"`

The usual advice is `thin`, on the grounds that fat LTO costs a lot for a small
gain. Measured here, that is not what happens:

| | thin | fat |
|---|---:|---:|
| Binary | 22,370,896 B | 20,513,488 B (**−8.3%**) |
| Clean build | 4m 29s | 5m 42s (**+27%**) |
| Peak RSS in the link step | not measured | 2.26 GiB |

73 seconds on a build that happens once per tag, against 1.8 MB off every
download, is a good trade. The 2.26 GiB peak matters because fat LTO is a
single-threaded, whole-program step: it is comfortably inside a 7 GB GitHub
runner, which is the constraint that would otherwise force `thin`.

**Runtime performance was not benchmarked.** Neither LTO mode was compared on
frame time or startup. If someone ever does benchmark it and fat LTO shows no
runtime benefit, the size argument still stands on its own.

### `panic = "unwind"` — the spec's baseline, overridden

The recommended baseline included `panic = "abort"`. This is the closest call
in the whole profile, and the numbers are larger than the usual advice suggests,
so here is the whole thing.

It was **built and launched**, not reasoned about: the resulting binary starts,
opens its window and stays running. Nothing about GPUI, tokio or the objc
bindings makes `abort` inoperable here.

- **What it buys:** 3,472,704 bytes — 17,040,784 against 20,513,488, a **16.9%**
  smaller binary — and 76 seconds off a clean build (4m 26s vs 5m 42s). That is
  not the marginal gain `panic = "abort"` usually produces; a graph this size
  carries a lot of landing pads.
- **What it costs:** dodo's failure isolation. `tokio`'s task harness wraps
  every polled task in `catch_unwind`
  (`tokio-1.53.1/src/runtime/task/harness.rs`, six call sites), which is what
  keeps a panic inside a bollard call — a malformed Docker API response, an
  unwrap on a field the daemon did not send — from taking anything else with
  it. GPUI's background executor similarly confines a panicking task to its own
  thread. With `abort`, each of those becomes an immediate `SIGABRT` of a GUI
  application holding unsaved editor state.

**Rejected**, because the thing it costs is precisely the thing no local test
can check. "It launches" verifies the happy path; the failure path — what
happens when a background task panics against a real Docker daemon returning
something unexpected — cannot be verified without fault injection this project
does not have. Trading an unverifiable robustness property for 3.5 MB on a
20 MB developer tool is the wrong direction.

If that judgement is ever revisited, it is a one-line change with these numbers
attached, and the honest prerequisite is a fault-injection test around
`docker::services` and `api_explorer::services`. The profile states
`panic = "unwind"` explicitly rather than relying on the default, because an
override should be visible where it is made.

Two related notes, since they come up whenever `panic = "abort"` does:

- It would not have broken `cargo test`. `panic` is set on `release`, and cargo
  builds test targets with the `test` profile (inheriting `dev`), forcing
  unwinding for them regardless — so `#[should_panic]` keeps working. dodo has
  250 `#[test]` functions and none of them is `#[should_panic]` today.
- GPUI itself only uses `catch_unwind` in `gpui/src/test.rs`, so the risk is
  about tokio and background tasks, not about the UI framework.

### `strip = "symbols"` and where the symbols go

The shipped binary has no symbol table, which is expected to be the largest
part of the 8.87 MB the first optimization step removes. That conflicts with
wanting symbolicated crash reports later, so the trade is resolved by having
both:

- **`[profile.release]`** — what ships. Stripped, smallest.
- **`[profile.release-debug]`** — `inherits = "release"`, plus `debug = "full"`
  and `split-debuginfo = "packed"`. Identical code and identical optimisation,
  with the debug info emitted *beside* the executable.

That profile was built and checked, not just declared. On macOS it runs
`dsymutil` and produces `target/release-debug/dodo.dSYM` (a symlink into
`deps/`), **277 MB**, next to a 24,729,688-byte unstripped binary. Crucially:

```
$ dwarfdump --uuid target/release-debug/dodo.dSYM
UUID: 6B19F241-174B-32E9-95D1-D0B14E146372 (arm64) …/DWARF/dodo-7e2a36ded21f841d
$ dwarfdump --uuid target/release-debug/dodo
UUID: 6B19F241-174B-32E9-95D1-D0B14E146372 (arm64) target/release-debug/dodo
```

The matching UUID is the whole mechanism — it is what a symbol server keys on,
and it is why a stripped shipped binary and a separately archived `.dSYM` can
still be paired up after a crash. So the future crash-reporting flow is "build
the tag twice, ship the stripped one, archive the `.dSYM`", not "ship symbols
to every user". `docs/release.md` records the steps.

The profile is expensive — 11m 46s clean, because full DWARF for ~500 crates
is not cheap — so use it deliberately. In particular, CI's `cargo-bloat` job
does **not** use it: cargo-bloat only needs the symbol table, so that job sets
`CARGO_PROFILE_RELEASE_STRIP=none` and measures the release profile instead,
which keeps the optimisation identical and the build fast.

---

## Linker optimizations

The short version: **on macOS there was nothing to add**, and the file says so
rather than pretending otherwise.

- `-Wl,-dead_strip` is already in rustc's default Apple link line — verified
  with `rustc --print link-args -O`, which lists it. Confirmed by measurement:
  relinking dodo with `cargo rustc --release -- -C link-arg=-Wl,-dead_strip`
  produced a binary of exactly the same size (22,370,896 B both ways). Adding
  it to `.cargo/config.toml` would have been decoration.
- **Linux**: `lld` is a real win on link time for a binary this size, but it is
  not installed by default, and `.cargo/config.toml` must never make a fresh
  clone fail. It is opted into by CI
  (`.github/actions/linux-build-deps`) through
  `CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUSTFLAGS`, together with
  `--gc-sections` and `--as-needed`. Unverified — nobody has built dodo on
  Linux.
- **Windows (MSVC)**: nothing set. `/OPT:REF` and `/OPT:ICF` are what would go
  there, and rustc is understood to pass them already for an optimized MSVC
  build — but that could not be checked here (`rustc --print link-args` needs
  the target's std installed, and this machine has only the macOS one), and
  there is no Windows machine to measure on. Adding flags would be guesswork.
- `-C target-cpu=native` is deliberately absent: it produces a binary that
  crashes on any machine older than the one that built it, which is exactly
  wrong for a downloadable app.

`.cargo/config.toml` therefore contains no rustflags at all — only
`net.git-fetch-with-cli` (which matters a lot for the zed dependency, see
below) and two aliases, `cargo dist` and `cargo dist-debug`.

---

## Feature flags

Only one feature exists, and it gates real code:

```toml
default = ["syntax-highlighting"]
syntax-highlighting = [
    "gpui-component/tree-sitter",
    "gpui-component/tree-sitter-html",
    "gpui-component/tree-sitter-yaml",
]
```

Turning it off drops three tree-sitter grammar crates and their generated C
parsers: **515,168 bytes, 2.5%** of the shipped binary. The code editor then
renders plain unhighlighted text, which is exactly what gpui-component's
highlighter does when no grammar feature is set — so the degraded state is one
the library already supports, not one this project has to maintain.

### Features deliberately not added

- **`production` / `development` / `profiling`.** Nothing in this crate reads
  them. A feature that gates no code is worse than no feature: it invites
  `--features production` builds that differ from what was tested in name only.
- **`docker`, gating the whole Docker module.** This one is real — it would
  remove `bollard`, `tokio` and `futures-util` and their transitive graph — but
  the module is woven through `src/i18n.rs` (roughly 550 `Str` variants and
  their exhaustive match arms) and `src/i18n_lint.rs` (which `include_str!`s
  the Docker view sources). Gating it means `#[cfg]` on all of that, and the
  i18n guard tests exist precisely to make that surface hard to get wrong. The
  size win does not justify making dodo's most-tested invariant conditional. If
  the module is ever to be optional, do the i18n split first.

---

## Dependency optimization report

600 distinct packages in the normal (non build-, non dev-) dependency graph, by
`cargo tree --edges normal --prefix none | sort -u`. The great
majority arrive through `gpui`, which is not something this repository can
trim: dodo's own direct dependencies are already tight, and the existing
comments in `Cargo.toml` explain each `default-features = false`.

### Already right, and why it should stay that way

| Crate | Choice | Effect |
|---|---|---|
| `reqwest` | `default-features = false`, `rustls`, `blocking`, `http2` | no OpenSSL, no `native-tls`, no system-proxy stack |
| `bollard` | default features only | keeps `hyperlocal` for the unix socket, avoids the `ssl`/`rustls` features the local socket does not need |
| `tokio` | `default-features = false`, `rt-multi-thread`, `net`, `time` | no `fs`, no `process`, no `signal` |
| `gpui-component` | three tree-sitter grammars, not `tree-sitter-languages` | 3 grammars instead of ~35 |
| `futures-util` | already transitive via bollard | no new build cost |

### Findings

**1. `aws-lc-sys` is the most substantial avoidable dependency.**
`reqwest`'s `rustls` feature selects rustls' default crypto provider,
`aws-lc-rs`, which builds `aws-lc-sys` — a large C and assembly cryptography
library with its own cmake build:

```
aws-lc-sys ← aws-lc-rs ← rustls ← { hyper-rustls, reqwest, rustls-platform-verifier, tokio-rustls }
```

The usual alternative is the `ring` provider, which is a smaller library (also
C and assembly, not pure Rust) and builds faster. **Not changed here**,
deliberately: swapping a TLS crypto provider is a security-relevant runtime
change, not a build optimization. It is the first thing to look at if build
time or binary size becomes a problem — and the first thing to do then is
measure what it is actually worth with `cargo-bloat`, which was not run here.
Check the feature names against the reqwest version in `Cargo.lock` before
attempting it; they differ between releases.

**2. Thirteen crates appear at more than one version, all upstream.**
`cargo tree -d --edges normal` lists `bitflags` 1/2,
`getrandom` 0.3/0.4, `hashbrown` 0.16/0.17, `itertools` 0.11/0.13/0.14,
`objc2` 0.5/0.6 (with `objc2-app-kit` and `objc2-foundation` following),
`png` 0.17/0.18, `pollster` 0.2/0.4, `spin` 0.9/0.10 and `thiserror` 1/2.
Every one of them comes from inside the zed / gpui-component graphs and cannot
be resolved from this repository. The value of the CI job that reports them is
noticing a *new* duplicate introduced by something dodo added.

**3. Licensing: three GPL-3.0-or-later crates are linked into the binary.**

```
zlog (GPL-3.0-or-later) ← ztracing (GPL-3.0-or-later) ← sum_tree ← gpui ← dodo
```

`zlog`, `ztracing` and `ztracing_macro` come from `zed-industries/zed`. dodo
itself has no `LICENSE` file. This is not a build question and is not fixed
here, but it has to be answered before binaries are distributed: either dodo
adopts a GPL-3.0-compatible licence, or it stops linking those crates (which
today means not using gpui). `deny.toml` deliberately does **not** carry an
exception for them, so `cargo deny` keeps reporting it.

**4. Nothing unused to remove.** Every direct dependency in `Cargo.toml` is
referenced from `src/`. The one to keep an eye on is `futures-util`, used at a
single call site in `docker::services`.

---

## Reproducibility

The honest scope of the word for this project.

**What is deterministic.** Given the same commit, the same `Cargo.lock`, the
same toolchain and the same target, the build is deterministic in content:
`--locked` everywhere means dependency resolution cannot drift, and
`build_time` — the one wall-clock value in the binary — honours
`SOURCE_DATE_EPOCH`. The release workflow sets it to the tagged commit's own
committer timestamp, so re-running a release for an existing tag embeds the
same string. `scripts/package.sh` additionally passes
`--sort=name --owner=0 --group=0 --numeric-owner --mtime=@$SOURCE_DATE_EPOCH`
when it finds GNU tar, so the archive around the binary is reproducible too
(macOS ships BSD tar, which cannot do this — its archives are not).

**What is not.** Bit-for-bit identical binaries are not claimed. Rust's output
still depends on the absolute path of the build directory and on the exact
toolchain build, and no `--remap-path-prefix` is configured. Nobody has run a
rebuild-and-diff experiment here.

### The rev-pinning problem, and why `Cargo.lock` is the only pin

Four dependencies come from git with **no `rev`**:

```toml
gpui             = { git = "https://github.com/zed-industries/zed" }
gpui_platform    = { git = "https://github.com/zed-industries/zed", features = ["font-kit"] }
gpui-component   = { git = "https://github.com/longbridge/gpui-component" }
gpui-component-assets = { git = "https://github.com/longbridge/gpui-component" }
```

`Cargo.lock` pins them to zed `a1230fc5`, gpui-component `3c270ed2` and
gpui-component-assets `b004e595`. A stray `cargo update` moves all four to
whatever upstream HEAD is at that moment.

The obvious fix — write the revs into `Cargo.toml` — **was attempted and does
not work here.** Three separate cargo errors, in order:

1. Pinning all four: `gpui-component` at `3c270ed2` requires
   `gpui-component-assets` from the *unpinned* URL, which re-resolves to
   current HEAD, so two copies of it enter the graph — and it declares
   `links = "gpui-component-default-icons"`, which cargo forbids duplicating.
   *"package `gpui-component-assets` links to the native library ... but it
   conflicts with a previous package"*.
2. Pinning only `gpui-component`: same conflict, same cause.
3. `[patch."https://github.com/zed-industries/zed"]` with an explicit rev:
   first *"resolved to more than one candidate"* (the zed repo contains two
   `gpui` packages, 0.0.0 and 0.2.2), then, once disambiguated with
   `version = "=0.2.2"`, *"patch for `gpui` points to the same source, but
   patches must point to different sources"* — cargo refuses to patch a source
   to the revision it already resolves to.

The root cause is upstream: `gpui-component`'s own manifest depends on gpui and
on its sibling crates through unpinned default-branch git references. Nothing
in this repository can pin around that.

**So the mitigation is procedural, and it is enforced:**

- `--locked` on every cargo invocation in every workflow and in the
  `cargo dist` alias. A build that may rewrite the lock is a build of
  unreviewed upstream code.
- `Cargo.lock` is committed and is treated as a pin, not as a cache.
- `cargo update` is never run as a side effect of another change. Updating
  those four crates is its own commit, with its own build and manual UI check.

---

## Startup optimization

Reviewed, with no behaviour changed — that was the constraint. `src/main.rs`
does, in order: parse `--version`/`--build-info` and possibly exit,
`gpui_platform::application().with_assets(Assets)`, then inside `app.run`:
`gpui_component::init`, `settings::init`, `api_explorer::init`, `docker::init`,
and finally a spawned `open_window`.

Observations, in the order worth acting on:

1. **`settings::init` parses every vendored theme at launch.** `Assets::themes()`
   iterates the 12 JSON files under `assets/themes` (148 KB) and registers them
   before the window exists. Only one theme is displayed. Deferring the other
   eleven until the settings dialog is opened would move that work off the
   startup path. This is the largest single item and it is a real change to
   initialization order, so it was not made here.
2. **Assets are embedded, which is already the right call.** `rust-embed`
   compiles `assets/` into the binary (224 KB: 148 KB themes, 76 KB icons), so
   there is no filesystem I/O and no path resolution at startup. Splitting them
   out to save binary size would trade a fixed 224 KB for per-launch I/O and a
   new failure mode.
3. **`build_info` costs nothing.** Every value is a `&'static str` from `env!`;
   there is no initialisation to defer because there is none at all.
4. **The tokio runtime is already lazy.** `docker::services` builds it when the
   Docker section is first used, not at launch, and `docker::POLL_INTERVAL`
   only runs while a Docker page is visible (`DockerView::should_poll`).
5. **Collections load off the UI thread already** — `DiskCollectionStore` reads
   `~/Library/Application Support/dodo/` on the background executor.

Measuring first would be the right next step: none of the above is backed by a
startup profile, because none was taken.

---

## Future optimization opportunities

Roughly in order of expected value:

1. **Profile startup**, then act on the theme-loading item above. Everything in
   the previous section is reasoning, not measurement.
2. **`opt-level = "s"` or `"z"` for cold crates.** A per-package profile
   override (`[profile.release.package."*"]`) could optimise the long tail for
   size while keeping `3` for gpui and dodo. Unmeasured; the graph is large
   enough that it might be worth several MB.
3. **Replace the `aws-lc-rs` crypto provider with `ring`**, if build time or
   size becomes pressing — with the security caveat above.
4. **`-C remap-path-prefix`**, if bit-for-bit reproducibility is ever a goal.
5. **`build-std` with `panic_immediate_abort`** would cut more, but it needs
   nightly and it inherits every objection to `panic = "abort"`.
6. **Benchmark before trusting any of this.** dodo has no benchmark harness. A
   startup-to-first-frame measurement would make several of these decisions
   empirical rather than argued.
