# dodo's macOS input method

`crates/dodo-ime-macos` builds **Dodo Vietnamese.app**, an InputMethodKit input
method that types Vietnamese using `crates/dodo-ime-core`. macOS launches it;
`Dodo.app` does not, and typing keeps working with dodo closed.

This round's scope stops at *it types*. There is no install button, no IPC with
`Dodo.app`, no settings page and no tray wiring — §6 lists what the next round
owes. The design rationale lives in the crate's module docs, which are the
authority; this file is how to build, install and enable it by hand, and what
was and was not verified.

Two documents sit behind it: the investigation report that proved the approach
(`dodo-ime-macos-scout`), and `docs/macos-signing.md`, which constrains where
the bundle is nested. Where this file disagrees with the report, this file is
the later measurement — §5 lists the three corrections.

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
`scripts/package.sh` still does — see §6.

macOS never looks inside `dodo.app` for an input method. That copy exists so a
later round's install action has something to copy out. The location is fixed by
`docs/macos-signing.md` §7.2: `codesign` discovers nested code in a fixed set of
directories, `Contents/Library/InputMethods` is not one of them, and a bundle
placed there is sealed as an opaque resource rather than as code.

## 2. Install and enable, by hand

```sh
ditto "dist/Dodo Vietnamese.app" ~/Library/Input\ Methods/
```

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
source in the same System Settings pane, and `pkill -f DodoVietnamese`.

## 3. What it does

Telex, modern tone placement, spell check and bracket shortcuts on — the
compiled-in defaults in `dodo_ime_macos::DEFAULT_CONFIG`, which are Unikey's.
They cannot be changed yet; that is the settings round.

`tieengs` composes `tiếng`; `dduowngf` composes `đường`; a space, a comma, an
arrow key or a command shortcut commits the syllable and reaches the application
untouched. English that Telex would mangle survives, because the engine's spell
check hands back the keys as typed when the result is not a Vietnamese syllable:
`where` stays `where`. `test` still becomes `tét`, exactly as it does in Unikey
— `tét` *is* a Vietnamese syllable and nothing can tell the two intentions apart.

**No typing history, ever.** The bundle writes no file, opens no socket and
prints nothing about what was typed. The only state that outlives a keystroke is
the syllable being composed, and `commitComposition:` / `deactivateServer:` —
which is how macOS reports a password field — drop it.

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

## 7. What the next round has to add

- **The install action in dodo's UI**: copy the nested bundle out with `ditto`,
  `TISRegisterInputSource`, `TISEnableInputSource` and `TISSelectInputSource` on
  the *mode*, and `kill` any running `DodoVietnamese` so an upgrade takes effect.
  `crates/dodo-ime-macos/src/bundle.rs` holds the identifiers it needs.
- **Wiring the bundle into the release.** `scripts/package.sh` does not pass
  `--input-method` yet, so a shipped `dodo.app` carries no input method. Doing so
  means building `DodoVietnamese` in the macOS release rows and is a change to
  `.github/workflows/release.yml`; it is deliberately not done here, because a
  nested bundle that nothing can install is weight without a use.
- **Settings and IPC.** The report's §7 design — two single-writer JSON files
  under `data_dir()` plus an `NSDistributedNotificationCenter` ping — is
  unimplemented. `DEFAULT_CONFIG` is what the bundle types with until it exists.
  Copy `environments.json`'s explicit-`"version"`-and-refuse-if-higher pattern,
  not `collections.json`'s.
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

## 8. Strings

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
