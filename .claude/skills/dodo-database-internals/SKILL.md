---
name: dodo-database-internals
description: Deep internals of src/database/ that no single file makes obvious - the one-tree-many-roots connection model, why a connection hover card can never carry a password, DataTable's shared row-height knob, the self-contained-module invariant and its grep check, why the Driver capability set only grows with a control that reads it, the object tree as a driver-answered question rather than a hard-coded ladder, the placeholder-child trick for an unopened tree node, PageBuffer's memory bound and why no LIMIT is ever injected, round 5's write-boundary types, PostgreSQL's binary row decoding, the MySQL/MariaDB driver, the Redis non-SQL driver, round 6's query-data persistence and catalog search, and the plain-text credential posture. Load before touching anything under src/database/.
---

**`src/database/`** is the Database Explorer — create a connection, browse its objects, run a
query, read the result, and safely edit identified SQL rows — **PostgreSQL, SQLite, MySQL/MariaDB
and Redis as of round 6**. Same five-layer split as the other multi-file tools;
**`src/database/mod.rs` is the authority** on the structure, shipped rounds and deliberate cuts.
Fifteen things worth knowing before touching it:

- **The left panel is one tree and the connections are its roots**, not a list stacked on a
  tree. `state::tree::Forest` holds one `CatalogTree` per connection, so selecting a connection
  clears nothing and several are browsable at once; `state::tree::RowRef` qualifies every element
  id by its connection, because two servers routinely produce the same node id. Opening a root
  connects it. The per-connection actions are a right-click menu built with the tree widget's own
  `context_menu`, and **the disclosure chevron is dodo's** — `gpui_component`'s tree draws none.
- **The connection hover card must never carry the password.** `ConnectionProfile::details`
  cannot produce one, `DetailField` has no `Password` variant, and a test asserts both. Same
  posture as the plain-text store below: say what is stored, never put it where a glance or a
  screenshot reaches.
- **`DataTable` has one height knob for the header row and the body rows**
  (`Size::table_row_height`), and each cell is `overflow_hidden` with the size's own vertical
  padding taken out. That is why round 1's two-line header clipped, why the columns carry their
  own zero-vertical `paddings`, and why `views/result_grid.rs` expresses the row height and both
  header lines as multiples of the base text size — so "it fits at 14, 16 and 18px" is arithmetic
  four unit tests check rather than something eyeballed once.
- **It is self-contained, and the invariant is checkable.** No `use crate::` line in the module
  names another tool — only `crate::{database,i18n,app_icon,paths}` — so
  `grep -rn '^use crate::' src/database/ | grep -vE 'crate::(database|i18n|app_icon|paths)'`
  returns nothing. (Other modules are *mentioned* in its doc comments; a pointer is not an edge.)
  The design report proposed a "detect running database containers" prefill on the connection form
  and it was dropped in *every* round, precisely so no compile-time edge exists between two tools.
- **The `Driver` capability set grows only with controls that read it.** Round 2 added `cancel`
  and `explain`; round 3 added `detail` and the DDL source; round 5 adds the optional mutation
  dialect read by the editing controls. `services/mod.rs` states it, and states how a non-SQL
  backend fits without contorting the trait.
- **The object tree is a *question*, not a ladder.** A driver answers "the children of this node";
  nothing above `services/` knows PostgreSQL puts schemas under a database and SQLite does not.
  That is the whole reason a second backend is one file. `models/catalog.rs` also keeps a server's
  identifiers (data, never translated) apart from dodo's own grouping words (translated) in
  `NodeLabel`, which is what gives the i18n guard something to enforce.
- **`state/tree.rs` owns expansion, and the sharp reason is a widget bug waiting to happen**:
  `TreeItem::is_folder` is `children.len() > 0`, so a node whose children have not been fetched
  draws no disclosure triangle and emits no expand event — the tree could never be opened. Every
  expandable node therefore gets a placeholder child until its real children arrive.
- **Object detail reuses the one result grid; it is not another query system.** Double-clicking a
  table or view opens Data / Columns / Indexes / Constraints / DDL in the right pane. View-only
  impossibilities are omitted; SQLite's unenumerable CHECK constraints and PostgreSQL's
  reconstructed DDL are said explicitly. `models/detail.rs` owns the backend-neutral request,
  `state/detail.rs` owns paging and load states, and each driver interprets its own opaque node id.
