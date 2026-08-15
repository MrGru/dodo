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

Two features exist, and both gate real code:

```toml
default = ["syntax-highlighting", "sql-highlighting"]
syntax-highlighting = [
    "gpui-component/tree-sitter",
    "gpui-component/tree-sitter-html",
    "gpui-component/tree-sitter-yaml",
    "gpui-component/tree-sitter-javascript",
]
sql-highlighting = ["gpui-component/tree-sitter", "gpui-component/tree-sitter-sql"]
```

Turning `syntax-highlighting` off drops four tree-sitter grammar crates and
their generated C parsers. The three that were there before the JavaScript
grammar were measured at **515,168 bytes, 2.5%** of the shipped binary; the
JavaScript grammar's own cost is measured below. The code editor then
renders plain unhighlighted text, which is exactly what gpui-component's
highlighter does when no grammar feature is set — so the degraded state is one
the library already supports, not one this project has to maintain.

**`sql-highlighting` is separate on purpose**, and the next section is why: it
costs more than every other grammar in this crate put together, several times
over, and folding it into `syntax-highlighting` would hide that behind a flag
nobody would think to question. Two features that could have been one is the
price of keeping the trade visible and reversible.

### What the SQL grammar costs

**+2,460,432 bytes, +10.79%** — measured, not estimated, as the difference
between two release builds of the same commit that differ only in this feature.

| Change | Δ bytes | Δ % | Note |
|---|---:|---:|---|
| `tree-sitter-javascript` (shipped) | +363,440 | +1.65% | the Scripts tab's two editors |
| **`sql-highlighting` (shipped)** | **+2,460,432** | **+10.79%** | the Database Explorer's query editor |
| `dprint-plugin-typescript` (**rejected**) | +2,829,280 | +12.7% | a real JavaScript formatter |

So the SQL grammar is **6.8× the JavaScript grammar** and **87% of what this
project refused to pay for JavaScript formatting**. The cause is not subtle and
is not something a build flag can shrink: `tree-sitter-sequel`'s generated
`parser.c` is **17,383,606 bytes** against `tree-sitter-javascript`'s
**2,487,319** — a 7.0× ratio, against a 6.8× binary-size ratio.

It ships anyway. The captain was shown this number, the comparison above and a
recommendation to ship without it, and chose to take it: a database client's
query editor is the one place in dodo where the user *writes* the language
rather than reading what a server sent, and colour there is worth 2.4 MB. The
condition attached was this row and the separate feature.

Two things that make the trade reversible rather than permanent:
`--no-default-features --features syntax-highlighting` produces a 25,907,616-byte
binary whose query editor still has a gutter, selection, multi-cursor, search
and soft wrap — the library's own graceful default, not a broken state — and
`database::models::engine::editor_language` is the single place that decides
what grammar the editor asks for.

A note on the measurement, because it is unusually clean for a fat-LTO build:
the design round predicted +2,460,448 B for this grammar from an isolated spike
on a different tree, and the shipped feature measures **16 bytes** from that.
The control (`a8cd40d`, 22,805,152 B) also reproduced byte-for-byte across the
two rounds. Deltas below ~15 KB on this graph are still noise; these are not.

### What the script engine costs

Measured on the same machine and profile, immediately before and after the
round that added it, so the two numbers are comparable to each other and not to
the table above (which was taken on an older tree):

| Configuration | Bytes | Δ | Δ % |
|---|---:|---:|---:|
| Control — the commit before the round | 20,811,216 | — | — |
| With `rquickjs`, the sandbox, the consent gate and the Console | 21,975,776 | **+1,164,560** | **+5.60%** |

That is the **whole feature**, not the engine alone: it includes roughly 2,600
lines of new Rust and 34 new `Str` variants in both languages. The scouting
report measured `rquickjs` on its own at +999,024 B (+4.9%) against its own
control, which is consistent with this once the rest of the round is accounted
for. For scale, the two alternatives it measured were `rhai` at +1,906,032 B —
nearly twice QuickJS, for a language no imported Postman script is written in —
and `boa_engine` at +5,806,400 B.

