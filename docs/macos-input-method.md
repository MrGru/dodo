# dodo's macOS input method

`crates/dodo-ime-macos` builds **Dodo Vietnamese.app**, an InputMethodKit input
method that types Vietnamese using `crates/dodo-ime-core`. macOS launches it;
`Dodo.app` does not, and typing keeps working with dodo closed.

dodo can now **install it** (§7), **tell it how to type** (§8), or choose an
Accessibility-gated **Event Tap** alternative (§3a). The bundle is ad-hoc signed
for local use; release wiring, the tray mark, a menu-bar icon and Developer ID
signing/notarisation remain — §9 lists them. The design rationale lives in the
crate's module docs, which are the authority; this file is how to build, install
and enable it by hand, what dodo does when it does that for you, and what was
and was not verified.

Two documents sit behind it: the investigation report that proved the approach
(`dodo-ime-macos-scout`), and `docs/macos-signing.md`, which constrains where
the bundle is nested. Where this file disagrees with the report, this file is
the later measurement — §6 lists the three corrections.

---

## 1. Build

```sh
scripts/macos-input-method-bundle.sh
```

That builds `target/release/DodoVietnamese` and assembles and ad-hoc signs
`dist/Dodo Vietnamese.app`. A valid signature is required for local use on
current macOS; the script verifies it with `codesign --verify --deep --strict`
before reporting success. Pass `--binary` to use one you already have, `--out`
to put the bundle somewhere else, or `--sign <identity>` for a real identity.

To carry it inside `dodo.app`:

```sh
scripts/macos-app-bundle.sh --binary target/release/dodo \
    --input-method "dist/Dodo Vietnamese.app"
```

which nests it at **`dodo.app/Contents/Helpers/Dodo Vietnamese.app`**. Without
`--input-method` nothing is nested and `dodo.app` is unchanged, which is what
`scripts/package.sh` still does — see §9.

macOS never looks inside `dodo.app` for an input method. That copy exists so the
install action (§7) has something to copy out. The location is fixed by
`docs/macos-signing.md` §7.2: `codesign` discovers nested code in a fixed set of
directories, `Contents/Library/InputMethods` is not one of them, and a bundle
placed there is sealed as an opaque resource rather than as code.

## 2. Install and enable, by hand

dodo has a button for this now — §7 — and this section is still the authority on
*what* that button does and why each step is there.

```sh
ditto "dist/Dodo Vietnamese.app" ~/Library/Input\ Methods/"Dodo Vietnamese.app"
```

**The destination has to name the bundle.** `ditto src dst` copies the *contents*
of `src` into `dst`, so the shorter `ditto … ~/Library/Input Methods/` this file
used to give leaves `Contents/` sitting directly in `~/Library/Input Methods` and
installs nothing. Measured while writing the install action.

`~/Library/Input Methods/` needs no admin rights. Then make macOS notice it:

```
System Settings → Keyboard → Text Input → Input Sources → Edit… → +
  → Vietnamese → Dodo Vietnamese → Add → Done
```

and switch to it with the Globe key, Caps Lock, or `Ctrl-Space`.

Three things about this that cost time if you do not know them:

- **`TISRegisterInputSource` is needed, and once is not always enough.** On a
  fresh install the source can take a few seconds to appear in
  `TISCreateInputSourceList`, and after a remove-and-reinstall at the same
  identifier it did not appear until the call was repeated. It returns `0`
  either way.
- **Enable and select the *mode*, never the parent.** The parent
  (`…inputmethod.Dodo`) has `kTISPropertyInputSourceIsSelectCapable = false`;
  selecting it fails with `-50` (`paramErr`). The mode is
  `…inputmethod.Dodo.Vietnamese`.
- **Upgrading requires killing the running process.** Replacing the bundle on
  disk does *not* restart it — the old binary keeps serving until it exits, and
  macOS relaunches it on the next input session. So an install action must be
  copy → register → `kill`. This is not a constraint on the bundle's design;
  nothing here caches anything across a launch, so being killed at any moment is
  always safe. It is a constraint on whoever writes the installer.

To remove it: delete the bundle from `~/Library/Input Methods/`, remove the
source in the same System Settings pane, and `pkill -x DodoVietnamese`. `-x`
matches the process name exactly; `-f` matches whole command lines and will happily
kill an editor that has this file open.