- **The memory bound is a type, not a comment.** `models/page.rs`'s `PageBuffer` stops the driver
  when rows, total bytes or one cell trips the budget, and **a full page still answers
  `Continue`** — being offered one further row is what proves there was more, which is what makes
  the footer's truncation notice trustworthy. **No `LIMIT` is ever injected into a statement the
  user wrote**. Round 3's table-data pages are different: the backend generates their whole
  `SELECT … LIMIT … OFFSET …` from its opaque catalog id, and advances by rows actually kept so a
  byte-bound page cannot skip data.
- **Round 5's write boundary is a pair of types, not a UI convention.**
  `models::identity::EditableSource` can be constructed only from wire column origins plus a
  catalog-proven primary key or all-NOT-NULL unique index; `models::statement` is the only owner
  of quoted mutation SQL and bound parameters. Changes stay in `state::edit::PendingGrid` until
  the exact batch is shown, then a driver runs it in one transaction and rolls everything back
  unless every statement reports exactly one matched row. MySQL requests `CLIENT_FOUND_ROWS` for
  that reason. Redis, joins/unions/computed columns, missing/truncated keys and nullable unique
  indexes never obtain the token. Concurrent lost updates remain an explicit v1 limitation; do
  not add old-value predicates as a shortcut.
- **Round 2's long work stays server-honest.** Cancel uses PostgreSQL's protocol CancelRequest or
  SQLite's interrupt handle, never task dropping; `services/postgres.rs::live` has the opt-in
  server-side proof. Export re-runs the displayed statement through `services/export.rs`'s
  file-backed sink and keeps one row, never the truncated grid. Query tabs remain session-only;
  round 6 persists the bounded execution history instead of restoring tabs.
- **PostgreSQL rows arrive as binary and `services/postgres.rs` decodes them.** `query_raw`
  streams (the blocking client's `simple_query` would give text for free but materialises the
  whole result, defeating the budget); `numeric` is rendered as text rather than through an `f64`,
  because not rounding is the entire reason a column is `numeric`. Undecodable types fall back to
  UTF-8-as-text — right for an enum, whose binary form *is* its label — or to bytes, and the
  header always carries the server's own `format_type` name. Its `live` test module skips itself
  unless `DODO_PG_TEST_HOST` is set and carries the container command in its doc.
- **MySQL and MariaDB are one driver and one visible engine.** `services/mysql.rs` uses the
  blocking `mysql` client with rustls, streams text-protocol rows, preserves the protocol's
  original table/column metadata, gets DDL from `SHOW CREATE`, and cancels through a second
  connection's `KILL QUERY`. Its opt-in live test is deliberately run against both server images.
- **Redis is a non-SQL driver, not SQL-shaped view code.** `services/redis.rs` owns command-line
  tokenization, reply-to-grid mapping and the generic logical-db → type → key tree. Key browsing
  is one `SCAN … TYPE` cursor page per expansion with a nested More node — never `KEYS` and never
  a full keyspace — and a separate catalog connection keeps browsing from changing the console's
  selected database. Values are fetched only when key detail opens; Redis honestly reports no
  Explain, DDL or cancel capability.
- **Round 6 stores query data, not sessions.** `query-data.json` contains connection-scoped saved
  queries and the newest 200 history entries under a 4 MiB text budget; it never carries a
  connection profile or password. Reopening selects the saved connection only while id, engine
  and target still match, otherwise it opens text with a warning. Global catalog search walks
  connected drivers through the existing `children` seam once, caps calls/nodes, filters its
  in-memory index locally, and follows Redis More nodes rather than querying key leaves.
- **No OS keychain, on any platform, and no `keyring` dependency.** A database password is stored
  the way the API Explorer stores a secret variable: plain text under `data_dir()`, masked in the
  UI, with a notice that is never absent. The report's `CredentialStore` trait is deliberately not
  built — one storage behaviour does not need a trait. `models/connection.rs` states the posture
  and a store test asserts the password really is in the file, so nobody later assumes otherwise.