It is **not** behind a cargo feature, for the reason the next section gives
about `docker`: gating it would mean `#[cfg]` on the `Str` variants and on the
send path, and the i18n guard tests exist to make that surface hard to get
wrong. `services/script/` is nonetheless the only module that names `rquickjs`,
so a build without an engine is a `NullEngine` swap in one line rather than a
refactor.

### What the post-response hook and the editor polish cost

The round after that one added the post-response hook and the Tests tab, and
made the two script editors habitable: JavaScript syntax highlighting, a
debounced parse check that underlines errors in place, and a Format action.
Measured on the same machine and profile, against the same control the table
above ends on:

| Configuration | Bytes | Δ vs control | Δ % |
|---|---:|---:|---:|
| Control — `f57d68e`, the commit before the round | 21,975,776 | — | — |
| Control **+ the JavaScript grammar alone** (`Cargo.toml` one-line change, nothing else) | 22,339,216 | **+363,440** | **+1.65%** |
| The whole round | 22,440,864 | **+465,088** | **+2.12%** |

The formatter was then isolated on its own by stubbing
`models::script_format::format` to the identity and rebuilding:

| Configuration | Bytes | Δ |
|---|---:|---:|
| The whole round | 22,440,864 | — |
| …with `script_format::format` stubbed to the identity | 22,424,352 | **−16,512** |

One warning about reading numbers this small off a fat-LTO build. The stubbed
row above was taken in a second build directory and came out byte-identical to
the same code built in `target/`, so the build directory itself costs nothing —
but an intermediate build of this branch measured 22,455,024, and the only
difference was two placeholder strings getting four words shorter. Fourteen
kilobytes moved because of that, which is nearly as much as the formatter costs
in total. Inlining decisions shift under LTO for reasons that have nothing to do
with the size of the edit, so measure the tree you actually mean to ship and do
not attribute a swing of this magnitude to whatever you happened to change.

Three numbers worth separating, because they were the ones in doubt:

- **Syntax highlighting is the single largest line item: +363,440 B (+1.65%)**,
  one `tree-sitter-javascript` grammar and its generated C parser. That
  confirms the scouting report's estimate almost exactly (+363,440 B, +1.77%
  against its own smaller control). It is the price of the feature working at
  all; there is no cheaper grammar.
- **The formatter costs 16,512 B — 0.07%, three and a half percent of what
  highlighting cost**, because it is a couple of hundred lines of Rust over
  `&str` with no dependency at all.
- **Everything else in the round** — the post-response hook, `pm.test` /
  `pm.expect` and their JavaScript prelude, the Tests tab, the diagnostics
  plumbing and 15 new `Str` variants in two languages — is the remaining
  ~85,100 B.

The formatter number is the one that justified a judgement call. The obvious
way to get a *real* JavaScript formatter is `dprint-plugin-typescript`, and it
was measured rather than guessed: it does not build at all at `0.93` in this
graph (`swc_common 0.37.5` fails on `use serde::private`), and at `0.95.15` it
builds and costs **+2,829,280 B, +12.7%** on top of the grammar build. That is
five times what the entire rest of this round cost, on a binary that had
already grown 5.6% for the script engine, to pretty-print a text box most users
will paste into. So dodo ships the modest option — reindent, normalise blank
lines, leave everything else byte-for-byte — and `models/script_format.rs`'s
module doc states plainly what it does not do. If a future round wants real
formatting, that measurement is the number to argue with.

### What code generation cost

The round after that one added the Generate code dialog: four emitters (cURL,
`fetch`, `axios`, `XMLHttpRequest`), the shared normalized form they all read,
the dialog itself, and 9 new `Str` variants in both languages. **It added no
dependency at all** — three of the four targets are string generation and the
fourth reuses `percent-encoding`, which was already in the graph for
`request_body`. So this row is what roughly 2,400 lines of new Rust (about half
of it tests) costs on its own, with no new crate anywhere in it:

