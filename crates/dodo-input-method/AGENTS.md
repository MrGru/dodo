# `dodo-input-method`

Dodo's in-process input methods: Event Tap on macOS and Keyboard Hook on Windows. Read `src/lib.rs`
first; it owns startup, persistence, and the one listener per platform. `crates/dodo-ime-core/` is
the engine and stays platform-free.

## It is a tool, not a Settings page

The pane owns input-language selection, the switch shortcut, and the four Vietnamese settings.
It holds no copy of those values: `Layout::new` builds the pane before `load` reads
`input-method.json`, so controls read the `InputMethod` global in `render` and write it directly.
The sidebar row remains available only on macOS and Windows through `src/tools.rs`'s `hosts:` value;
Linux has no implementation.

There is no implementation choice. macOS always starts Event Tap after settings load; Windows always
starts Keyboard Hook. Both exist only while Dodo runs.

## The listener owns the language switch

A listener stays attached in every selected language. English and Japanese pass ordinary keys
through, but the listener still answers the configured shortcut; stopping it outside Vietnamese
would make switching back impossible. A cycle inside an OS callback returns to the state layer over
an mpsc channel because the callback has no `App`. Reconfiguration replaces the live listener's
settings and never installs a second listener.

The shortcut is `Shortcut { modifiers, key }`, where the key is non-printing or `Modifiers` itself.
A command modifier is required; a modifier-only shortcut needs at least two modifiers. Printing keys
are deliberately absent because a listener receives what the current layout types, not a stable
physical label.

## Event Tap

`models/event_tap.rs` owns callback policy and direct composition; `services/event_tap.rs` owns the
CoreGraphics boundary. Generated events carry a process-unique marker and pass through before any
state is touched. Secure input passes through. Focus, target, navigation, mouse, recovery, or
configuration changes discard retained composition.

Browser address bars need the adjustment in `models/browser_rewrite.rs`: Chromium extends the
selection; Safari and Firefox insert then remove a zero-width character. Unknown applications are
left unchanged. The browser switch is macOS-only and defaults on.

## Keyboard Hook

`GetKeyboardState` is thread-local and stale in a background low-level hook. The Windows service
builds the 256-byte layout state from `GetAsyncKeyState`, folds in the arriving key, and tracks Caps
Lock. Character case and `Modifiers` must come from that same physical snapshot. Injected, repeated,
unknown, command, and uncertain-target events pass through; `Drop` unregisters both keyboard and
mouse hooks before callback state is freed.

The pure Windows policy is compiled and tested from every host. Actual global-hook behavior still
requires a Windows runtime check.

## Persistence

`input-method.json` is ordinary Dodo state, owned entirely by this crate. It carries the selected
language, active languages, switch shortcut, Vietnamese settings, and macOS browser workaround.
The schema remains version 8 so existing files load; unknown historical fields are ignored and
vanish on the next save. Reads and atomic writes live in `services/document.rs` and
`services/store.rs`.

## Maintaining this file

Keep only crate-wide decisions and traps that are not already clear in `src/lib.rs`. Point to the
authoritative module instead of duplicating implementation detail, and remove stale guidance when
the source changes.
