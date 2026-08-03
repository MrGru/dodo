# Epic E8 — Persistence & `data_dir()`

**Source:** `src/paths.rs` and each store's own service file
**Stage:** Phase 3 (Solutioning)
**Status:** Done

---

Cross-cutting: E3 (`collections.json`, `environments.json`, `script-consent.json`), E5
(`connections.json`), and E6 (`updater.json`) each own one of the five persisted files.

| Story | Title | Status | Depends on |
|---|---|---|---|
| **E8.1** | `data_dir()` per platform (`~/Library/Application Support/dodo`, `%APPDATA%\dodo`, `$XDG_CONFIG_HOME`/`~/.config`), classified from `build_info::VERSION_INFO.target` rather than `#[cfg]` so all three branches are unit-testable from one host | Done | E0.1 |
| **E8.2** | Five persisted files, each behind a trait, load/save on the background executor never the UI thread; `collections.json` tolerates only added fields (`#[serde(default)]`), the other four carry an explicit `"version"` and refuse a file whose version is higher | Done | E8.1 |

**AC (E8.1):** The macOS path is frozen — changing it would orphan every existing installation's
saved collections; do not change it as a side effect of an unrelated refactor.

**AC (E8.2):** `updater.json` is the one persisted file that is not appearance/session-scoped —
because a "skip this version" that expired every launch would make the Skip button a lie.
