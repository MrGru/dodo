# Project agent memory

`dodo` is a Rust desktop app: a single window with a collapsible sidebar, where each sidebar
entry swaps the main pane to a self-contained developer tool (JSON formatter, Encoder/Decoder,
API Explorer, Docker, Database Explorer, Cleaner) plus, in the sidebar footer, a Settings dialog and a
**Check for updates** dialog. It is built on GPUI (Zed's UI framework) and the `gpui-component`
widget library, both pulled from git and pinned only by `Cargo.lock`. See `README.md` for the
user-facing description and `Cargo.toml` for exact dependency sources.

Read `src/main.rs` for the startup sequence and `src/layout.rs` for the view model; the doc
comments there are the authority on structure. This file is only a map — for anything below the
map's resolution, load the matching skill from the table below rather than reading a whole module
cold.

**`_bmad/`, `_bmad-output/` and `bmad.config.yaml` are tracked, and are not the authority for
anything.** They are bmad scaffolding kept for contributors who work that way; the repo owner does
not, and decided on 2026-08-05 that they stay — so their presence is settled, not an oversight.
Authority is this file, the skills it indexes, each module's `mod.rs` doc comments and `docs/`.
`_bmad-output/` reads authoritative and is not: its PRD, epics, per-story files and
`sprint-status.yaml` are not kept in step with what actually lands. A session that is not
deliberately running a bmad workflow should not read, follow or update any of it — in particular
it must not mark a story or `sprint-status.yaml` to reflect work it just did.

`src/main.rs` also owns **app lifecycle**, and both halves are counter-intuitive enough to name
here: a release Windows build is a **GUI-subsystem** binary (no console window behind the app),
which costs it valid standard handles, so `attach_parent_console` buys them back on the
`--version` / `--build-info` path alone — and **no shell waits for a GUI-subsystem process at
all**, so every Windows smoke test must run it through `Start-Process -Wait`, never PowerShell's
`&`. Capturing into a variable does *not* make the shell wait; believing it did is what cost
v0.1.5 its Windows archive, and the same wrong claim sat in three comments and one doc before
being corrected. "What 'verified' means" in `docs/release.md` has the panic, the run IDs and the
fix. Closing the single window quits the
app through **`QuitMode::LastWindowClosed`** (GPUI's own check, run after the window is removed,
not a callback that force-quits) plus a macOS-only `cmd-w` binding, needed because dodo installs
no menu bar for that shortcut to hang off. The doc comments there carry the reasoning;
`docs/release.md` records that the Windows half has never run on a Windows host.

Most tools are a single `src/<tool>.rs`. **`src/api_explorer/`, `src/docker/` and
`src/database/` are the exceptions** and the pattern to copy when a tool outgrows one file:
`models/` (plain data, no GPUI, unit tested), `services/` (the trait that is the only place naming
the outside-world crate), `state/`, `components/`, `views/`. Each module's own `mod.rs` doc
comments are the authority on its split and what shipped when; the matching skill below is where
the non-obvious parts of each are written down — load it before changing anything in one of these
three modules rather than inferring the design from the files cold.

**`src/cleaner/` is the same shape but started unfinished, and rounds have been landing since.**
Its `mod.rs` doc comment is the authority on what has shipped; do not assume it is still round 1
without checking. `core/` still carries `#[allow(dead_code)]` on items ahead of what constructs
them — **those are pending work, not dead code to delete** — but the allow comes off as each
producer lands: `core::safety` now has a real `validate_path` and a moved-to-trash cleanup path
(`macos::cleanup::cleanup_items`), so its allow is already gone. `core::permissions` is still the
one whole-module allow marking an area that does not exist at all yet.

**"`render` only runs when something changed" is false in gpui, and it cost the Cleaner its frame
rate.** A dirty view marks its whole *ancestor* path dirty, and an ancestor re-rendering sets
`Window::refreshing`, which bypasses the element cache for every *descendant* — so a child view
scrolling, a 120 ms progress tick, or a redraw anywhere above re-runs a view's `render` with
nothing of its own changed. Any `render` that copies a whole collection is therefore paying that
copy per frame, however well the rows themselves are virtualized: `src/cleaner/views/results_sync.rs`
is the fix, the measurements and the decision table, and is the pattern to copy — stamp a revision
where the data is mutated, compare it before re-copying. Cheap `render` bodies are not an
optimisation in dodo; they are the contract.

**dodo persists ten things across restarts, and reads an eleventh another process writes**, all
under `data_dir()` (`src/paths.rs`) and each
behind a trait so the state layer never learns where they live: `collections.json`
(`api_explorer::services::collection_store`), `environments.json` (`services::variable_store`),
`script-consent.json` (`services::consent_store`, the imported scripts the user has approved),
`updater.json` (`updater::services::config_store`), `connections.json`
(`database::services::connection_store`, which also holds database passwords in plain text — see
`dodo-database-internals`), `query-data.json` (`database::services::query_store`, saved queries
plus bounded query history, with query text intentionally stored as plain text), `quick-nav.json`
(`quick_nav::services::config_store`), `cleaner-ignored-items.json`
(`cleaner::services::ignore_store`, the orphan-detection candidates the user has marked "Keep",
keyed by absolute path string rather than a `CleanableItemId` since that id is a session-local
hash with no promise of surviving a restart), `session.json`
(`session::services::session_store`) — and `input-method.json`
(`input_method::services::store`, the macOS input method's engine settings and selected
keyboard language).
The eleventh is `input-method-status.json`, which **dodo only reads**: the input-method process
writes it, and `dodo-ime-ipc`'s single-writer rule is why dodo has no method that could. Persistence
and initial load run on the background executor, never the UI thread.

**`session.json` is what makes "nothing is persisted across restarts" obsolete**, and any doc still
saying it — including the `dodo-theming-settings` skill — is stale rather than describing a
decision. The captain asked for session restoration on 2026-08-06, and `src/session/mod.rs` is the
authority: theme, font size, border radius, language, the window's rectangle **and mode**, the open
tool, the sidebar's collapsed state, and **which tools the sidebar lists at all and in what order**.
The other nine files persist something `session.json` does not attempt: *what the user typed or
decided about one specific thing* — an approved script, a saved query, a cleaner path marked
"Keep", a skipped update version, an edited quick-nav pattern — which cannot expire each launch
without becoming a lie. **The one exception is `Run scripts`**, a `ScriptPolicy`
global that still starts each launch at the cautious `Ask for imported` — it is the gate in front of
running code that arrived inside someone else's collection file, not a preference, and its approvals
are persisted per script in `script-consent.json` instead. Do not quietly start persisting it; that
is the captain's call.

Two things about that file that are decisions rather than details. **Every field is an `Option` and
absent means *never chosen*** — writing `"Default Light"` into a fresh file merely because that was
on screen would freeze system-appearance following for everyone who never opened the dialog. And
**restoring window geometry opts out of gpui's own placement care**: `Window::new` only cascades and
clamps in `default_bounds`, the branch it takes when `window_bounds` is `None`, so a supplied
rectangle goes to the platform unexamined. `session::models::geometry` is the replacement — clamp to
a display that still exists, honour `layout::window_min_size`, centre on the preferred display when
the saved one is gone — and it is a pure function so all of it is tested without a frame. It is
paired with a saved display **UUID**, because on macOS the rectangle alone cannot say which monitor
it meant: every coordinate the pinned gpui reports there is display-*local* (`MacDisplay::bounds`
returns `(0, 0)` for every display). `models::document::WindowRecord` names the four functions that
prove it. Do not "simplify" that pairing away.

**The Features settings page is why `View::ALL` is no longer the sidebar's order.** The captain
asked on 2026-08-06 for per-tool on/off plus drag reordering, persisted; `session::models::features`
is the authority and is pure, so every rule is a unit test rather than something found by looking at
the app. Four of them matter outside that file: a stored entry naming a tool this build lacks is
**dropped** and a tool the file never names comes back **beside its default neighbour**, enabled;
**at least one tool always stays visible**, enforced in the model and drawn as a disabled switch with
the reason beside it; the tool on screen is always a listed one, which is the single function
`Features::active` answering both "the remembered tool was switched off" and "the open tool was just
switched off"; and **a switched-off tool is not a quick-navigation route** — `layout` drops its
detectors before `detect_among` runs, so a pasted `curl` with the API Explorer off falls through
rather than reopening it. The last one is the trap: `detect_among`'s allowed list is a **membership
test and never an order**, because `Detector::ORDER` is a correctness property (most specific first)
and the sidebar's order is a preference. `Layout::features` is the live list, `View::for_detector` is
the single mapping both `apply_route` and `allowed_detectors` read, and `settings::features_page`
builds the rows by hand because a `SettingItem` cannot carry a position. Adding the field is also
what took `session.json` to **schema version 2**; an older build would have read it, dropped the key
and written it back pruned. The historical tray language field took it to **3** for the same
reason — and forced the fix that makes the claim true: `parse_document` now stamps
`SCHEMA_VERSION` onto what it read, because until it did, a document loaded from a version-1
file was written straight back *as* version 1, so a newly-added key landed in a file older
builds still believed they understood.

**`src/quick_nav/` is the one feature that is not a tool**: a vim-shaped normal mode where
`Cmd+V` / `Ctrl+V` / `p` sends the clipboard to whichever tool can read it, and `Esc` leaves a
focused input. Its module docs are the authority and are worth reading before touching key
handling anywhere — three things there are counter-intuitive. **Normal mode is a key-binding
context, not a flag**: the bindings carry `Dodo && !Input`, gpui evaluates `!` against the whole
dispatch path, and that is the only definition — so `p` still types a `p` inside every text field
and ordinary paste is untouched. **The pane holds a focus handle for this**, because with nothing
focused gpui's dispatch path is the window root alone and carries none of the pane's context.
And **`Esc` is bound at the pane, deliberately shallower than every library `Escape`**: gpui tries
matched bindings deepest-first until one stops propagating, so a dialog, a popover, a select and
an input's own completion popup all still win, and this fires only once they have declined.
`models/detect.rs` carries the detection order (most-specific first: cURL → database URI → JWT →
JSON → Base64) and the rule that resolves the captain's editable-regex request against dodo's
existing parsers — *a pattern selects candidates, the parser confirms*. `layout.rs`'s
`apply_route` is the single place a detected route meets a `View`.

**`src/tray/` is the second feature that is not a tool**, and macOS-only: a menu bar
`NSStatusItem` showing a dodo with one keyboard-input-language glyph, plus a four-row native
menu. Its module docs are the authority. Four things there are decisions rather than details.
**The menu bar and native input method share `dodo_ime_core::LanguageId`** — the selected
language is written to `input-method.json`, which the bundle reads; `i18n::Language` remains
dodo's interface-language preference. A historical `session.json` tray value is migrated once
and no longer written. **Events arrive without a second event loop**: the
`tray-icon`/`muda` handlers run on the main thread but are `Fn + Send + Sync`, so they only
`unbounded_send` on a `futures-channel` mpsc that one foreground `Task` awaits — and both
handler slots are `OnceCell`s, so they must be installed *before* the menu and status item
exist or they are locked out for the process's life. **`QuitMode::Explicit` is set from the
tray, after the status item exists**, never statically in `main.rs`: dodo installs no menu bar,
so the tray's Quit is the only way out, and a failed tray with the mode already switched would
be unquittable. **`muda` has no radio group** and toggles a check item *before* emitting its
event, so the whole group is re-asserted on every selection. Adding a language is one variant
plus one `assets/icons/tray/dodo-<code>.svg`; the marks are rasterised through gpui's own
`SvgRenderer`, so they cost `src/assets.rs` no new filter and add no PNG.

**`crates/dodo-ime-core/` is dodo's own input method, and it is a crate, not a module.** dodo
depends on it and names only its configuration types — a normalized `KeyEvent`/`EngineAction`
vocabulary (`core/`) plus a Vietnamese engine speaking Telex and VNI (`languages/vietnamese/`);
no keystroke is ever processed in the dodo process **except the explicitly selected Windows
Keyboard Hook fallback**. Windows TSF now lives in `crates/dodo-ime-windows/`; later rounds add
per-application language memory, abbreviations and Linux IBus. Its crate docs are the authority;
four things there are decisions rather than details. **It is a crate because the OS hosts are
separate processes**: the macOS host has to be its own `.app` bundle that the system launches — it
cannot be the dodo process — and Windows and Linux load theirs into other people's applications;
all three link this engine and none may link gpui. A module of a binary crate
cannot be linked at all, so `src/input_method/` became `crates/dodo-ime-core/` and
**`purity_lint.rs` now guards a real boundary rather than an aspirational one**. Its remaining job
is what `Cargo.toml` cannot do: a dependency is one line and nothing warns, so the lint's
allow-list turns adding one — including a sibling workspace crate — into a failing test, and
`the_scan_covers_every_file` proves the check reads every file. Never widen the allow-list; pass
the value in at the boundary instead. The module-wide `#![allow(dead_code)]` is **gone**, not
because it was wired up but because everything is `pub` in a library and therefore reachable.
**`LanguageId` is the shared keyboard-language identity**: it is persisted through
`input-method.json`; English and Japanese pass keys through until their native engines exist.
`ActiveLanguages` defaults to English/Vietnamese and is the one menu/cycle set;
`LanguageSwitch` persists its key, modifiers and optional beep beside it. dodo remains the only
settings writer: native hosts report an explicit switch through the host-owned status file, waking
dodo via macOS's separate distributed notification or Windows' named event, so the UI and tray
adopt then persist it without polling. The purity lint keeps UI and IPC dependencies outside the
state machine. And **the Vietnamese
engine is semantic, not string
rewriting**: a letter is `(base, mark, case)`, the tone belongs to the syllable and its position is
recomputed at render time, so `toas` + `n` becomes `toán` without anything relocating a mark.
`InputScheme` is an enum for the same reason — Telex and VNI produce identical `Transform`s and
share every rule about Vietnamese. **Every modifier reaches back over the current syllable**, the
stroke included since 2026-08-08 (`did` is `đi`, `add` is still `add` because the rule is about the
*initial* letter); a scheme file that decides position itself rather than asking
`Syllable::mark_target` is how that one was wrong for a round. The price is stated in
`vietnamese::tests`: a Latin word whose keys spell a **valid** Vietnamese syllable is composed and
stays composed, because `rules`' word-boundary restore only rescues invalid ones — so `dodo` types
`đô`, which is what Unikey does too and is not a bug to fix. Tests: `corpus.rs` holds ~460 real words as *answers* and
derives both key sequences from them, so tone placement is never fed to the thing being tested.
**To actually type at it before any OS host exists**, run
`cargo run -p dodo-ime-core --example telex` (interactive) or
`… --example telex -- --keys tieengs` (one-shot, `--verbose` for the per-key `EngineAction`s).
It is an `examples/` target, never a `[[bin]]`, so it ships in nothing; its own header comment is
the authority on why it is line-based (raw per-keystroke input costs a dependency) and why its
strings are bare literals rather than `i18n::Str`.

**`crates/dodo-ime-macos/` is the macOS host**: an InputMethodKit `.app` that
macOS launches, links the engine, and types with **no dependency on `Dodo.app` running** — dodo
does not link it and cannot start it. `docs/macos-input-method.md` is the authority on building,
installing and enabling it by hand, on what dodo's own install action does (§7), on the two files
the two processes exchange (§8), on what was and was not verified (§5), and on what the next round
owes (§9: release wiring, the tray mark, a menu-bar icon, signing). Four things there
are decisions rather than details. **`CFBundleIdentifier` must contain `.inputmethod.` as an
infix**, not merely end in it: `io.github.mrgru.dodo.inputmethod` never appears in the
input-source list and `…inputmethod.Dodo` does, with `TISRegisterInputSource` returning `0` and
logging nothing either way — `src/bundle.rs` holds every identifier plus the eight-bundle table
that measured it, and the investigation report had this as an unverified READ note. **The bundle
nests at `dodo.app/Contents/Helpers/`**, never `Contents/Library/InputMethods/`, because only the
former is a directory `codesign` discovers as nested code (`docs/macos-signing.md` §7.2) — macOS
itself never looks inside `dodo.app`, so that copy exists purely for the install action to copy
out.
**Everything that could get Vietnamese wrong is pure and tested without a frame** — `keymap`,
`text`, `ops`, `session` — while `client.rs` and `controller.rs` decide nothing; that split is
what lets `tests/controller.rs` (`harness = false`, because the class is `MainThreadOnly` and
libtest spawns threads) drive the real class against a mock client and catch the one failure no
unit test can, a mistyped selector, which compiles and silently registers a method nobody calls.
And **`IMKTextInput` is hand-written `msg_send!` by necessity**: it is declared in Carbon's
HIToolbox, not in InputMethodKit, so `objc2-input-method-kit` does not and will not bind it —
`NSNotFound` there is `NSIntegerMax`, not `NSUIntegerMax`, and `NSRange`s are UTF-16 while the
engine counts graphemes, which `text.rs` converts through the engine's own walk so the two
definitions cannot drift.

**`crates/dodo-ime-windows/` is the Windows TSF COM DLL**, a `cdylib` Windows loads
independently of dodo. It links only the pure engine and IPC contract; `DllRegisterServer` writes
per-user COM registration and the Vietnamese profile, while its TSF edit session performs marked
composition rather than injecting keys. It re-reads settings before each key so a selected Keyboard
Hook makes TSF pass through. The fallback itself lives in dodo's
`src/input_method/services/keyboard_hook.rs`, is only active while dodo runs, tags injected output,
and passes uncertainty, repeats, key-up, shortcuts and secure-desktop uncertainty through. See
`docs/windows-input-method.md` for install/recovery and what captain testing still owes.

**`crates/dodo-ime-ipc/` is a crate because neither process can reach the other's code**. dodo and
both native hosts link it; it holds the identifiers each platform looks up, the two single-writer
JSON files, and the distributed-notification name. The alternative was two copies of one schema kept in step by
nothing, and a drifted field name there does not fail to compile: it reads as absent, so the user's
setting silently has no effect. `dodo-ime-core` cannot hold it — its `purity_lint` forbids `serde::`
by test — and dodo must not link `dodo-ime-macos`, which would drag InputMethodKit into a UI
application for four string constants. Three things there are decisions. **One writer per file, and
no locking**: dodo owns `input-method.json`, the native host owns `input-method-status.json`, every write
is temp-file-then-`rename`, and neither side has a method that could write the other's. **The
version rule matters more here than anywhere else in dodo** — a months-old bundle reading a new
dodo's settings file is ordinary, not exotic, so both parsers refuse a `"version"` above their own
and the bundle then types with `DEFAULT_CONFIG` and reports revision `0`, which is exactly how dodo
knows to say "not picked up yet". And **the status file is the one file the native host writes**, which
contradicts the older "writes no file" claim in `dodo-ime-macos`'s own docs and is corrected there:
nothing the user typed may ever appear in it; it is written on start, settings changes and explicit
language-switch commands, never ordinary typing, and a test pins its key set so adding a field is
deliberate.

**`src/input_method/` is dodo's end of it, and `services/tis.rs` carries a crash worth knowing
about.** It is a **tool, not a Settings page** — the captain asked on 2026-08-09 — and the move took
the whole surface: backend choice, status, install button and the four engine settings are on the
pane and nowhere else, so no control is reachable twice. That makes `View::InputMethod` the **first
platform-conditional tool**, so `View::ALL` is written out twice and every `match` on `View` carries
a `cfg` arm; the row draws `AppIcon::Keyboard`, deliberately not the globe, which is the API
Explorer's. The pane holds **no setting at all** — `Layout::new` builds it before
`input_method::load` has read the file, so every control reads the global in `render` and the two
either/or settings are radio groups rather than dropdowns, whose `SelectState` would be a second
copy of the setting; the shortcut recorder's three fields are the exception that proves it, holding
only what the user is *doing*. Native remains the persisted default. Event Tap is the macOS-only
alternative in `services/event_tap.rs`; Windows instead offers `Keyboard Hook`, a clearly-labelled
no-install fallback that runs only while dodo does. Both direct-output fallbacks use the existing
engine and never share transformation with Native TSF/InputMethodKit.
**Whichever backend is selected owns the language switch**, because the others are passing every key
through — so both fallbacks stay attached in *every* language (a listener that stopped in English
could never switch back), answer the shortcut before the engine sees anything, and are
**reconfigured, never joined by a second**, which is what makes a recorded shortcut live on the next
keystroke and the replaced one inert. `models/live_switch.rs` is that rule, pure and tested
everywhere; a cycle performed inside an OS callback returns to the state layer over an mpsc channel
because a callback has no `App`. The switch is `Shortcut { modifiers, key }` at
`SettingsDocument::language_switch`, **schema 8** (`backend` was 4), where `key` is the engine's
non-printing key set plus `Modifiers`; validity is one rule — a command modifier must be held — and
never a count or a fixed shape. **A printing key is not in the vocabulary and never will be**: a host
is handed what a key *types*, so `⌥Z` arrives as `Ω` and could not be matched again. **The macOS
Native host cannot see a modifier-only shortcut at all** — `recognizedEvents:` is
`NSEventMaskKeyDown` and `FlagsChanged` reaches only `handleEvent:client:` — so the pane says to use
Event Tap; `docs/macos-input-method.md` §8a is the authority and §9 owns the fix.
Event Tap retains its macOS-only
accessibility/secure-input/feedback protections; the TSF installer has separate tested path data in
`models/windows.rs` and is intentionally per-user. The
Windows hook API has no normal password-field bit, so it passes secure-desktop and all other
uncertain input through; `docs/windows-input-method.md` is explicit that this needs captain runtime
testing. `dodo_ime_ipc::paths` now duplicates both the macOS and Windows data-directory rules and
is tested against `src/paths.rs`. The module remains un-gated so Linux checks its pure parts, but
only macOS and Windows expose the pane.

**`src/dialog_slot.rs` is why Settings and the updater cannot stack two copies of themselves.**
A dialog layer is a stack and `open_dialog` pushes unconditionally, so a dialog with two ways in —
Settings (sidebar footer, menu bar item) and the updater (sidebar footer, background check) — put
two identical cards on screen. Both now `claim` a slot keyed by a marker type and `release` it from
`on_close`; the module doc is the authority, and `gpui-component-recipes` records the two rules that
bite (never pair `release` with a second `close_dialog`, and the window rather than the flag
decides). No other dodo dialog needs it: the rest are opened from a control the modal overlay
covers.

`data_dir()` lives in `src/paths.rs`, not under `api_explorer/` any more, and it knows all
three platforms: `~/Library/Application Support/dodo`, `%APPDATA%\dodo`, `$XDG_CONFIG_HOME` or
`~/.config`. The macOS path is frozen — changing it orphans every existing installation's saved
collections. It classifies the platform from `build_info::VERSION_INFO.target` rather than
`#[cfg]`, which is what lets all three branches be unit tested from a Mac that cannot compile two
of them; copy that trick rather than a `cfg` split for anything else platform-shaped and pure.

The files version differently, and the difference is deliberate. A `RequestSnapshot` inside
`collections.json` is versioned only by `#[serde(default)]`, which copes with *added* fields and
nothing else. `environments.json`, `script-consent.json`, `updater.json`, `connections.json`,
`query-data.json`, `quick-nav.json` and `cleaner-ignored-items.json` carry an explicit
`"version"` from their very first write, and their `parse_document` **refuses** a file whose
version is higher rather than half-reading it. Copy that pattern for any new file; do not copy
`collections.json`'s.

**Build and release engineering lives in `docs/`**, and those four files are the authority for it:
`docs/build-optimization.md` (release profile, the measured before/after size table, linker
findings, the dependency report, startup review), `docs/release.md` (CI, the release workflow,
packaging, verification, the application icon, the in-app updater), `docs/macos-signing.md` and
`docs/macos-input-method.md` (building the input-method bundle, installing it by hand or from
dodo's own Settings page, and the two files the two processes exchange).
The rest is `Cargo.toml`'s `[profile.*]` comments, `build.rs`, `scripts/` and `.github/`.

**dodo is unsigned on every platform, and `docs/macos-signing.md` is the authority on changing
that** — written on 2026-08-08 as a *procurement* document, because the captain decided signing
waits but must stay reachable. It is the answer to "what must the repo owner personally buy or
create" (Apple Developer Program, US$99/yr; a Developer ID Application certificate, max five per
account, Account Holder role only; one notarisation credential), the secrets by exact name, the
entitlements — **dodo needs none, and neither will the input-method bundle** — and the ordering.
Three things there are corrections to plans that were recorded wrongly and would have cost a
release each. Signing happens **inside** `scripts/package.sh` / `macos-app-bundle.sh`, before the
tar, because the published SHA-256 and `update.json` entry are computed from that archive. A
workflow `if:` **cannot read `secrets`** — it reads an `env:` set from one at job level. And
`codesign --deep` is deprecated for *signing* (still correct for verifying): nested bundles are
signed inside-out, one call each. Signing is a user-experience purchase, not an enablement one —
an unsigned dodo and an unsigned input method both run today.

**The application icon is a committed pipeline, not a file someone dropped in.** `assets/branding/`
holds the original artwork and the 1024 RGBA master; `python3 scripts/generate-icons.py` derives
the macOS `.icns`, the Windows `.ico`, the Linux hicolor PNGs and one 256px PNG from it, and all of
those are committed because packaging must not depend on the host (`iconutil` is macOS-only). Read
"Application icon" in `docs/release.md` before touching any of it — it is the authority on all five
launch paths, on which of them anyone has actually looked at (one: the macOS bundle), and on the
fact that a `.icns` `iconutil` accepted can still render blank. **The icon is answered in three
unrelated places**, which is the part that catches people: `build.rs` compiles the `.ico` into
`dodo.exe`, `scripts/macos-app-bundle.sh` puts the `.icns` in the bundle, and `src/window_icon.rs`
covers at runtime what no file can — a bare macOS binary's Dock tile, and the Linux `app_id` that
`dodo.desktop` is matched against.

**Do not confuse `assets/{branding,macos,windows,linux}` with `assets/icons`**: only
`icons/**/*.svg` and `themes/**/*.json` are embedded in the binary through `rust-embed` (the
`#[include]` filters in `src/assets.rs`), which is why the packaged icon artwork costs zero bytes.
The one exception is `assets/branding/dodo-256.png`, which `src/window_icon.rs` pulls in with
`include_bytes!` — a different mechanism the filters do not govern. Anything new under `assets/`
that must stay out of the binary has to stay outside those two filters *and* out of an
`include_bytes!`.

**Every release publishes an `update.json` manifest**, generated by
`tools/update-manifest` — a **standalone crate that is not part of dodo**. `exclude = ["tools/*"]`
in `[package]` keeps it out of `cargo package`, `workspace.exclude` keeps it out of the workspace,
it carries its own `Cargo.lock` and four dependencies, and it is built only by the release workflow
through `--manifest-path`. It costs the binary zero bytes. Do not add it to the workspace, and do
not give dodo a `[[bin]]`. "Automatic updates" in `docs/release.md` is the
authority: the manifest shape and why `manifest_version` / `signature` / `channel` exist, the
hand-verification recipe, and the channel design. Three things that are
decisions rather than details: the manifest points at macOS's **`-app.tar.gz` bundle** selected by
exact filename (an installer swaps the `.app`); **any missing platform fails the release**,
experimental ones included, because a silently absent platform means those users are never offered
an update; and the publish step is **create-or-update**, because `gh release create` cannot repair
a tag that already exists and tags here are immutable. `src/updater/` is what reads it.

Eight things about build and release that catch people:

- **The repo is a cargo workspace, and `crates/` vs `tools/` is the rule for which shape a new
  crate takes.** It gained `[workspace]` on 2026-08-08 when the input-method engine moved to
  `crates/dodo-ime-core/`, which contradicts the older "do not add it to a workspace" line above —
  that line was and remains right *about `tools/update-manifest`*, and the difference is whether
  dodo **links** the crate. A linked crate is a workspace member so there is one `Cargo.lock` and
  one `--locked`; a second lockfile for a linked crate would be a second, silently divergent
  resolution of shared dependencies. A crate the release workflow merely *runs* stays standalone
  and excluded. **`crates/dodo-ime-macos/` is the case the rule did not anticipate** and settles it
  the same way: dodo does not link the macOS input-method host at all, but the host links
  *`dodo-ime-core`*, which dodo does — so a second lockfile would resolve the engine independently
  and "the engine the tests prove" and "the engine the shipped bundle types with" would be two
  resolutions nothing compares. `default-members = [".", "crates/dodo-ime-core",
  "crates/dodo-ime-macos"]` is load-bearing: without it a bare `cargo test` / `cargo clippy
  --all-targets` would silently stop covering a member. Naming the macOS host there is safe on
  every platform only because its Objective-C dependencies sit under a
  `[target.'cfg(target_os = "macos")'.dependencies]` table — a plain `[dependencies]` entry would
  make the Linux and Windows `cargo check` rows build AppKit bindings. And `workspace.exclude`
  matches **paths, not globs** — `tools/*` there excludes nothing, which is why it is spelled out.
  `cargo metadata --no-deps` at the root now lists four packages, not one — the fourth being
`crates/dodo-ime-ipc`, which is a member for the same reason and named in `default-members` too.

- **Two of the four `cargo check` targets cannot be run from this Mac at all.**
  Linux and Windows both die in `aws-lc-sys`'s C build script (no cross C toolchain, no
  `windows.h`) — not a portability problem in dodo, and not fixable by a cargo flag. The two Apple
  targets do cross-check locally. "The `check` row runs natively" in `docs/release.md` has the
  detail, including the two traps that cost time: Homebrew's `rustc` shadows rustup's and ships
  only the host std (`rustup run` does **not** fix it — use the toolchain's absolute path), and a
  cross-check needs its own `CARGO_TARGET_DIR` or it invalidates the warm cache a size
  measurement depends on.
- **`fmt` and `clippy` are blocking jobs; keep them green.** Run `cargo fmt --all` and
  `cargo clippy --all-targets --locked -- -D warnings` before committing. The pre-existing debt
  (34 unformatted files, 12 warnings) is paid off, and **there is no crate-level `allow`** — every
  suppression is `#[allow]`ed at the item it applies to (or, where a whole module is the pending
  unit, as an inner attribute under that module's `//!` docs) with the reason and the condition for
  removing it written next to it. Copy that shape; never widen an `allow` to quieten a lint.
  Dead-code warnings in a module under construction are **scaffolding, not defects**: annotate,
  do not delete. `.githooks/pre-push` runs `fmt`, `clippy` and `cargo test --locked` and refuses
  the push if any fails; it is opt-in per clone with `git config core.hooksPath .githooks`
  (see "Pre-push checks" in `README.md` for its cost and the `--no-verify` bypass).
  Note that `cargo build` alone does **not** prove the tree is green — `src/i18n.rs`'s test module
  is exhaustive over `Str`, so new strings break `cargo test` while the app still builds.
  `build (windows-x64)` failed on its one real run (a `#[cfg(unix)]`-only bollard connector; fixed
  by the platform split in `docker/services/engine.rs`, not yet confirmed green) and
  `build (macos-x64)` is unverified — those rows are
  `experimental` and non-blocking on purpose. See the honesty note atop `.github/workflows/ci.yml`
  for what has actually run.
- **No `--release` build runs on a push any more.** `ci.yml` does `cargo check` per platform plus
  one debug build; the four-platform release matrix lives in
  `.github/workflows/release-profile.yml` (weekly + manual) and, for a tag, in `release.yml`. The
  accepted cost — release-only failures surface up to a week late — is stated at the top of both
  `ci.yml` and "CI architecture" in `docs/release.md`. Do not quietly re-add a release build to
  the push path.
- **dodo's source is MIT (`LICENSE`), and that does not settle how binaries may be distributed.**
  `gpui -> sum_tree -> ztracing -> zlog` pulls GPL-3.0-or-later into every build.
  `THIRD-PARTY-NOTICES.md` is the authority: it records the verified chain and keeps the
  distribution question explicitly **open**. `deny.toml` deliberately carries no `allow` or
  `exceptions` entry for those crates so `cargo deny` keeps reporting them — do not silence it,
  and do not write a conclusion about that question into the repo.
- **`rusqlite` and `sqlx` cannot be in the same graph, even switched off.** Both declare
  `links = "sqlite3"` through `libsqlite3-sys` — at versions that do not overlap (`rusqlite 0.40`
  needs `0.38`, `sqlx 0.9` needs `>=0.30.1, <0.38`) — and cargo refuses to resolve a graph
  containing two packages linking the same native library, `optional = true` or not. The error
  names `libsqlite3-sys` and says nothing about which of your dependencies wanted it. This rules
  out a "sqlx for the network backends, rusqlite for SQLite" mix unless the versions are pinned to
  a compatible pair; it cost the design round one failed build and is recorded here so it costs
  the next one none.
- **`Cargo.lock` really is the only possible pin on the four git dependencies.** Explicit
  `rev = "…"` pins were tried and cannot work here — upstream depends on itself through unpinned
  default-branch refs, and the three resulting cargo errors are recorded in
  `docs/build-optimization.md`. Hence `--locked` everywhere, and `cargo update` only ever as its
  own reviewed commit.
- **`dodo --version` / `--build-info`** print what `build.rs` embedded and exit before any window
  opens (`print_build_metadata_and_exit` in `src/main.rs`). That path is how CI proves a packaged
  binary runs at all — a GUI app cannot open a window on a headless runner — so keep it free of
  GPUI initialisation.

## Skills

Detailed, verified knowledge lives in `.claude/skills/<name>/SKILL.md`. Load one when its trigger
fires — they are written to be read at the moment of need, not up front, so a session that never
touches a module never pays for its internals. This table is the single index; the four
`dodo-*-internals` skills hold what used to be inlined in this file as a per-module wall of text.

| Skill | Load it when |
|---|---|
| `dodo-api-explorer-internals` | Touching anything under `src/api_explorer/` — the send pipeline, scripting/sandbox, consent gating, codegen/curl, collections, or tab/column layout. |
| `dodo-docker-internals` | Touching anything under `src/docker/` — engine discovery, the four list pages, polling, the detail dialog, or a "Coming soon" placeholder. |
| `dodo-database-internals` | Touching anything under `src/database/` — the connection tree, query execution, the `Driver` trait, or result-grid layout. |
| `dodo-build-release-internals` | Touching `src/updater/`, `.github/workflows/`, `Cargo.toml`'s dependencies, `docs/release.md`, `docs/macos-signing.md`, `docs/build-optimization.md`, `scripts/generate-icons.py`, `tools/update-manifest/`, `deny.toml`, or `THIRD-PARTY-NOTICES.md`; preparing or debugging a release. |
| `gpui-component-recipes` | Writing or editing any `render` / `new` that builds a gpui-component widget (input, code editor, diagnostics, select, dialog, settings panel, sidebar, button, icon); a widget call will not compile; a widget builds but nothing appears on screen; or a code editor draws uncoloured text. |
| `dodo-tool-view` | Adding, renaming, reordering or removing a sidebar tool; a new sidebar entry does not appear or renders blank. |
| `dodo-i18n-text` | Writing or changing **any** text a user reads — a label, title, placeholder, description, error, dropdown option; or an `i18n` / `i18n_lint` test fails. |
| `dodo-theming-settings` | Adding or changing a setting, adding or removing a theme or a language, or a settings change does not apply until restart. |
| `dodo-build-validate` | First `cargo` invocation of a session, adding tests, a build or `cargo test` failing oddly, or being asked whether a UI change actually works. |

Two things that catch everyone and belong here rather than behind a trigger:

- **`Cargo.lock` is the only pin on the four git dependencies.** `cargo update` silently jumps
  them to upstream HEAD. Never run it as a side effect of another task. An explicit `rev` pin
  cannot replace it — upstream depends on itself through unpinned default-branch refs, and the
  three resulting cargo errors are recorded in `docs/build-optimization.md`. Hence `--locked`
  everywhere, and `cargo update` only ever as its own reviewed commit.
- **The pinned `gpui-component` source is the reference for every widget question**, at
  `~/.cargo/git/checkouts/gpui-component-*/<rev>/crates/ui/src` (rev from `Cargo.lock`). Its
  `<checkout>/skills/` directory holds the upstream authors' own guidance, which is excellent on
  GPUI fundamentals and stale in a few places — `gpui-component-recipes` records which.

## Maintaining this file

Keep this file for knowledge useful to almost every future agent session in this project.
Do not repeat what the codebase already shows; point to the authoritative file or command instead.
If a fact only matters when touching one module, it belongs in that module's skill (table above),
not here — this file is the map, not the territory. Prefer rewriting or pruning existing entries
over appending new ones. When updating this file, preserve this bar for all agents and keep
entries concise.
