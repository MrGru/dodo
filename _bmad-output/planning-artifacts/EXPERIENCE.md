# Experience Spine — dodo

**Source:** `CLAUDE.md`
**Stage:** Phase 2/3 — behavioral spine (flows, modes, state)

## Per-tool experience

### JSON Formatter

Paste JSON, pick an indent width, get it pretty-printed; an invalid input shows its parse error
inline as a diagnostic rather than a blank result or a dialog.

### Encoder / Decoder

Pick a codec (Base64 standard/URL-safe, URL percent-encoding, Hex) and a direction; a JWT switches
to a three-pane split (header/payload/signature) — decode-only, no signature check is performed or
implied.

### API Explorer

1. Open or create a request tab; each tab owns its own method/URL/params/headers/body/auth/scripts.
2. Send (Cmd/Ctrl+Enter or the Send button) runs one background job: pre-request script → `{{name}}`
   resolution → `prepare` → transport execute → post-response script, in that order.
3. A pre-request script failure stops the send outright. A post-response script failure never
   drops the response — it becomes a Console line and a Tests-tab error banner while the response
   itself still renders normally.
4. An imported script's hooks are gated by consent (`ScriptPolicy`, default "Ask for imported" every
   launch) before they run automatically; editing the script re-arms the gate by changing its
   content hash.
5. Pasting a `curl` command into the URL box rebuilds the whole request into a new tab (or the
   current tab if untouched) — guarded against firing on a keystroke that merely contains the word
   "curl".
6. Generated code (`curl`/`fetch`/`axios`/XHR) never emits a variable marked `secret` in its
   resolved form; a token typed directly into the Auth tab is not protected the same way, and the
   dialog says so.

### Docker

1. Containers/Images/Volumes/Networks are separate pages on Docker's own vertical rail (not nested
   sidebar items).
2. Exactly the visible page polls, every 5 s, and stops the instant the section is no longer active.
3. Row actions (Start/Stop/Restart/Delete) act immediately except Delete, which confirms first.
4. Inspect and container logs open in a `window.open_dialog`, never a same-page overlay.
5. Exec/Terminal, Create/Pull/Build, deeper Stats, and Favorites render as visibly disabled
   controls with a tooltip — the user can see they exist and that they're not available yet,
   rather than the feature being silently absent.

### Database Explorer

1. Each connection is a root in one shared tree; opening a root connects it; several connections
   can be open and browsed at once.
2. A connection's hover card never shows its password — there is no code path that could render
   one even by mistake (`DetailField` has no `Password` variant).
3. Running a query streams into a memory-bounded page; hitting the bound still shows "more rows
   available" rather than silently truncating without saying so.
4. Cancel and Explain are server-honest: Cancel uses the real protocol-level cancellation (or
   SQLite's interrupt handle), never just dropping the async task; Explain runs the database's own
   EXPLAIN, only where the driver actually supports it (PostgreSQL today).
5. Export re-runs the displayed statement through a dedicated file-backed sink and always exports
   the full result, never the on-screen truncated page.

### Updater

1. dodo checks silently at startup; nothing downloads without an explicit click, by construction
   (the check path is never handed a `Downloader`).
2. Download → verify (SHA-256 against the same HTTPS origin) → install → restart, or, if install
   isn't possible (unwritable install location, read-only volume, bare binary), a "downloaded,
   install manually" state with the archive's path — never a bare error for something that isn't
   actually one.

## Behavioral constraints that must not regress

- The sidebar's collapse/expand interaction must keep its current toggle behavior across any
  layout-touching change.
- A permission/consent decision made for one script hook must never silently cover the other hook.
- `Undo`-shaped or destructive actions (Docker Delete, any future Database Explorer edit feature)
  require an explicit confirm step, never a silent one-click.
- A tool's "coming soon" control stays visibly present and disabled — it must not vanish, and it
  must not quietly start working without its own story and tests.