| Configuration | Bytes | Δ vs control | Δ % |
|---|---:|---:|---:|
| Control — `74726c6`, the commit before the round | 22,440,864 | — | — |
| The whole round | 22,507,008 | **+66,144** | **+0.29%** |

Both rows were built on the same machine, from the same `target/`, with
`cargo build --release --locked`. The control was rebuilt from `74726c6` after
the round rather than taken from the previous section's table, and came out
byte-identical to it — which is worth knowing on its own: on this graph the
release build is reproducible to the byte across a branch switch, so a delta
this small is signal rather than noise. Read the warning at the end of the
previous section anyway before attributing it to any one part of the round; on a
fat-LTO build inlining decisions move by tens of kilobytes for reasons unrelated
to the size of the edit.

**+66,144 B is the cheapest round the API Explorer has had**, an order of
magnitude below the two before it (+1,164,560 and +465,088), and the reason is
visible in the diff: those rounds each added a crate — `rquickjs`, then a
tree-sitter grammar — and this one adds none. It is a data point for the
project's standing bias: new Rust is nearly free at this scale, and new
dependencies are not.

### What the update manifest cost

**Nothing. Zero bytes, byte-identical binary.** The round that added the release
manifest generator put it in `tools/update-manifest`, a standalone crate with its
own `Cargo.toml` and `Cargo.lock`, and added `exclude = ["tools/*"]` to dodo's
`[package]`. Nothing in it is compiled by, linked into, or lints alongside the
application.

| Configuration | Bytes | SHA-256 |
|---|---:|---|
| Control — `8f05fc7`, the commit before the round | 22,507,024 | `ad5413e7c9fef795…` |
| The whole round | 22,507,024 | `ad5413e7c9fef795…` |

Both rows are genuine compiles on the same machine and `target/` with
`cargo build --release --locked`; the control was forced with `touch
src/main.rs` so cargo could not simply report it up to date, and the two came out
identical to the byte. This is the one kind of row where the fat-LTO noise
warning above does not apply — there is no delta to attribute, because the
compiler was handed the same input twice.

Two secondary confirmations, since "it is a separate crate" is the whole claim:
`cargo metadata --no-deps` at the repo root lists `dodo` as the only package and
one workspace member (as of that round — the repo gained a `[workspace]` with a
second member, `crates/dodo-ime-core`, on 2026-08-08; `update-manifest` is still
not one of them), and reverting the `exclude` key did not cause cargo to
rebuild at all — `exclude` is packaging metadata and not part of the build
fingerprint.

The precedent this sets is worth naming: **release engineering goes in
`tools/`, not behind a feature flag.** A `[[bin]]` on dodo or a
`cfg(feature)`-gated module would both have put the code in the crate and made
"does it ship?" a question about build flags. A separate crate makes the answer
structural. `docs/release.md`, "Automatic updates", is the authority on what the
tool does.

### What the in-app updater cost

The round after that one added the consumer side: `src/updater/` (the manifest
parse, SemVer and channels, an incremental SHA-256, `updater.json`, the state
machine, the pipeline, three platform installers and the dialog), a
cross-platform `src/paths.rs`, and 40 new `Str` variants in both languages.
About 6,000 lines, roughly half of them tests. **It added no dependency.**

| Configuration | Bytes | Δ vs control | Δ % |
|---|---:|---:|---:|
| Control — `e0829ee`, the commit before the round | 22,507,024 | — | — |
| The whole round | 22,805,120 | **+298,096** | **+1.32%** |

Both rows are genuine compiles on the same machine and `target/` with
`cargo build --release --locked`; the control was forced with `touch
src/main.rs`. +298 KB is comfortably above the ~14 KB of noise a fat-LTO build
shows for a trivial edit, so the number is signal — but do not attribute it to
any single part of the round for the reasons the previous section gives.

**It is the second-biggest dependency-free round the project has had**, four and
a half times code generation's +66,144 B, and the shape of the code explains it:
unlike the four pure string emitters that round added, this one pulls in
`std::process::Command`, a second `reqwest::blocking::Client` construction path,
`std::fs` rename/metadata/read_dir across three installers, and 80 new `match`
arms in `Str::text`. Every one of those is monomorphised into a binary built
with `codegen-units = 1`.

