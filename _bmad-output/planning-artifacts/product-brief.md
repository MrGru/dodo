# Product Brief — dodo

**Source:** `CLAUDE.md`, `README.md`, `src/`
**Stage:** Phase 1 (Analysis)

> This file previously described "Cowork Local v3.0" (CoworkAthon), an unrelated Python project
> that has no connection to this repository. It has been rewritten to describe `dodo`. See
> `../CHANGELOG.md` for that history.

---

## 1.1 Problem statement

Developers routinely reach for a handful of small, single-purpose tools during everyday work:
pretty-printing a blob of JSON, base64/URL/hex-decoding a token, poking an HTTP API by hand,
checking on a Docker/Podman container, or running an ad-hoc SQL query against a database. Each of
these exists as a separate web tool, CLI, or heavyweight GUI (Postman, DBeaver, Docker Desktop),
usually cloud-connected, slow to start, or bundled with far more than the task needs.

`dodo` is a single native desktop app that puts several of these developer tools behind one
sidebar, each self-contained, so switching between "format this JSON" and "check this container's
logs" and "run this query" doesn't mean switching apps.

## 1.2 Why this shape

- **GPUI** (Zed's UI framework) gives a native, GPU-rendered UI without an Electron/web-view
  runtime, and `gpui-component` supplies a ready widget set (sidebar, inputs, tables, dialogs,
  theming) so each tool can focus on its own logic rather than re-inventing basic widgets.
- Both are pulled from git rather than crates.io and are pinned **only** by `Cargo.lock` — there is
  no stable released version to depend on, which shapes several decisions recorded in
  `architecture-decisions.md` (why `cargo update` must never run as a side effect of another task,
  why an explicit `rev =` pin was tried and abandoned).
- Every tool is a self-contained module; the sidebar and main pane (`src/layout.rs`) know nothing
  about a tool's internals beyond its `View` variant. Most tools are a single `src/<tool>.rs` file;
  the three that outgrew that (`api_explorer`, `docker`, `database`) share one `models/ → services/
  → state/ → components/ → views/` split, documented per-module in their own `mod.rs`.

## 1.3 Target users and use cases

- **UC-1** — A developer pastes an ugly one-line JSON payload into the JSON Formatter and gets it
  pretty-printed at a chosen indent width, with a parse error shown inline as a diagnostic if the
  input is malformed.
- **UC-2** — A developer has a JWT, a base64 blob, or a URL-encoded string and needs to inspect or
  convert it via the Encoder/Decoder, including splitting a JWT into header/payload/signature
  (decode-only — no signature verification).
- **UC-3** — A developer is integrating against an HTTP API: they build a request in the API
  Explorer (method, URL, params, headers, body, auth, a pre-request/post-response script), send it,
  and read the response (status, timing, size, headers, syntax-highlighted body, cookies, test
  results, console output) — optionally importing a Postman/Insomnia collection or pasting a `curl`
  command to rebuild the request, or exporting the request back out as `curl`/`fetch`/`axios`/XHR
  code.
- **UC-4** — A developer with containers running locally opens the Docker page to see status,
  live CPU%, ports and start times across Containers/Images/Volumes/Networks, drills into an
  Inspect dialog or a log viewer, and starts/stops/restarts/deletes a container without leaving the
  app.
- **UC-5** — A developer needs to browse a PostgreSQL or SQLite database's schema, run a query, and
  read the result set (with server-honest Cancel and, for PostgreSQL, Explain), then export what
  they see to CSV or JSON.
- **UC-6** — Any of the above, kept current: an in-app updater checks for a new release, asks
  before downloading, verifies the download's integrity, and installs it without the user leaving
  the app or hunting down a new archive by hand.

## 1.4 Strategic positioning

dodo does not compete with Postman, DBeaver, or Docker Desktop feature-for-feature; it competes on
being **one lightweight native binary** that covers the slice of each tool a developer reaches for
most often, with no telemetry, no account, and no network dependency beyond what the tool itself
talks to (the target API, the local Docker socket, the target database).

| Axis | Postman / Insomnia | DBeaver / TablePlus | Docker Desktop | **dodo** |
|---|---|---|---|---|
| Runtime | Electron / web-view | JVM / native | Electron + VM | **Native GPUI, no web-view, no VM** |
| Scope | HTTP client only | DB client only | Container mgmt only | **JSON, encode/decode, HTTP, Docker, DB — one sidebar** |
| Account / cloud sync | Often required | Optional | Account nudges | **None — everything local** |
| Distribution | Installer, auto-update via vendor infra | Installer | Installer + background VM | **Single binary; in-app updater reads a static `update.json`** |
| License | Proprietary | Mixed | Proprietary | **dodo's own code MIT; open question on GPL-3.0-or-later transitively via `gpui`** |

## 1.5 Success signals

dodo has no telemetry and no external success metrics pipeline (there is no server component to
report to) — "success" here is qualitative and verified by hand or by test, not by a dashboard:

| Signal | How it's actually checked today |
|---|---|
| A tool works end to end | Manual run per `dodo-build-validate`'s guidance, plus the tool's own unit tests |
| No regression on a change | `cargo fmt --all`, `cargo clippy --all-targets --locked -- -D warnings`, `cargo test` — all blocking in CI |
| A release is installable on the platform it targets | `docs/release.md`'s "What 'verified' means" — the honest record of what has and hasn't actually run on a real Windows/Linux host |
| The updater's whole pipeline works | Manual walk: check → download → verify → install → restart, since there is no automated end-to-end test across a real GitHub Release |
| i18n coverage | Two `cargo test` guards: no bare string literal in view code, no untranslated literal reaches the screen |

## 1.6 Out of scope (deliberate, not "not yet")

These are stated cuts, each with the reason recorded in the owning module's doc comments — not a
backlog waiting for time:

| Area | What's cut | Why (see) |
|---|---|---|
| Docker | Exec/Terminal, Create/Pull/Build, Stats beyond live CPU%, Favorites | `src/docker/mod.rs` — each is a literal disabled "coming soon" control, not an absent feature |
| Database Explorer | Object detail/DDL tabs, editing/CRUD, favorites, pinned queries, persisted history/tab restore, autocomplete, global search, MySQL/Redis, column sorting | `src/database/mod.rs` — capability set grows only with a control that reads it |
| Database Explorer | Injecting a `LIMIT` into a statement the user wrote | `models/page.rs` — bounding happens at the sink, never by rewriting the query |
| API Explorer | OAuth 2.0 auth type | `services/http/auth.rs` / `views/request_auth.rs` — the one remaining `later_step()` placeholder in the codebase |
| Updater | Signature verification of the downloaded archive | `services/verify.rs` — integrity (SHA-256 from the same HTTPS origin) only, not authenticity |
| Distribution | A settled answer on GPL-3.0-or-later binaries | `THIRD-PARTY-NOTICES.md` — deliberately left open, not decided by omission |
| Everywhere | A second database backend beyond PostgreSQL/SQLite, a workspace/project concept, cloud sync, telemetry, accounts | No shipped control anywhere reads any of these; adding one is a new epic, not a hidden default |

## 1.7 Constraints (do not relax without recording why)

- **`Cargo.lock` is the only pin** on `gpui`, `gpui_platform`, `gpui-component` and any other
  git dependency. Never run `cargo update` as a side effect of another task.
- **Each tool stays self-contained.** `src/database/` in particular has a checkable invariant:
  `grep -rn '^use crate::' src/database/ | grep -vE 'crate::(database,i18n,app_icon,paths)'`
  returns nothing.
- **Every user-facing string goes through `Str`**, never a bare literal in view code — enforced by
  a `cargo test` guard, not just a convention.
- **`fmt` and `clippy` are blocking.** `cargo fmt --all` and
  `cargo clippy --all-targets --locked -- -D warnings` must be clean before committing; no
  crate-level `#[allow]`.
- **No file read on the UI thread.** Anywhere a tool needs disk or network I/O (API Explorer body
  upload, Docker's bollard calls, PostgreSQL/SQLite queries), it happens on the background
  executor.
- **No `--release` build runs on push.** `cargo check` per platform plus one debug build is the
  push-time gate; the four-platform release matrix is weekly/manual/tag-only, and that accepted
  cost (release-only failures surface up to a week late) is stated, not hidden.