## 3. What it does

Telex, modern tone placement, spell check and bracket shortcuts on — the
compiled-in defaults in `dodo_ime_macos::DEFAULT_CONFIG`, which are Unikey's.
Since the settings round they are the **fallback** rather than the whole story:
dodo writes `input-method.json` and the bundle reads it (§8).

`tieengs` composes `tiếng`; `dduowngf` composes `đường`; a space, a comma, an
arrow key or a command shortcut commits the syllable and reaches the application
untouched. English that Telex would mangle survives, because the engine's spell
check hands back the keys as typed when the result is not a Vietnamese syllable:
`where` stays `where`. `test` still becomes `tét`, exactly as it does in Unikey
— `tét` *is* a Vietnamese syllable and nothing can tell the two intentions apart.

**No typing history, ever.** The bundle opens no socket and prints nothing about
what was typed. The only state that outlives a keystroke is the syllable being
composed, and `commitComposition:` / `deactivateServer:` — which is how macOS
reports a password field — drop it.

One sentence of that rule changed in the settings round and is worth stating
plainly rather than quietly: the bundle used to write **no file at all**, and now
writes exactly one, `input-method-status.json` (§8). Nothing the user typed may
appear in it — not a syllable, not a key, not a count of them, not the identifier
of the application being typed into — and it is written when the process starts and
when settings change, **never on a keystroke**, because a file written per
keystroke would be a typing log whatever its fields said. `dodo_ime_ipc::status`
carries the constraint, and a test there pins the file's key set so that adding a
field is a decision someone makes on purpose.

### 3a. Event Tap backend

**Sidebar → Input method → Backend → Event Tap.** This is an alternative to,
not a replacement for, Native Input Method. It runs only while Dodo is open and
requires Dodo to be enabled in **System Settings → Privacy & Security →
Accessibility**. After Dodo has saved the selection and the Native Input Method
handoff permits Event Tap startup, the first untrusted reconciliation in a Dodo process calls
`AXIsProcessTrustedWithOptions` with `kAXTrustedCheckOptionPrompt=true`.

macOS owns that asynchronous request and the Accessibility list; Dodo cannot
and does not change TCC. Enable Dodo there, then return to Dodo: its window
activation re-checks trust and starts Event Tap without reselecting the backend
or relaunching. Until then every key passes through. Existing grants are checked
without prompting, and an untrusted process makes only one request attempt.
Because TCC grants may be tied to the application's identity, an unsigned or
ad-hoc-signed build may need to be enabled again after it is replaced; verify
that behaviour against the packaged build identity.

Event Tap drives the existing `dodo-ime-core` Vietnamese engine in direct
output mode. The trade-off is deliberate: unlike Native Input Method it cannot
show marked text, so only the smallest cursor-safe changed tail enters the
focused application and its undo history. It retains the raw characters of the
current uncommitted word and, after a plain space or proven pass-through
punctuation, one bounded in-memory snapshot until every contiguous separator is
physically deleted. It then recomputes through the normal engine rules; any
navigation, shortcut, mouse click, focus/secure-input change, tap recovery,
configuration change or new text drops the snapshot. It never writes, logs, sends, or exposes
keys or words.

Every Dodo-generated `CGEvent` has a process-unique
`kCGEventSourceUserData` marker. The callback passes a marked event through
before decoding it or touching composition state, including generated
Backspace and Unicode key-up/down pairs, so output cannot feed back into the
engine. A Unicode replacement carries its payload on key-down only; its tagged
key-up only ends that key, avoiding a second insertion. The smallest
counterfactual changes only that key-up to carry the payload too; the native
descriptor test pins it empty. An ordinary physical
Backspace is never replaced: it passes through once and removes one rendered
grapheme from the in-memory word state. Secure input is passed through
unchanged. Space, punctuation, navigation and shortcuts commit the current word
if needed, then pass through unchanged. The rewrite trigger is a changed tail
such as physical `D` followed by `D` (`D` → `Đ`); ordinary physical appends do
not stage Unicode and so mask the defect. Target changes and marked synthetic
re-entry are separate reset/filter paths, covered independently of that
replacement. The descriptor and document simulators cover the transaction, but
no browser or native-editor Event Tap run was available here.

### 3b. Browser address bars

