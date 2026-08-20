# What dodo keeps on disk

dodo writes **eleven** files. All of them live under `data_dir()`, and each sits behind a trait so the
state layer never learns where it lives. Loading
and saving run on the background executor, never on the UI thread.

## Where the files are

`data_dir()` knows all three platforms: `~/Library/Application Support/dodo`, `%APPDATA%\dodo`, and
`$XDG_CONFIG_HOME` or `~/.config`. **The macOS path is frozen** — changing it orphans every existing
installation's saved collections.

Every *rule* lives in `crates/dodo-paths`, which has no dependencies and deliberately no build
script; it classifies the platform from a **target triple** rather than `#[cfg]`, which is what lets
all three branches be unit tested from a Mac that cannot compile two of them. Copy that trick rather
than a `cfg` split for anything else that is platform-shaped and pure.

**The seam is `src/main.rs`'s own `paths` module**, which is where the one impure input enters:
`build_info::VERSION_INFO.target` is a fact about *this binary*, so it is read there and handed to
the crate. That is also why `HostOs::current()` is `paths::current()` — an inherent method cannot
follow its type across a crate boundary while its body stays behind. Each feature crate that writes
a file has its own `paths` module answering the same question with `cfg!`, because a library crate
is handed no triple; `main.rs`'s tests assert every one of those spellings is the same answer, and
that guard is not decoration — a Cleaner resolving as if it were on Windows would hide half its
categories, and a database `data_dir()` that disagreed would silently lose every saved connection.

## The inventory

| File | Owner | What it holds |
|---|---|---|
| `collections.json` | `api_explorer::services::collection_store` | saved requests and folders |
| `environments.json` | `api_explorer::services::variable_store` | environments and variables |
| `script-consent.json` | `api_explorer::services::consent_store` | the imported scripts the user approved |
| `updater.json` | `updater::services::config_store` | update preferences and the skipped version |
| `connections.json` | `database::services::connection_store` | saved connections — **including passwords in plain text**; see `dodo-database-internals` |
| `query-data.json` | `database::services::query_store` | saved queries and bounded history, query text intentionally plain |
| `quick-nav.json` | `quick_nav::services::config_store` | the editable detection patterns |
| `cleaner-ignored-items.json` | `cleaner::services::ignore_store` | orphan candidates marked "Keep", keyed by absolute path string rather than a `CleanableItemId`, because that id is a session-local hash with no promise of surviving a restart |
| `session.json` | `session::services::session_store` | the session — see below |
| `input-method.json` | `input_method::services::store` | input languages, switch shortcut and Vietnamese engine settings |
| `flow.json` | `flow::services::document_store` | the active Flow Canvas diagram, including shared image resources |

## Versioning: copy one pattern, not the other

The files version differently, and the difference is deliberate.

- `collections.json` is versioned only by `#[serde(default)]`, which copes with *added* fields and
  nothing else. **Do not copy this.**
- Every other file carries an explicit `"version"` from its very first write, and its
  `parse_document` **refuses** a file whose version is higher rather than half-reading it. Copy
  this one.

There is a trap inside the second pattern that `session.json` had to fix for real:
`parse_document` must **stamp** its own `SCHEMA_VERSION` onto what it read. Until it did, a document
loaded from a version-1 file was written straight back *as* version 1, so a newly-added key landed
in a file older builds still believed they understood. `src/session/services/session_store.rs`
carries the fix and the test.

`input-method.json` follows the same rule. Its schema remains at version 8 so existing settings load;
unknown keys from older releases are ignored and disappear on the next save.

## `session.json`

"Nothing is persisted across restarts" is obsolete, and any doc still saying it is stale rather than
describing a decision. The captain asked for session restoration on **2026-08-06**, and
`src/session/mod.rs` is the authority: theme, font size, border radius, language, the window's
rectangle **and mode**, the open tool, the sidebar's collapsed state, and which tools the sidebar
lists at all and in what order.

The other ten files persist something `session.json` does not attempt: *what the user typed or
decided about one specific thing* — an approved script, a saved query, a path marked "Keep", a
skipped update version, an edited pattern — which cannot expire each launch without becoming a lie.
**The one exception is `Run scripts`**, a `ScriptPolicy` global that still starts every launch at
the cautious `Ask for imported`: it is the gate in front of running code that arrived inside someone
else's collection file, not a preference, and its approvals are persisted per script in
`script-consent.json` instead. Do not quietly start persisting it; that is the captain's call.

Four things about the file are decisions rather than details, and each is documented where it is
enforced:

- **Every field is an `Option` and absent means *never chosen*.** Writing `"Default Light"` into a
  fresh file merely because that was on screen would freeze system-appearance following for everyone
  who never opened the dialog. `src/session/models/document.rs` states it.
- **Restoring window geometry opts out of gpui's own placement care.** `Window::new` only cascades
  and clamps in `default_bounds`, the branch it takes when `window_bounds` is `None`, so a supplied
  rectangle goes to the platform unexamined. `src/session/models/geometry.rs` is the replacement and
  is pure, so all of it is tested without a frame.
- **The rectangle is paired with a saved display UUID**, because on macOS the rectangle alone cannot
  say which monitor it meant: every coordinate the pinned gpui reports there is display-*local*
  (`MacDisplay::bounds` returns `(0, 0)` for every display). `models::document::WindowRecord` names
  the four functions that prove it. Do not "simplify" that pairing away.
- **The schema is at version 3.** The Features list took it to 2 and the historical tray language
  field to 3, both for the same reason: an older build would have read the file, dropped the new key
  and written it back pruned.

## The Features list

The Features settings page is why `View::ALL` is no longer the sidebar's order. The captain asked on
2026-08-06 for per-tool on/off plus drag reordering, persisted. `src/session/models/features.rs` is
the authority and is **pure**, so every rule is a unit test rather than something found by looking
at the app — read it there. Four of its consequences reach outside that file:

- A stored entry naming a tool this build lacks is **dropped**; a tool the file never names comes
  back **beside its default neighbour**, enabled.
- **At least one tool always stays visible**, enforced in the model and drawn as a disabled switch
  with the reason beside it.
- The tool on screen is always a listed one, which is the single function `Features::active`
  answering both "the remembered tool was switched off" and "the open tool was just switched off".
- **A switched-off tool is not a quick-navigation route.** `Layout::allowed_detectors` drops its
  detectors before `detect_among` runs, so a pasted `curl` with the API Explorer off falls through
  rather than reopening it. The trap here is that the allowed list is a **membership test and never
  an order**: `Detector::ORDER` is a correctness property (most specific first) and the sidebar's
  order is a preference. `View::for_detector` — generated from each row's `pastes:` field in
  `src/tools.rs` and exhaustive over `Detector` by the compiler — is the single mapping both
  `apply_route` and `allowed_detectors` read.

`settings::features_page` builds its rows by hand because a `SettingItem` cannot carry a position;
`dodo-theming-settings` owns why, and the mechanics of the page.
