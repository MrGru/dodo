# `dodo-input-method`

dodo's own end of the native input methods: installing them, telling them how to type, and — when a
fallback backend is selected — running the engine in dodo's process. Read `src/lib.rs` first; it
owns the crate's shape, the "no engine here under Native" rule, the language-switch ownership rule,
and why the module is compiled on every platform.

The two platform documents are the authority on behaviour and on what has actually been verified:
`docs/macos-input-method.md` (the bundle, Event Tap, installing, the two IPC files, the shortcut
this host cannot see) and `docs/windows-input-method.md` (TSF, the Keyboard Hook fallback, where a
key's case comes from, and the captain's test script). `crates/dodo-ime-core/AGENTS.md` is the
engine.

**This crate was `src/input_method/` until 2026-08-15.** Any path that still says so is stale.

## It is a tool, not a Settings page

The captain asked for that on 2026-08-09 and the move took the **whole** surface: backend choice,
status, install button and the four engine settings are on the pane and nowhere else, so no control
is reachable twice. That makes `View::InputMethod` the first and still only platform-conditional
tool, which its row in `src/tools.rs` says with one `hosts:` field and no `#[cfg]`. The row draws
`AppIcon::Keyboard` — deliberately not the globe, which is the API Explorer's.

The pane holds **no setting at all**, which `views/input_method_view.rs` explains: `Layout::new`
builds it before `input_method::load` has read the file, so every control reads the global in
`render`. That is also why the two either/or settings are radio groups rather than dropdowns, whose
`SelectState` would be a second copy of the setting.

## `services/tis.rs`: the crash to know about before you touch it

**Two concurrent `TISCreateInputSourceList` calls abort the process** — `SIGABRT` from inside
HIToolbox, not an error return. It was found by running this module's own tests in parallel, which
is exactly what `cargo test` does. Calling it from a non-main thread is fine; calling it from two
threads at once is not. `LOCK` is half the answer and `SystemOps::on_main` is the other half,
because AppKit calls TIS on the main thread whenever it likes and no lock of ours serialises against
that. The file is the only unchecked FFI in dodo and its module doc states all four of the API's
undocumented behaviours.

## The Windows lesson: `GetKeyboardState` is the wrong question

It answers **per thread**, and only advances as that thread reads key messages from its own queue.
In a `WH_KEYBOARD_LL` callback — which runs on dodo's thread while the key is on its way to somebody
else's application — it is frozen at whatever dodo last saw. That one fact produced four symptoms:
shift read as up, so `ToUnicodeEx` returned the unshifted character (no capital could reach the
engine, and every rewritten syllable came back lowercase); `Modifiers` was always empty, so no valid
shortcut could ever match and the language switch was dead; and caps lock's toggle bit went stale.

`models::keyboard_hook::layout_state` now **builds** the array from `GetAsyncKeyState` rather than
fetching it — a stale array is worse than an empty one, because a leftover Control byte turns a
letter into a control character — with the arriving key folded in (`with_key_down`, which is what a
modifier-only shortcut fires on) and caps lock tracked rather than asked. The TSF DLL runs on the
application's own thread where the snapshot should be right, so it **merges** instead
(`keymap::merge_physical`) and passes the real scan code from `lParam`.

The other half of the switch was **two gates**: the hook required a focused edit control and TSF
required a writable context before either looked at the shortcut, so a window with nowhere to type
could not change language. Both now match the shortcut first.

All of it is pure and tested from a Mac. **None of it has run on Windows** —
`docs/windows-input-method.md` step 3a is what would prove it.

## Whichever backend is selected owns the language switch

The others are passing every key through, so a fallback that ignored the shortcut would leave it
working nowhere. Both fallbacks therefore stay attached in **every** language (a listener that
stopped in English could never switch back), answer the shortcut before the engine sees anything,
and are **reconfigured, never joined by a second** — which is what makes a recorded shortcut live on
the next keystroke and the replaced one inert. `models/live_switch.rs` is that rule, pure and tested
everywhere; a cycle performed inside an OS callback returns to the state layer over an mpsc channel,
because a callback has no `App`.

The switch is `Shortcut { modifiers, key }` at `SettingsDocument::language_switch`, **schema 8**
(`backend` was 4), where `key` is the engine's non-printing key set plus `Modifiers`. Validity is one
rule — a command modifier must be held — and never a count or a fixed shape. **A printing key is not
in the vocabulary and never will be**: a host is handed what a key *types*, so `⌥Z` arrives as `Ω`
and could not be matched again. The macOS Native host cannot see a modifier-only shortcut at all, so
the pane says to use Event Tap; `docs/macos-input-method.md` §8a is the authority and §9 owns the
fix.

## A Backspace rewrite is wrong in a browser address bar

Only a fallback can be wrong that way — a native host composes through a marked-text client. Inline
autocomplete keeps a selection alive between keystrokes, so the first Backspace eats it instead of
the character the engine meant. `models/browser_rewrite.rs` is the rule and is pure: one `BROWSERS`
table routing bundle IDs to **two** strategies (Blink takes `Shift`+`Left` then `1 -> 0` on the
count; WebKit and Gecko take an invisible character then one extra Backspace — the Chromium trick is
unreliable in Safari), three guards that answer "post it verbatim", and an unlisted application
deliberately left alone rather than treated as WebKit.

Two things are not detectable and are paid for rather than solved: **start-of-field** is not, so
`delete_before > 0` is the proxy; and **focus** is not, so Safari and Firefox pay one extra insert
plus Backspace in ordinary page inputs too. That is accepted, and emitted from one function so it
can be narrowed later. `browser_address_bar_fix` is the switch, default on, added to
`input-method.json` **without** a schema bump, because a defaulted `bool` no native host reads is
not a misread. `docs/macos-input-method.md` §3b is the authority — including the part no unit test
can prove: **no real browser has run this.**

## Two smaller things

`dodo_ime_ipc::paths` duplicates both the macOS and Windows data-directory rules and is tested
against the binary's own copy; `docs/architecture/persistence.md` explains why that guard exists
wherever a crate re-answers `data_dir()`.

The TSF installer has separate tested path data in `models/windows.rs` and is intentionally
per-user. The Windows hook API has no normal password-field bit, so it passes secure-desktop and all
other uncertain input through; `docs/windows-input-method.md` is explicit that this needs captain
runtime testing. Event Tap retains its macOS-only accessibility, secure-input and feedback
protections.