A browser address bar keeps an **inline autocomplete selection alive between
keystrokes**. Event Tap rewrites the current syllable as *n* Backspaces followed
by one Unicode insert, and in an address bar the first Backspace deletes that
selection rather than the character the engine meant — so the tone mark lands on
the wrong letter. Textareas and ordinary in-page inputs have no such selection
and were already correct. Safari reproduces it as readily as Chrome, so this is
not a Chromium quirk.

`src/input_method/models/browser_rewrite.rs` is the authority and is pure. Two
strategies, because Blink and WebKit do not clear a selection the same way:

- **Chromium family** (Chrome, Chrome Canary, Chromium, Brave, Edge, Vivaldi,
  Opera, Arc, Cốc Cốc) — a full `Shift`+`Left` key-down/key-up pair, carrying
  `Shift` and `NumericPad` exactly as macOS flags a real arrow key, ahead of the
  Backspaces. One Backspace then becomes **none**, because the inserted string
  overwrites what that selection covers; two or more are **unchanged**, because
  the first Backspace consumes the selection, which is the one real character it
  would have deleted anyway. The arithmetic holds with or without an
  autocomplete selection, which is why no omnibox-focus test is needed.
- **Safari and Firefox** (plus Safari Technology Preview and Firefox Developer
  Edition) — one invisible character (`U+200B`, the single named constant
  `SELECTION_COMMIT_CHARACTER`) typed before the Backspaces, which makes the
  browser commit and dismiss the suggestion, and then **one extra Backspace** to
  remove it again. `Shift`+`Left` is deliberately not used here: WebKit's
  selection anchoring makes it unreliable.

Both lists are one table, `BROWSERS`, and adding a browser is one row.

**An application in neither list is left exactly as it was.** That is the
deliberate reading of "everything else": treating every unrecognised application
as WebKit would type an invisible character into every text field on the system
to fix a problem only browsers have.

