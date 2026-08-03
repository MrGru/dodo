# Epic E5 — Database Explorer

**Source:** `src/database/` (`models/`, `services/`, `state/`, `components/`, `views/`)
**Stage:** Phase 3 (Solutioning)
**Status:** Done (round 2 — PostgreSQL and SQLite only), many cuts stated by design

---

Depends on E0 (shell), E7 (i18n), E8 (persistence — `connections.json`, including stored
passwords). Self-contained: `grep -rn '^use crate::' src/database/ | grep -vE
'crate::(database,i18n,app_icon,paths)'` returns nothing.

| Story | Title | Status | Depends on |
|---|---|---|---|
| **E5.1** | One tree, one root per connection (`Forest`/`CatalogTree`/`RowRef`); connection hover card that structurally cannot render a password | Done | E0.1 |
| **E5.2** | `DataTable` with one shared row-height knob for header and body; each cell `overflow_hidden` with its own vertical padding removed | Done | E5.1 |
| **E5.3** | `Driver` capability trait grows only with a control that reads it (Cancel, Explain); object tree is a "children of this node?" question the driver answers, not a hard-coded ladder | Done | E5.1 |
| **E5.4** | `PageBuffer` memory bound (rows, total bytes, single-cell size); a full page still answers `Continue`; no `LIMIT` ever injected into the user's statement | Done | E5.3 |
| **E5.5** | PostgreSQL binary row decoding (not text via `simple_query`, which would materialize the whole result); server-honest Cancel (protocol CancelRequest / SQLite interrupt handle) and PostgreSQL Explain | Done | E5.3 |
| **E5.6** | Plain-text credential store under `data_dir()`, masked in the UI, notice never absent; no `keyring` dependency, no `CredentialStore` trait | Done | E5.1 |
| **E5.7** | Object detail/DDL tabs, editing/CRUD, favorites, pinned queries, persisted history/tab restore, autocomplete, global search, MySQL/Redis, column sorting | **Deliberately not built** | E5.3 |

**AC (E5.4):** Exporting a result re-runs the displayed statement through a file-backed sink and
always exports the full result, never the truncated on-screen page.

**AC (E5.7, deliberate non-goal):** None of these exist because no shipped control reads a
capability that would need them; adding any one is a new epic-scoped decision, not a gap-fill.
