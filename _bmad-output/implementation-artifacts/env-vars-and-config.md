# Environment Variables & Config (Single Source of Truth)

**Source:** direct `grep -rn "env::var" src/` audit against the running code
**Stage:** Phase 4 (Implementation)

---

## Environment variables dodo actually reads

| Name | Read by | Purpose |
|---|---|---|
| `DOCKER_HOST` | `docker/services/engine.rs` | First entry in Docker's numbered engine-discovery order; if set and non-empty, used before falling back to `/var/run/docker.sock`, a macOS `podman machine`, or bollard's Podman defaults |
| `TMPDIR` | `docker/services/engine.rs` | Used to locate a macOS `podman machine`'s actual per-user socket at `$TMPDIR/podman/podman-machine-default-api.sock` |
| `HOME` | `api_explorer/views/explorer.rs`, `database/views/database.rs` | Resolves a sensible default directory for file pickers |
| `DODO_PG_TEST_HOST` | `database/services/postgres.rs` | Opts a PostgreSQL `live` test module into running against a real server; the test module skips itself entirely if unset |
| `DODO_PG_TEST_PORT` | `database/services/postgres.rs` | Companion to `DODO_PG_TEST_HOST` |
| `DODO_PG_TEST_USER` | `database/services/postgres.rs` | Defaults to `postgres` if unset |
| `DODO_PG_TEST_PASSWORD` | `database/services/postgres.rs` | Defaults to empty if unset |
| `DODO_PG_TEST_DB` | `database/services/postgres.rs` | Defaults to `postgres` if unset |

None of these have anything to do with dodo's own runtime configuration — they're either a
discovery input (`DOCKER_HOST`, `TMPDIR`, `HOME`) or a live-integration-test opt-in
(`DODO_PG_TEST_*`, checked in `src/database/services/postgres.rs`'s `live` test module, which
"carries the container command in its doc").

## Persisted configuration (not environment variables)

dodo's actual runtime configuration is five JSON files under `data_dir()` (`src/paths.rs`), never
environment variables:

| File | Owner | Versioning |
|---|---|---|
| `collections.json` | `api_explorer::services::collection_store` | `#[serde(default)]` only — copes with added fields, nothing else |
| `environments.json` | `api_explorer::services::variable_store` | Explicit `"version"`, refuses a newer-than-known file |
| `script-consent.json` | `api_explorer::services::consent_store` | Explicit `"version"`, refuses a newer-than-known file |
| `updater.json` | `updater::services::config_store` | Explicit `"version"`, refuses a newer-than-known file; the one file whose content (`skipped_version`) is meant to survive a restart |
| `connections.json` | `database::services::connection_store` | Explicit `"version"`, refuses a newer-than-known file; also holds database passwords in plain text |

Appearance, font size, border radius, language, and the **Run scripts** `ScriptPolicy` are
deliberately *not* in this list — none of them persist across restarts (see `CLAUDE.md`'s note
that the theming/settings skill's "nothing is persisted" scope excludes only these five files).

## Path conventions

`data_dir()` resolves per platform (`~/Library/Application Support/dodo`, `%APPDATA%\dodo`,
`$XDG_CONFIG_HOME`/`~/.config`), classified from `build_info::VERSION_INFO.target` rather than
`#[cfg]` — see `src/paths.rs` and `architecture-decisions.md` ADR-027.