Three guards skip both strategies, because getting any of them wrong destroys
text the user typed: nothing to delete, a plan the engine is also passing the
original key through with (`OutputPlan` has no separate "do not touch preceding
text" flag and `pass_through` is the nearest signal it carries), and a plan with
nothing to insert. Guards answer "post it verbatim", which is what this host did
before the workaround existed.

**Start of field cannot be detected**, and this is the limitation to know. No
CoreGraphics API reports the caret's offset in someone else's text field without
an Accessibility query per keystroke. The proxy is `delete_before > 0`: the
engine only asks for Backspaces it believes it rendered itself, and the composer
forgets that belief on a mouse-down, an arrow key, a focus change or a
target-process change. The residual risk is a caret moved by something the tap
cannot observe — and there the *existing* Backspace rewrite is already deleting
the wrong characters, so neither strategy is what broke it.

**Accepted trade-off.** A bundle identifier cannot say whether focus is in the
address bar or in a page input, so the workaround runs for the whole
application. For the Chromium strategy that is free. For Safari and Firefox it
means every tone mark typed into an ordinary in-page input also costs one
invisible insert and one extra Backspace, which a page can observe as extra DOM
`input` events; the resulting text is unchanged. The invisible character is
emitted from one place so a future focus test can narrow this.

The whole behaviour is behind **Sidebar → Input method → Browser address bars**,
default on, persisted as `browser_address_bar_fix` in `input-method.json`. That
field was added **without** a schema bump: it is a defaulted `bool` only dodo's
Event Tap reads, and a bundle that has never heard of it ignores one unknown key
rather than refusing the whole file. The row is drawn only under the Event Tap
backend — Native composes through a marked-text client and has no Backspace
rewrite for a selection to land in the middle of.

The frontmost application's bundle identifier is cached by an
`NSWorkspaceDidActivateApplicationNotification` observer, seeded once when the
tap starts. **Nothing asks `NSWorkspace` anything on the keystroke path.** All
the extra events are posted through the same queue as every other synthetic
event, in staging order, so "before the Backspaces" is a property of the
descriptor list rather than of two racing post APIs.

**Not verified: any real browser.** The count arithmetic, every guard, the
bundle-ID routing (including an unknown identifier) and the staged descriptor
sequences are unit tested; no browser was driven from this environment, so
whether `Shift`+`Left` and `U+200B` actually clear the selection in each engine
is the captain's to confirm.

Only one backend transforms at a time. Selecting Native stops Event Tap before
writing settings. Selecting Event Tap waits for a live Native Input Method to
adopt the selection; a new native bundle then passes keys through. Event Tap
stays attached in **every** selected language, not only Vietnamese — it owns the
language-switch shortcut while it runs, so a tap that stopped in English could
never switch back. The settings schema is version 8, whose history is in
`dodo_ime_ipc::settings::SETTINGS_SCHEMA_VERSION`; an older host refuses a newer
file and falls back to English/pass-through rather than compose beside a
selected fallback. Windows details are in
[`windows-input-method.md`](windows-input-method.md).

## 4. Verifying a change

```sh
cargo test --locked            # includes the crate's unit tests
cargo test -p dodo-ime-macos --test controller
```

The second is the interesting one. `crates/dodo-ime-macos/tests/controller.rs`
constructs a real `IMKServer`, a real `DodoInputController` through the real
Objective-C runtime, and drives it with `inputText:key:modifiers:client:` against
a mock `IMKTextInput` client that records what it was told. It is the only thing
that can catch a mistyped selector — `define_class!` takes selector names as
string literals, so a typo compiles, registers a method nobody calls, and
produces an input method that installs correctly and types nothing.

Everything that could get Vietnamese *wrong* is in the pure modules (`keymap`,
`text`, `ops`, `session`) and is tested without a window server.

## 5. What was verified, and what was not

**Verified for the signing fix, in an isolated worktree:** the locally assembled
bundle is ad-hoc signed and passes `codesign --verify --deep --strict`. This is
structural signature validation only; it does **not** verify System Settings
naming, selection, or whether signing changes the historical
`TISSelectInputSource -50` result. Those remain for the captain to test.

**Previously verified on macOS 26.6 (build 25G72), Apple Silicon:**

- The bundle builds, installs to `~/Library/Input Methods/`, registers, and
  appears in the Text Input Sources database as `enabled=YES`,
  `enable_capable=YES`, `select_capable=YES`, with its localised name resolving
  to `Dodo Vietnamese` from `InfoPlist.strings`.
- The executable launches from the installed bundle, creates its `IMKServer`,
  and stays in its run loop.
- The controller class registers under the exact name `Info.plist` names.
- End to end **in process**: the real controller, driven through the real
  Objective-C runtime, produced `tiếng`, `chào` and `viêt` via
  `setMarkedText:selectionRange:replacementRange:` and
  `insertText:replacementRange:`, in the right order, with the right ranges — and
  handed back every key that was not its business.

**Added by the install-and-IPC round, same machine.** Every one of these was run
against the real system, and the machine was left exactly as it was found:
`~/Library/Input Methods` empty, `defaults export com.apple.HIToolbox` identical
byte for byte (same MD5 before and after), no `DodoVietnamese` process, and no
input-method files in the real `~/Library/Application Support/dodo`.

- **The install sequence works, twice.** Driven through the real
  `services::installer::install` against the real Text Input Sources API:
  `ditto` copied the bundle, `TISRegisterInputSource` returned `0`, and the mode
  `io.github.mrgru.dodo.inputmethod.Dodo.Vietnamese` was visible in
  `TISCreateInputSourceList` on the **first** attempt, so the retry loop did not
  have to fire. `TISEnableInputSource` returned `0`. Then the whole thing again
  over the top — the upgrade path — with identical results.
- **`TISSelectInputSource` returned `-50`**, as §5 already recorded for every
  input source on this machine. The install action reports that as
  *installed but not switched to*, with the number and what to do about it.
- **Two concurrent `TISCreateInputSourceList` calls abort the process.** `SIGABRT`
  from `islGetInputSourceListWithAdditions.cold.3` inside HIToolbox, three threads
  standing in `TISCreateInputSourceList` in the crash report. Found because
  `cargo test` runs tests in parallel and `services::tis`'s tests query the
  database. Calling TIS from a *non-main* thread is fine — every one of the
  verifications above did — but two at once is not, and AppKit makes its own TIS
  calls on the main thread. Hence a process-wide lock in `tis` **and** the
  main-queue hop in `SystemOps`.
- **The two files and the notification, across two processes.** With `HOME`
  pointed at a scratch directory: the bundle read `input-method.json` at launch
  (VNI, traditional tone placement, revision 9) and wrote
  `input-method-status.json` naming its own version, pid and applied revision; a
  distributed notification posted from a *second* process made it re-read a changed
  file within one run-loop tick (Telex, modern, revision 6 in an earlier pass) and
  rewrite the status file; and a `"version": 99` file made it fall back to the
  compiled-in defaults and report `settings-revision: 0`, which is the signal dodo
  shows as "the input method has not picked these settings up yet".

**Not verified: dodo's own UI or Event Tap.** The install button, the pane it
sits on, Event Tap's Accessibility state, and the notification dodo posts were
not exercised through a running dodo — a GUI cannot be driven from this
environment. What was exercised is the code underneath the native controls: the
installer driver against the real system, the store against the real file names,
and the notification through the same
`CFNotificationCenter::post_notification` call and the same shared name constant
that `services::notify` uses, posted from a small harness rather than from dodo.

**Not verified: typing into another application.** No input method could be made
the active input source on the test machine. `TISSelectInputSource` returned
`-50` for the dodo mode — and, the control that settles it, **for Apple's own
`com.apple.inputmethod.VietnameseIM.VietnameseTelex` too**, along with Ainu and
Kotoeri Hiragana, all of which report `enabled=YES` and `select_capable=YES`.
`NSTextInputContext.keyboardInputSources` listed only `com.apple.keylayout.ABC`
in the same session. `TISEnableInputSource` returned `0` without effect, and
writing the entry into `AppleEnabledInputSources` directly did not change it
either. So this is a property of the session, not of dodo's bundle: on every
attribute the API exposes, the dodo mode is indistinguishable from Apple's own.

Closing that gap needs a machine where input-source switching works, and the
check is then: install, add the source in System Settings, and type into TextEdit
and Terminal.

Three notes for whoever does it. Apple's own **`com.apple.PressAndHold`
interferes with synthetic key events on accent-bearing letters** — the
investigation saw `a` produce `â` instead of `á` — so a scripted harness should
prefer real typing or expect that. A harness that posts `CGEventPost` to itself
must post from a **background thread**: sleeping on the main thread blocks the
run loop, the events queue and never dispatch, and the result reads exactly like
"the input method typed nothing". And `TISCreateInputSourceList(NULL, true)`
returns sources that cannot be handed to `TISSelectInputSource`.

**Not re-verified by signing:** System Settings naming and selection, and
whether a valid signature changes the historical `TISSelectInputSource -50`
result. The prior `-50` control remains evidence only about that session, not a
resolution of selection.

**Also not verified:** Intel macOS, macOS earlier than 26, Developer ID signing,
notarisation, and any behaviour in Chrome, VS Code or Electron. The
investigation report's §6 capability matrix — which measured `setMarkedText:` and
`insertText:` working in all six clients it probed — is the evidence that the
composing path is the right one, and it is not evidence about this build.

## 6. Corrections to the investigation report

Three, all measured while building this:

1. **`CFBundleIdentifier` must contain `.inputmethod.` as an infix**, not merely
   end in `.inputmethod`. The report carried this as a **READ** note from
   `xkey`'s README with no counter-example tried. It is a hard requirement:
   `io.github.mrgru.dodo.inputmethod` never appeared in the input-source list,
   `io.github.mrgru.dodo.inputmethod.Dodo` did, and `TISRegisterInputSource`
   returned `0` for both while logging nothing.
   `crates/dodo-ime-macos/src/bundle.rs` has the eight-bundle table.
2. **`IMKInputController` validates both of its `init` arguments.** A nil server
   aborts, and a client that is not a real IMK proxy raises
   `NSInvalidArgumentException: unexpected client proxy of class …`. This is why
   the boundary test constructs the controller with a nil *client* and passes the
   mock as `sender:` — which is also why nothing in `controller.rs` reads
   `-[self client]`.
3. **The nesting location is `Contents/Helpers/`**, not
   `Contents/Library/InputMethods/`. That correction is `docs/macos-signing.md`
   §7.2's and predates this round; it is repeated here because it is the one
   choice that would have been expensive to change later.

## 7. Installing it from dodo

**Sidebar → Input method → Install.** It is a **tool**, not a settings page: the
sidebar's last row on macOS, drawing a keyboard, and it does not exist on the
other two platforms because the bundle it installs is an InputMethodKit object.
The captain asked for that on 2026-08-09, and the move took the whole surface —
the backend choice, status line, install button and four engine settings are on
the pane and nowhere else, so no control is reachable from two places.
`src/input_method/` is the implementation and its module docs are the authority;
this is what the button does and why.

The six steps, in this order, are §2's recipe as code —
`src/input_method/models/install.rs` holds them as data with a test each, and
`services/installer.rs` is a driver with no judgement in it:

1. **Verify the source signature** with `codesign --verify --deep --strict`.
   An invalid bundle is rejected before it can replace an installed one or be
   registered, and the button reports `codesign`'s detail instead of success.
2. **`ditto`** the verified bundle to `~/Library/Input Methods/Dodo Vietnamese.app`,
   after removing whatever was there. Naming the bundle in the destination is not
   optional (§2); removing first is what makes an upgrade *replace* rather than
   merge, since `ditto` never deletes a file the new version dropped.
3. **`TISRegisterInputSource`, in a loop**, until the mode is visible in
   `TISCreateInputSourceList` — up to five attempts, 700ms apart. The return value
   is never consulted, because §2 measured it returning `0` for a bundle that then
   did not exist.
4. **`TISEnableInputSource` on the mode.**
5. **`TISSelectInputSource` on the mode.**
6. **`pkill -x DodoVietnamese`**, last. Replacing the bundle does not restart the
   process serving from it (§2), and `-x` matches the process *name* so it cannot
   catch an unrelated command line that happens to contain the string.

Where the bundle is copied *from*: `<dodo.app>/Contents/Helpers/Dodo
Vietnamese.app` if dodo is in a bundle, otherwise `./dist/Dodo Vietnamese.app`,
which is what makes the button usable from a `cargo run` build. A dodo with
neither says so rather than failing obscurely — and that is every released dodo
today, because `scripts/package.sh` still does not pass `--input-method` (§9).

Two things about this that are not obvious:

- **The four TIS calls happen on the main queue**, hopped to from the background
  executor with `dispatch_sync`. Not because TIS needs the main thread — it does
  not — but because *two concurrent* `TISCreateInputSourceList` calls abort the
  process, and AppKit makes its own TIS calls on the main thread where no lock of
  dodo's can serialise against them. §5 has the crash. The copy, the `pkill` and
  the retry sleeps stay off the main thread.
- **A refused `TISSelectInputSource` is reported as a success with a caveat**, not
  as a failure: "Installed, but macOS would not switch to it (error −50). Turn it
  on in System Settings → Keyboard → Input Sources." On this machine that is what
  *every* input source does (§5), so calling it dodo's fault would be a lie.

## 8. Settings, and the two files

dodo writes what the input method should type like; the input method says what it
has applied. `crates/dodo-ime-ipc` is the contract and its crate docs are the
authority — it exists because neither process can link the other's code, so the
alternative was two copies of one schema kept in step by nothing.

```text
  Dodo.app  ──writes──▶  input-method.json         ──reads──▶  the bundle
  Dodo.app  ◀──reads──   input-method-status.json  ◀─writes──  the bundle
```

Both live under `data_dir()` beside dodo's other nine files. **One writer each**,
which is the whole concurrency design: no lock file, no advisory locking, and
every write is a temp file plus `rename`, so a reader sees one complete version or
the other and never half of either.

`input-method.json` carries the selected backend, the selected keyboard language,
the language-switch shortcut, and the four Vietnamese settings the page offers —
input scheme (Telex or VNI), tone-mark placement (modern `hoà` or traditional
`hòa`), spell check, bracket shortcuts — plus a `revision` dodo bumps on every
write. The menu bar and the bundle share
that language identity; English and Japanese pass keys through until native
engines exist. The output mode is deliberately **not** persisted: Native Input
Method always composes, while Event Tap deliberately uses direct rewriting
because it has no marked-text client.

`input-method-status.json` carries the bundle's version, its pid, when it started
and **the revision it has applied**. That last field is the only thing that can
distinguish "your change arrived" from "the settings happen to agree", and it is
what the status row means when it says the input method has not picked the
settings up yet. See §3 for the privacy constraint on this file, which is absolute.

The ping is a `CFNotificationCenter` **distributed notification**,
`io.github.mrgru.dodo.inputmethod.Dodo.settings-changed`, posted after the file is
written and never before — a notification that arrives first makes the bundle read
the previous settings and report the previous revision. It carries no payload: the
file is the payload. The CF spelling rather than
`NSDistributedNotificationCenter` because its observer is a plain `extern "C"`
function pointer, so the bundle gains no Objective-C class and no `block2`.

Both parsers refuse a `"version"` above the one they know rather than half-reading
it — `environments.json`'s pattern, not `collections.json`'s — and here it matters
more than anywhere else in dodo, because the two processes are updated
independently: a months-old bundle in `~/Library/Input Methods` reading a new
dodo's settings file is an ordinary situation, not an exotic one. A bundle that
refuses the file uses its English/pass-through default and reports revision `0`,
so dodo can say so.

### 8a. The language-switch shortcut, and the one combination this host cannot see

The shortcut is `{ modifiers, key }` — schema 8, `dodo_ime_ipc::settings::Shortcut`
— where `key` is one of the engine's non-printing keys or the literal
`"modifiers"`, meaning the modifiers *are* the shortcut. Every host matches it the
same way, against a normalized `KeyEvent`, so `⌘` and the Windows key are one
field and so are `⌥` and Alt. A printing key is not in the vocabulary: this host
is handed what a key *types*, and `⌥Z` arrives as `Ω`, so a shortcut recorded from
a letter could never be recognised here.

**This bundle never sees a modifier-only shortcut**, and that is the one thing
about the flow that is not symmetric. `recognizedEvents:` in `controller.rs` is
`NSEventMaskKeyDown` alone — deliberately, because widening it takes over
InputMethodKit's own mouse handling, and `commitComposition:` on a click outside
the composition comes free from leaving it narrow. macOS therefore delivers a bare
`⇧` to nothing here: `inputText:key:modifiers:client:` is called for key-downs and
`FlagsChanged` reaches only `handleEvent:client:`, which InputMethodKit picks
*instead of* `inputText:` when a controller implements it. Receiving one means
rewriting the whole key path through `NSEvent` — including whether
`NSEvent.characters` still resolves dead keys the way `inputText:` does — and that
is a round of its own, with captain testing, not a line to add here.

Until then: `⌃⇧Space` and every other combination that ends in a key works under
Native Input Method, and a modifier-only combination needs the Event Tap backend,
which reads `CGEventType::FlagsChanged` directly. The Input method pane says so
beneath the recorder rather than showing a setting that does nothing.

## 9. What the next round has to add

- **`handleEvent:client:`, for a modifier-only shortcut under Native Input
  Method.** See §8a for why the current event mask cannot deliver one and what
  changing it costs. `tests/controller.rs` is where the replacement key path
  would have to be driven against the mock client, and only a captain at a real
  keyboard can confirm dead keys and non-QWERTY layouts still work afterwards.

- **Wiring the bundle into the release.** `scripts/package.sh` does not pass
  `--input-method` yet, so a shipped `dodo.app` carries no input method and the
  install button on a released build can only report that. Doing so means building
  `DodoVietnamese` in the macOS release rows and is a change to
  `.github/workflows/release.yml`.
- **A menu-bar icon.** `tsInputMethodIconFileKey` is unset, so the input menu
  shows the name with no glyph. It wants a `.pdf` or `.tiff`, which
  `scripts/generate-icons.py` does not produce.
- **Developer ID signing and notarisation.** Local builders already ad-hoc sign
  the bundle, and the outer builder signs the nested input method before dodo.app.
  `docs/macos-signing.md` is the authority for replacing that with a shared Team
  ID, notarisation and release wiring.

## 10. Strings

The bundle has exactly two user-visible strings — the input method's name and
its mode's — and they do **not** go through dodo's `i18n::Str`. Two reasons, and
the second is the real one: `Str` lives in the `dodo` crate, which this bundle
must not link; and *macOS* reads these, not dodo. They live in
`Contents/Resources/<lang>.lproj/InfoPlist.strings`, keyed by the input-source
id, and System Settings picks the `.lproj` matching the **system** language —
which is a different setting from dodo's interface language and is not expected
to agree with it. `en` and `vi` ship today; adding one is one more `.lproj` in
`scripts/macos-input-method-bundle.sh`.

Without them the input-source list shows the raw identifier: the investigation
watched System Settings render `dev.dodo.inputmethod.poc.Vietn…`.
