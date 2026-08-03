# Architecture Decision Records — dodo

**Source:** `CLAUDE.md` and the module doc comments it points to
**Stage:** Phase 3 (Solutioning)

> These decisions were extracted from `CLAUDE.md` and the source tree, not invented for this
> document — `CLAUDE.md` remains the authority if this list and the code ever disagree.

---

## Full ADRs (the ones most likely to be relitigated)

### ADR-001 — `Cargo.lock` is the only possible pin on the four git dependencies

**Status:** Accepted.

**Context:** `gpui`, `gpui_platform`, `gpui-component` (and one more) are pulled from git rather
than crates.io, with no published version to pin. An explicit `rev = "…"` pin was tried and could
not work: upstream depends on itself through unpinned default-branch refs, producing three
recorded cargo errors (`docs/build-optimization.md`).

**Decision:** `--locked` everywhere; `cargo update` only ever as its own reviewed commit, never as
a side effect of another task.

**Consequence:** A first build after cloning fetches exactly what `Cargo.lock` says; nobody may
"just bump" a dependency without deliberately reviewing the diff.

### ADR-002 — Three modules share a five-layer split; the rest stay a single file

**Status:** Accepted.

**Context:** `api_explorer`, `docker`, and `database` each outgrew a single `src/<tool>.rs`.

**Decision:** `models/` (plain data, unit-tested, no GPUI) → `services/` (the only place naming
the outside-world crate) → `state/` → `components/` → `views/`. Copy this split when a fourth tool
outgrows one file; do not invent a different shape.

**Consequence:** A fake `Transport`/`Engine`/`Driver` implementation is all that's needed to unit
test the pipeline above it without a network, daemon, or real database.

### ADR-003 — `services/` is the only place naming the outside-world crate

**Status:** Accepted.