Two things it deliberately did **not** cost:

- **No `sha2`.** It is in `Cargo.lock` only as a build dependency of
  `rust-embed-utils`, so nothing of it is linked today; taking it would have
  been a genuinely new runtime dependency. `models/sha256.rs` is ~120 lines
  against the NIST FIPS 180-4 vectors. Same reasoning for `semver` and
  `models/version.rs`, which additionally needed a channel policy no crate
  supplies.
- **No `flate2` / `tar` / `zip`.** Extraction shells out to the operating
  system's own `tar`, which reads both formats dodo publishes and exists on all
  three platforms.

The test doubles cost nothing either: unlike
`consent_store::InMemoryConsentStore`, which is `#[allow(dead_code)]` because it
doubles as a runtime fallback, the updater's four are `#[cfg(test)]` and are not
compiled into the shipped binary at all.

### What the Database Explorer cost

Round 1 of `src/database/`: the five layers, the `Driver` trait, the PostgreSQL
and SQLite drivers, `connections.json`, the lazy object tree, the query editor
and the bounded result grid — roughly 7,000 lines, about half of them tests,
plus 82 new `Str` variants in both languages. **It is by far the largest single
addition this project has taken, and the only round to add five dependencies.**

| Configuration | Bytes | Δ vs control | Δ % |
|---|---:|---:|---:|
| Control — `a8cd40d`, the commit before the round | 22,805,152 | — | — |
| The round, **without** `sql-highlighting` | 25,907,616 | **+3,102,464** | **+13.60%** |
| The round as it ships, **with** `sql-highlighting` | 28,368,048 | **+5,562,896** | **+24.39%** |

All three are genuine `cargo build --release` compiles on the same machine and
the same warm `target/`, with `SOURCE_DATE_EPOCH` pinned so `build.rs`'s
embedded timestamp cannot move bytes between rows.

Where the +3,102,464 goes, in rough order:

- **The drivers.** The design round measured `postgres` + its rustls connector
  + `rusqlite`/bundled, exercised from a real entry point, at **+2,275,472 B**
  on this same control. `rusqlite`'s `bundled` feature is the single biggest
  line item in that: it compiles the whole SQLite C amalgamation into the
  binary. That is deliberate — the alternative is linking the host's
  `libsqlite3`, whose version varies by platform and which is absent on Windows
  — and it is the lever to examine first if this number ever has to come down.
- **`sqlformat`**, measured by that round at **+165,232 B** for a real SQL
  formatter. Two genuinely new crates; `winnow` and `memchr` were already here.
- **The rest — roughly 660 KB — is dodo's own code**, and it is much more than
  the ~70 KB a round of new Rust has cost before. That is not a surprise at
  this size: it is 82 new `match` arms in `Str::text` (in two languages), a
  `TableDelegate` and a `TreeState` feed monomorphised against gpui's element
  types, two drivers' worth of catalog SQL, and a hand-written binary decoder
  for PostgreSQL's wire format — all through `codegen-units = 1`.

Two costs the round deliberately did **not** pay:

- **No `keyring`.** Not on any platform. A database password is stored the way
  the API Explorer already stores a secret variable, so the round adds no
  credential backend, no `security-framework` on macOS, and — the one that
  mattered — none of the 97-crate `secret-service` subtree and its D-Bus daemon
  requirement on Linux.
- **No `sqlx`.** The design round measured the same four backends through it at
  **+3,087,472 B**, which is 248,880 bytes *more* than the native crates for a
  strictly smaller feature set: `sqlx` exposes no query-cancel API at all, and
  its compile-time-checked-query macros — the reason to use it — are exactly
  the part a client running user-typed SQL against an unknown database cannot
  use.

### What the system tray cost

`src/tray/`: the macOS menu bar item, its native menu, the keyboard-input-language
switcher and its `session.json` key — about 900 lines including tests, three SVG
marks, and three new `Str` variants in both languages.

