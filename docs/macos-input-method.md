# dodo's macOS input method

`crates/dodo-ime-macos` builds **Dodo Vietnamese.app**, an InputMethodKit input
method that types Vietnamese using `crates/dodo-ime-core`. macOS launches it;
`Dodo.app` does not, and typing keeps working with dodo closed.

dodo can now **install it** (§7) and **tell it how to type** (§8). What is still
missing is the release wiring, the tray mark, a menu-bar icon and signing — §9
lists them. The design rationale lives in the crate's module docs, which are the
authority; this file is how to build, install and enable it by hand, what dodo
does when it does that for you, and what was and was not verified.

Two documents sit behind it: the investigation report that proved the approach
(`dodo-ime-macos-scout`), and `docs/macos-signing.md`, which constrains where
the bundle is nested. Where this file disagrees with the report, this file is
the later measurement — §6 lists the three corrections.

---

## 1. Build

```sh
scripts/macos-input-method-bundle.sh
```

That builds `target/release/DodoVietnamese` and assembles
`dist/Dodo Vietnamese.app`. Pass `--binary` to use one you already have,
`--out` to put the bundle somewhere else.

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

**Verified on macOS 26.6 (build 25G72), Apple Silicon:**

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

**Not verified: dodo's own UI.** The install button, the pane it sits on and the
notification dodo posts were not exercised through a running dodo — a GUI cannot
be driven from this environment. What was exercised is the code underneath all
three: the installer driver against the real system, the store against the real
file names, and the notification through the same
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

**Also not verified:** Intel macOS, macOS earlier than 26, signing and
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
the status line, the install button and the four engine settings are on the pane
and nowhere else, so no control is reachable from two places.
`src/input_method/` is the implementation and its module docs are the authority;
this is what the button does and why.

The five steps, in this order, are §2's recipe as code —
`src/input_method/models/install.rs` holds them as data with a test each, and
`services/installer.rs` is a driver with no judgement in it:

1. **`ditto`** the bundle to `~/Library/Input Methods/Dodo Vietnamese.app`, after
   removing whatever was there. Naming the bundle in the destination is not
   optional (§2); removing first is what makes an upgrade *replace* rather than
   merge, since `ditto` never deletes a file the new version dropped.
2. **`TISRegisterInputSource`, in a loop**, until the mode is visible in
   `TISCreateInputSourceList` — up to five attempts, 700ms apart. The return value
   is never consulted, because §2 measured it returning `0` for a bundle that then
   did not exist.
3. **`TISEnableInputSource` on the mode.**
4. **`TISSelectInputSource` on the mode.**
5. **`pkill -x DodoVietnamese`**, last. Replacing the bundle does not restart the
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

`input-method.json` carries the four settings the page offers — input scheme
(Telex or VNI), tone-mark placement (modern `hoà` or traditional `hòa`), spell
check, bracket shortcuts — plus a `revision` dodo bumps on every write. The
output mode is deliberately **not** a setting: macOS always has a marked-text
channel, so the host always composes.

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
refuses the file types with `DEFAULT_CONFIG` and reports revision `0`, so dodo can
say so.

## 9. What the next round has to add

- **Wiring the bundle into the release.** `scripts/package.sh` does not pass
  `--input-method` yet, so a shipped `dodo.app` carries no input method and the
  install button on a released build can only report that. Doing so means building
  `DodoVietnamese` in the macOS release rows and is a change to
  `.github/workflows/release.yml`.
- **The tray mark.** `src/tray/input_language.rs` is presentational; once the IME
  exists the truth about "which language am I typing in" lives in its process.
  The report's §13.4 flags that this really is the same concept as
  `tray::InputLanguage` — confirm that reading before wiring it, because
  `AGENTS.md` is emphatic that dodo's two *existing* language settings never
  merge, and a future session could easily merge the wrong pair.
- **A menu-bar icon.** `tsInputMethodIconFileKey` is unset, so the input menu
  shows the name with no glyph. It wants a `.pdf` or `.tiff`, which
  `scripts/generate-icons.py` does not produce.
- **Signing.** `docs/macos-signing.md` is the authority. The bundle needs no
  entitlements, must be signed with the hardened runtime and the same Team ID as
  `dodo.app`, and must be signed **before** the outer bundle.

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
