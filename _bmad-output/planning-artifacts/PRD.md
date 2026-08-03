# PRD — dodo

**Source:** `CLAUDE.md`, module `mod.rs` doc comments, `docs/build-optimization.md`, `docs/release.md`
**Stage:** Phase 2 (Planning)

---

## 2.1 Functional requirements (FR) — by epic

| FR | Description | Epic | Status |
|---|---|---|---|
| **FR-0.1** | Single centered 900x620 window with a collapsible sidebar; selecting a sidebar item swaps the main pane | **E0** | Done |
| **FR-0.2** | Closing the window quits the app via `QuitMode::LastWindowClosed`, plus a macOS-only `cmd-w` binding (dodo installs no menu bar) | **E0** | Done |
| **FR-0.3** | `dodo --version` / `--build-info` print embedded build metadata and exit before any window opens | **E0** | Done |
| **FR-1.1** | Pretty-print pasted JSON at a chosen indent width | **E1** | Done |
| **FR-1.2** | Show a parse error inline as a diagnostic when input is invalid | **E1** | Done |
| **FR-2.1** | Base64 (standard + URL-safe), URL percent-encoding, Hex — both directions | **E2** | Done |
| **FR-2.2** | JWT inspector: split header/payload/signature (decode-only, no signature verification) | **E2** | Done |
| **FR-3.1** | Request tabs with method/URL/query params/headers/body/auth/scripts; async send (Cmd/Ctrl+Enter or button) | **E3** | Done |
| **FR-3.2** | Response view: status badge, timing, size, headers, syntax-highlighted body, cookies, tests, console | **E3** | Done |
| **FR-3.3** | Pre-request and post-response scripting in a sandboxed QuickJS engine, with `pm.test`/`pm.expect` | **E3** | Done |
| **FR-3.4** | Script consent gating for imported scripts, keyed by hook content hash | **E3** | Done |
| **FR-3.5** | Saved, importable (Postman/Insomnia) request collections | **E3** | Done |
| **FR-3.6** | Paste a `curl` command into the URL box to rebuild the whole request; generate `curl`/`fetch`/`axios`/XHR code from a request | **E3** | Done |
| **FR-3.7** | OAuth 2.0 as a fifth auth type (Basic/Bearer/API Key already ship) | **E3** | Planned (placeholder) |
| **FR-4.1** | Containers page: list, colored status, live CPU%, ports, relative start time, search, Start/Stop/Restart/Delete | **E4** | Done |
| **FR-4.2** | Images/Volumes/Networks list pages, compose grouping, filters, bulk actions | **E4** | Done |
| **FR-4.3** | Inspect dialog for all four resource types; container log viewer | **E4** | Done |
| **FR-4.4** | Exec/Terminal, Create/Pull/Build, Stats beyond CPU%, Favorites | **E4** | Deliberately not built (disabled controls) |
| **FR-5.1** | One tree with a connection per root; PostgreSQL and SQLite drivers | **E5** | Done |
| **FR-5.2** | Run a query, read a memory-bounded result page, Cancel (server-side), Explain (PostgreSQL) | **E5** | Done |
| **FR-5.3** | Export the displayed statement's full result to CSV/JSON via a re-run against the driver | **E5** | Done |
| **FR-5.4** | Searchable, session-only query history | **E5** | Done |
| **FR-5.5** | Object detail/DDL tabs, editing/CRUD, favorites, pinned queries, persisted history, autocomplete, global search, MySQL/Redis, column sorting | **E5** | Deliberately not built |
| **FR-6.1** | Check for updates (manual button + silent auto-check at startup); download, verify (SHA-256), install, restart | **E6** | Done |
| **FR-6.2** | Refuse-to-install falls back to "downloaded, install manually" rather than erroring | **E6** | Done |
| **FR-7.1** | Settings dialog: appearance, font size, border radius, language, Run-scripts policy | **E7** | Done |
| **FR-7.2** | Every user-facing string routes through `Str`; language switch applies live | **E7** | Done |
| **FR-8.1** | Five files persisted under `data_dir()`, each behind a trait, on the background executor | **E8** | Done |
| **FR-9.1** | CI: per-platform `cargo check` + one debug build on push; weekly/manual/tag-gated release matrix | **E9** | Done |
| **FR-9.2** | Every release publishes a signed-by-nothing, integrity-checked `update.json` manifest via a standalone `tools/update-manifest` crate | **E9** | Done |
| **FR-9.3** | Application icon pipeline (`scripts/generate-icons.py`) produces committed `.icns`/`.ico`/hicolor PNGs | **E9** | Done |

## 2.2 Non-functional requirements (NFR)

