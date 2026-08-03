# Epic E6 — In-app Updater

**Source:** `src/updater/` (`models/`, `services/`, `state/`, `components/`, `views/`)
**Stage:** Phase 3 (Solutioning)
**Status:** Done

---

Depends on E0 (shell), E8 (persistence — `updater.json`, the one persisted file that survives a
restart on purpose, because of `skipped_version`). Consumes the `update.json` manifest E9
publishes.

| Story | Title | Status | Depends on |
|---|---|---|---|
| **E6.1** | Check silently at startup; ask before downloading — structural, since `services::pipeline::check` is never handed a `Downloader` | Done | E0.1 |
| **E6.2** | macOS install swap as two renames with an explicit rollback, not a `renamex_np(RENAME_SWAP)` call | Done | E6.1 |
| **E6.3** | Verification is integrity only (SHA-256 from the same HTTPS origin), never authenticity; a downloaded file is never executed; hand-written `sha256.rs`/`version.rs` rather than `sha2`/`semver` | Done | E6.1 |

**AC (E6.1):** A user who never clicks "Check for updates" never has a file downloaded on their
behalf.

**AC (E6.3):** Refusing to install (unwritable `/Applications`, read-only volume, a bare binary) is
treated as success — "downloaded, install manually" plus the archive's path — not an `Err`. Only a
broken archive or a half-failed rename is an `Err`.