**Context:** `reqwest` (API Explorer's `Transport`), `bollard` (Docker's `Engine`), `postgres`/
`rusqlite` (Database's `Driver`) are each named only inside their module's `services/`.

**Decision:** Anything above `services/` programs against the trait, never the concrete crate.

**Consequence:** Adding a second database backend, or swapping the HTTP client, is a new
`services/` file plus a trait impl — not a rewrite of `models/`/`state/`/`views/`.

### ADR-004 — A script's provenance is a property of the request, not of the text

**Status:** Accepted.

**Context:** `RequestSnapshot::script_origin` is set to `Imported` on import and never changed by
editing; editing changes the content hash instead, which re-arms the consent gate. One
`ConsentKey` covers both the pre-request and post-response hook together.

**Decision:** An imported script cannot be laundered past the consent gate by copy-pasting its
text into a "new" script, and approving one hook never silently approves the other.

**Consequence:** Any future script-editing feature must preserve `script_origin` across edits and
must not split the consent key per hook.

### ADR-005 — Pre-request and post-response script failure fail in opposite directions

**Status:** Accepted.

**Context:** A failed pre-request script stops the send (a half-configured request produces a
response nobody can reason about). A failed post-response script must not lose the response — the
request already happened and the response is the evidence needed to fix the script — so it becomes
a Console line plus the Tests tab's error banner while `SendOutcome::result` stays `Ok`.

**Decision:** Keep this asymmetry; do not "simplify" post-response failure into also aborting.

**Consequence:** A broken post-response script degrades gracefully instead of hiding a response the
user already paid the network round-trip for.

### ADR-006 — No `LIMIT` is ever injected into a statement the user wrote

**Status:** Accepted.

**Context:** `database/models/page.rs::PageBuffer` stops the driver when rows, total bytes, or one
cell trips the budget; a full page still answers `Continue`.

**Decision:** Bounding happens at the sink (the page buffer), never by rewriting the SQL text.

**Consequence:** The footer's truncation notice stays trustworthy — the user always ran exactly the
statement they wrote, and "there's more" is proven by the driver actually offering another row.

### ADR-007 — No OS keychain, on any platform, for a stored password

**Status:** Accepted.

**Context:** A database password (and an API Explorer secret variable) is stored the way both
already agree to: plain text under `data_dir()`, masked in the UI, with a notice that is never
absent.

**Decision:** No `keyring` dependency, no per-platform keychain integration, and no `CredentialStore`
trait (one storage behavior does not need a trait).

**Consequence:** A store test asserts the password really is in the file, so nobody later assumes
otherwise; any future secret-bearing feature follows this same honest posture rather than inventing
a false sense of security.

### ADR-008 — `rusqlite` and `sqlx` cannot be in the same dependency graph

**Status:** Accepted (as a constraint, not a choice).

**Context:** Both declare `links = "sqlite3"` through `libsqlite3-sys`, at versions that do not
overlap (`rusqlite 0.40` needs `0.38`, `sqlx 0.9` needs `>=0.30.1, <0.38`); cargo refuses to
resolve a graph containing two packages linking the same native library.

**Decision:** A "sqlx for network backends, rusqlite for SQLite" split is not viable unless the
versions are pinned to a compatible pair; this cost the design round one failed build.

**Consequence:** Any future non-SQL/second-SQL backend choice must check this constraint first,
not discover it via a build failure.

### ADR-009 — The GPL-3.0-or-later distribution question is left explicitly open

**Status:** Open (deliberately, not by omission).

**Context:** `gpui -> sum_tree -> ztracing -> zlog` pulls GPL-3.0-or-later into every build.
dodo's own source is MIT.

**Decision:** `deny.toml` carries no `allow` or `exceptions` entry for those crates, so
`cargo deny` keeps reporting them; `THIRD-PARTY-NOTICES.md` records the verified chain and states
the question is unresolved.

**Consequence:** Nobody may write a conclusion about binary distribution into the repo without
actually resolving the question; `cargo deny` stays noisy on purpose as a reminder.

---

## Summary of the remaining decisions

| ADR | Title | Where enforced |
|---|---|---|
| ADR-010 | Sending is one background job — script, `prepare`, and the request together; no file is ever read on the UI thread | `api_explorer/state/tab.rs::send`, `services/file_picker.rs`, `services/http/upload.rs` |
| ADR-011 | `pm.test`/`pm.expect` are written in JavaScript (the `PRELUDE`), not Rust bindings, so no `__`-prefixed global exists to find | `api_explorer/services/script/quickjs.rs` |
| ADR-012 | The `Eval` QuickJS intrinsic must be included — it registers no global itself, but `ctx.eval` cannot run without it | `api_explorer/services/script/quickjs.rs` |
| ADR-013 | A script's syntax check uses the same engine that will run it (QuickJS compile-only flag), not a second parser | `api_explorer/state/tab.rs` |
| ADR-014 | The Format action beside the script editor is not a JavaScript formatter — only re-indents/normalizes blank lines | `api_explorer/models/script_format.rs` (a real formatter measured +2.8 MB / 12.7%) |
| ADR-015 | Code generation (`services/codegen/`) and `curl` parsing (`services/curl.rs`) are separate modules sharing no code, joined only by a round-trip property test over the wire request | `api_explorer/services/codegen/` |
| ADR-016 | Generated code withholds a `secret`-marked variable (emitted as the literal `{{name}}`) but withholds nothing else, including secrets typed directly into the Auth tab | `api_explorer/services/codegen/mod.rs` |
| ADR-017 | Docker engine discovery is hand-rolled per platform because bollard's defaults miss a dangling `/var/run/docker.sock` and a macOS `podman machine`'s actual socket path | `docker/services/engine.rs` |
| ADR-018 | `docker::services` is the only place naming `bollard`, and the only place dodo constructs a tokio runtime (PostgreSQL's client builds its own separate private one) | `docker/services/mod.rs`, `database/services/postgres.rs` |
| ADR-019 | The sidebar is flat — Docker's four pages live on its own vertical rail, not a nested sidebar group, because an icon-collapsed sidebar renders no children | `docker/mod.rs`, `docker/views` |
| ADR-020 | A modal overlay must be `window.open_dialog`, never a hand-rolled scrim — a `div` scrim doesn't cover the whole window and a no-op mouse handler doesn't block clicks | `docker/views/detail.rs` |
| ADR-021 | The Database Explorer's left panel is one tree with a connection as each root, not a list stacked on a tree | `database/state/tree.rs::Forest` |
| ADR-022 | The object tree is a *question* ("children of this node?"), not a hard-coded ladder — a driver answers it, so a second backend is one file | `database/services/mod.rs` |
| ADR-023 | Checks silently, asks before downloading — structural: `pipeline::check` is never handed a `Downloader` | `updater/services/pipeline.rs` |
| ADR-024 | The macOS swap is two renames with a rollback, not one atomic `renamex_np(RENAME_SWAP)` call, to avoid a `libc` dependency for one platform's one call | `updater/services/installers/swap.rs` |
| ADR-025 | Verification is integrity, not authenticity — the digest comes from the same HTTPS origin as the archive; a downloaded file is never executed | `updater/services/verify.rs` |
| ADR-026 | `sha256.rs`/`version.rs` are hand-written rather than depending on `sha2`/`semver`, since `sha2` is currently only a build dependency | `updater/models/{sha256,version}.rs` |
| ADR-027 | `data_dir()` classifies platform from `build_info::VERSION_INFO.target`, not `#[cfg]`, so all three platform branches are unit-testable from one host | `src/paths.rs` |
| ADR-028 | Persisted-file versioning differs by design: `collections.json` copes with added fields only (`#[serde(default)]`); the other four carry an explicit `"version"` and refuse a newer-than-known file | `src/paths.rs` area |
| ADR-029 | No `--release` build runs on push; the four-platform release matrix is weekly/manual/tag-gated, accepting that release-only failures surface up to a week late | `.github/workflows/ci.yml`, `docs/release.md` |
| ADR-030 | `tools/update-manifest` is a standalone crate excluded from the workspace (`exclude = ["tools/*"]`), costing the shipped binary zero bytes | root `Cargo.toml`, `docs/release.md` |