| NFR | Target | Verification |
|---|---|---|
| **NFR-1 i18n completeness** | No bare string literal reaches a view; no untranslated literal reaches the screen | Two `cargo test` guards (see `dodo-i18n-text` skill) |
| **NFR-2 Lint/format cleanliness** | Zero `clippy` warnings under `-D warnings`; `cargo fmt --all` clean | Blocking CI jobs, no crate-level `#[allow]` |
| **NFR-3 Script sandbox bound** | 2 s deadline, 16 MiB heap, 256 KiB per-value cap per script run; one fresh QuickJS runtime per run | `src/api_explorer/services/script/quickjs.rs` |
| **NFR-4 No UI-thread file/network I/O** | File picker `stat`, body upload reads, bollard calls, PostgreSQL/SQLite queries all run on the background executor | `services/file_picker.rs`, `services/http/upload.rs`, `docker/services/engine.rs`, `database/services/{postgres,sqlite}.rs` |
| **NFR-5 Result-set memory bound** | A page stops on row count, total bytes, or single-cell size; never on an injected `LIMIT` | `database/models/page.rs::PageBuffer` |
| **NFR-6 Docker poll cadence** | Exactly one visible page polls, every 5 s, only while its section is active | `docker::POLL_INTERVAL`, `DockerView::should_poll` |
| **NFR-7 Persisted-file versioning** | `environments.json`/`script-consent.json`/`updater.json`/`connections.json` carry an explicit `"version"` and refuse a newer-than-known file; `collections.json` copes only with added fields via `#[serde(default)]` | `src/paths.rs` area, each store's `parse_document` |
| **NFR-8 Credential storage honesty** | A database or API Explorer secret is stored in plain text under `data_dir()`; the UI notice saying so is never absent | `database/models/connection.rs`, `api_explorer` secret-variable docs |
| **NFR-9 Cross-platform `data_dir()` correctness** | Correct path on macOS/Windows/Linux, unit-testable from one host via `build_info::VERSION_INFO.target`, not `#[cfg]` | `src/paths.rs` |
| **NFR-10 Windows GUI-subsystem console handles** | `--version`/`--build-info` still produce visible output on a GUI-subsystem Windows binary | `attach_parent_console` in `src/main.rs` |
| **NFR-11 Release verification honesty** | `docs/release.md` states exactly what has and hasn't run on a real host per platform, never claims more | `docs/release.md` "What 'verified' means" |

## 2.3 Definition of Done (per story/round, not a single ship gate)

dodo ships continuously rather than toward one deadline, so there is no single "ship gate" —
each round of work against an epic is done when:

- [ ] The feature's acceptance criteria (per its story in `implementation-artifacts/stories/`) are met.
- [ ] `cargo fmt --all` and `cargo clippy --all-targets --locked -- -D warnings` are clean.
- [ ] `cargo test` passes, including the i18n guard tests if any user-facing text changed.
- [ ] Any new user-facing text was added through `Str`, per `dodo-i18n-text`.
- [ ] Any new setting was wired through `dodo-theming-settings`'s pattern (applies live, or is
      explicitly stated as requiring restart).
- [ ] For a UI change, the feature was actually run per `dodo-build-validate` — not just type-checked.
- [ ] Anything the module's own `mod.rs` states as a deliberate cut was not silently reintroduced.

## 2.4 Risks and mitigations

| ID | Risk | Mitigation | Where recorded |
|---|---|---|---|
| **RISK-1** | The two Apple git dependencies (`gpui`, `gpui-component`) drift upstream since nothing but `Cargo.lock` pins them | Never run `cargo update` except as its own reviewed commit | `CLAUDE.md`, `docs/build-optimization.md` |
| **RISK-2** | The Windows release path has never actually run on a Windows host | `docs/release.md`'s "What 'verified' means" states this honestly rather than assuming green-CI means verified | `docs/release.md` |
| **RISK-3** | `rusqlite` and `sqlx` cannot coexist in the dependency graph (both link `libsqlite3-sys` at incompatible versions) | Documented so a future "sqlx for network backends, rusqlite for SQLite" idea isn't attempted blind | `CLAUDE.md`, `docs/build-optimization.md` |
| **RISK-4** | A GPL-3.0-or-later chain (`gpui → sum_tree → ztracing → zlog`) reaches every build | `THIRD-PARTY-NOTICES.md` records the chain and leaves distribution posture explicitly open; `deny.toml` has no `allow`/`exceptions` entry so `cargo deny` keeps surfacing it | `THIRD-PARTY-NOTICES.md`, `deny.toml` |
| **RISK-5** | A script (pre-request/post-response) escapes its sandbox or hangs | Positive intrinsic allowlist, 2 s deadline, 16 MiB/256 KiB caps, one fresh runtime per run, `pm.sendRequest` denied by not existing | `services/script/quickjs.rs` |
| **RISK-6** | A database or API Explorer password is trivially visible on disk | UI notice is never absent about plain-text storage; connection hover cards cannot render a password by construction (`DetailField` has no `Password` variant) | `database/models/connection.rs` |
| **RISK-7** | Two `cargo check` targets (Linux, Windows) cannot run natively from a macOS dev machine | Documented as a fact of `aws-lc-sys`'s C build script, not a portability bug in dodo; the two Apple targets cross-check locally instead | `docs/release.md` |
| **RISK-8** | A `LIMIT` silently injected into a user's query would misrepresent what "no more rows" means | `PageBuffer` bounds at the sink; a full page still answers `Continue` rather than falsely implying completeness | `database/models/page.rs` |