| Configuration | Bytes | Δ vs control | Δ % |
|---|---:|---:|---:|
| Control — `bbf9e43` | 30,502,496 | — | — |
| The same tree plus the round | 30,687,120 | **+184,624** | **+0.61%** |

Both are `cargo build --release --locked` on the same machine and the same warm
`target/`. `SOURCE_DATE_EPOCH` is not pinned here and does not need to be: the
only wall-clock value `build.rs` embeds is a fixed-width ISO 8601 string, so its
value can move but its length cannot.

**The control is not this round's parent commit**, and that is deliberate rather
than stale. The round was measured in isolation on `bbf9e43` and then rebased
onto the cleaner round that landed alongside it, so these two rows still isolate
what the tray costs — the same five packages and the same code — without mixing
in the several MB that arrived from an unrelated direction. Re-measuring against
the merge base would answer a different question.

**Five new packages, and that is where nearly all of it goes**: `tray-icon`,
`muda`, `crossbeam-channel`, `dpi` and `keyboard-types`. `once_cell`,
`thiserror` and `png` were already in the graph, and `futures-channel` — the
`Send + Sync` seam that carries menu events into a gpui foreground task — is a
promotion of an existing transitive dependency, so it costs nothing. The
`objc2-app-kit` feature set widens considerably (`NSStatusBar`, `NSStatusItem`,
`NSStatusBarButton`, `NSMenu`, `NSMenuItem`, `NSButton`, `NSCell`, `NSControl`,
`NSView`, `NSEvent`, `NSTrackingArea`, …), but features are additive across a
graph and a hand-written `NSStatusBar` would need essentially the same list, so
that part is not attributable to the crate choice.

The captain accepted this on 2026-08-07 knowing the number, the same way
`sql-highlighting` was accepted. The alternative that was measured against it —
calling `NSStatusBar`/`NSStatusItem` through the `objc2` crates dodo already
depends on — costs **zero** new packages, and was rejected anyway on two
grounds: it is a few hundred lines of `unsafe` responder-chain code that no CI
on this project can exercise (the release workflow proves a binary runs by
executing `--version` on a headless runner, which opens no status item), and it
would have to be written twice more from scratch for Windows and Linux, which
`tray-icon` already implements. `TrayIcon::ns_status_item()` keeps the raw
`NSStatusItem` reachable, so nothing is locked away by the choice.

Two costs the round deliberately did **not** pay:

- **No new icon assets in the packaged output, and no PNGs at all.** The three
  marks are SVGs under `assets/icons/tray/`, which `src/assets.rs`'s existing
  `icons/**/*.svg` filter already embeds, rasterised at runtime through gpui's
  own `SvgRenderer`. `scripts/generate-icons.py` is stdlib-only — its own PNG
  codec and box filter, no PIL, no ImageMagick — so it could not have drawn a
  glyph even if PNGs had been wanted.
- **No icon cache.** Rasterising a mark is one SVG parse at 36×36 and happens
  only when the user picks a different language from the menu. A
  `HashMap<LanguageId, Icon>` would be state to keep correct in exchange for
  time nobody can perceive.

### Features deliberately not added

- **`production` / `development` / `profiling`.** Nothing in this crate reads
  them. A feature that gates no code is worse than no feature: it invites
  `--features production` builds that differ from what was tested in name only.
- **`docker`, gating the whole Docker module.** This one is real — it would
  remove `bollard`, `tokio` and `futures-util` and their transitive graph — but
  the module is woven through the string catalogue (roughly 550 `Str` variants and
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

`zlog`, `ztracing` and `ztracing_macro` come from `zed-industries/zed`. dodo's
own source is now MIT (`LICENSE`), which settles what dodo's source is and
nothing more: **the terms under which a built binary combining it with
GPL-3.0-or-later object code may be distributed remain undecided.** That open
question, and the verified chain above, are recorded in
`THIRD-PARTY-NOTICES.md`. `deny.toml` deliberately carries no `allow` entry and
no exception for those three crates, so `cargo deny` keeps reporting them.

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
