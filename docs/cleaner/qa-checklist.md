# Cleaner manual QA, macOS compatibility, and release-readiness

This file is the ticket's required "manual QA checklist", "macOS compatibility matrix" and
"release-readiness checklist" deliverables, combined. Every row states **how** it was verified —
"Ran" (an actual command/build/launch happened this session), "Read" (code/logic verification without
execution), or "Not run" (needs a human, or a session with more capability, before shipping) — the
same honesty convention `.github/workflows/ci.yml`'s own header uses for what has actually run.

## What was actually run this session (Phase 17)

| Check | Result |
|---|---|
| `cargo fmt --all -- --check` | Ran — clean, every commit this project |
| `cargo clippy --all-targets --locked -- -D warnings` | Ran — clean, every commit this project, zero suppressions added |
| `cargo test --locked` | Ran — 1381 passed, 0 failed |
| `cargo build` (debug) | Ran — succeeds |
| `cargo check --locked` (native host target) | Ran repeatedly across every phase |
| Launching the built binary as a plain executable | Ran — window opens, renders (see below) |
| Mach-O fat-binary parsing against a real universal binary on this machine | Ran — `object` crate's parsed architectures/sizes for `/Applications/Firefox.app/Contents/MacOS/firefox` matched `lipo -archs` exactly (`x86_64`, `arm64`) |
| Filesystem safety properties: component-based containment, `/tmp/foo` vs. `/tmp/foobar`, root-deletion rejection, home/`/Applications`/`/System` protection, symlink rejection, symlink-swap-between-scan-and-clean (TOCTOU), parent-child dedup, hard-link accounting, unicode/newline filenames, mount-boundary logic, missing roots, cancellation | Ran — dedicated unit tests for every one, all passing |

## What was launched and visually confirmed, and what was not

- **The app launches and renders.** `./target/debug/dodo` was run directly, produced a real GPUI
  window (confirmed via `screencapture` + reading the resulting image), and the in-app updater
  correctly detected an available update, downloaded and verified it, then correctly refused to
  self-replace because it was running as a bare executable rather than from an app bundle — the exact
  behavior `docs/release.md` documents. This is a strong incidental signal that unrelated app
  machinery (updater, window management) still works with Cleaner's code present.
- **Clicking through the Cleaner sidebar entry, switching categories, and triggering a real scan was
  not done this session.** This sandbox has no Accessibility/UI-scripting permission granted to
  `osascript`/System Events (`osascript is not allowed assistive access`), so no click or keystroke
  could be sent to the running window programmatically. **This is the one required check that needs a
  human at a real keyboard (or a session with Accessibility permission) before release** — everything
  else in this file was verified without that dependency.
- No screenshot of Cleaner's own UI (Smart Care, a category's result list, the uninstall-review
  dialog, the permission banner) exists from this session, for the same reason.

## macOS compatibility matrix

| macOS capability Cleaner depends on | Status |
|---|---|
| `objc2`/`objc2-foundation`/`objc2-app-kit` (Trash move, Finder reveal, running-app check) | Compiles and links on this Apple Silicon host; not exercised at runtime this session (see above) |
| Full Disk Access detection (`macos::permissions`) | Read — implemented per the ticket's real-read-access-probe design (`~/Library/Mail`, `~/Library/Safari`, etc.), not exercised against a real TCC prompt this session |
| `docker` CLI (Docker Cache scanner) | Read — argument-vector invocation, `--format '{{json .}}'` parsing; not exercised against a real Docker daemon this session (would need one installed and running) |
| `codesign`, `/usr/bin/codesign --verify` (Universal Binaries scanner) | Read — same as above, not exercised against a real signed/unsigned binary pair this session |
| `.GlobalPreferences.plist` reads (Language Files scanner) | Read — the file exists on every macOS install; not exercised against this machine's real preferences this session |
| Apple Silicon (arm64) vs. Intel (x86_64) host | This session ran only on Apple Silicon. `current_architecture()`'s `x86_64`/`i386`/`arm` branches are exercised by unit test, not by an actual Intel host |
| macOS version range | Not tested against any specific OS version boundary — the ticket's suggested `LSMinimumSystemVersion`/`UnsupportedMacOsVersion` handling exists as a type (`ScanError::UnsupportedMacOsVersion`) but nothing in the current 14 scanners actually constructs it, since none of them has hit a version-gated API yet |

## Release-readiness checklist

- [x] No new crate, package, or workspace member — verified via `cargo metadata --no-deps` package
      count convention already established for `tools/update-manifest`; Cleaner adds zero entries to
      that list.
- [x] `cargo fmt`/`clippy`/`test` clean at every commit this session (11 commits, each independently
      verified before committing).
- [x] Every deletion path (Trash move, Docker CLI prune) requires explicit user selection and a
      confirmation dialog naming what will happen — no automatic deletion exists anywhere, including
      inside Smart Care.
- [x] Allow-list deletion model enforced end-to-end; protected paths (`/`, home, `/Applications`,
      `/System`, `/Library`, `/Users`, `/Volumes`, `/private`, `/bin`, `/sbin`, `/usr`, dodo's own
      data/config dirs) verified by a real-policy test, not a synthetic one.
- [ ] **Interactive UI walkthrough by a human** (or a session with Accessibility permission) — the one
      item this session could not complete; see above.
- [ ] **Docker Cache scanner exercised against a real running Docker daemon** — implemented and unit
      tested with synthetic CLI output, never run against `docker` itself.
- [ ] **Full Disk Access grant/deny/retry flow exercised against a real TCC prompt** — implemented per
      the ticket's real-read-access-probe design, never clicked through.
- [ ] **Phase 16 (real architecture/language removal) does not exist** — by design; see
      `docs/cleaner/architecture.md`'s "What's deferred".
- [ ] **No literal performance benchmarks** (traversal strategy comparisons, worker-count sweeps,
      memory profiling under load) — would need a real-hardware run against real large directory
      trees; see `docs/cleaner/known-limitations.md`.
- [ ] **No result-table virtualization** — a real UI rewrite target for a future pass, not attempted
      here; see `docs/cleaner/known-limitations.md` and `architecture.md`.

Items with `[ ]` are not blockers for merging this work-in-progress state, but are the concrete list a
maintainer should work through before calling Cleaner release-ready.
