# dodo's Windows input method

Windows has two Vietnamese backends in **Sidebar → Input method**. They share
`dodo-ime-core`, `LanguageId`, and `input-method.json`, but deliberately make
different trade-offs.

- **Native TSF** is `crates/dodo-ime-windows`, a COM Text Services Framework
  DLL. Install it once for the current user, select **Dodo Vietnamese** in
  Windows input methods, and it is designed to type when dodo is closed.
- **Keyboard Hook** is a no-install `WH_KEYBOARD_LL` fallback owned by dodo. It
  works only while dodo is running. It is useful when a machine cannot register
  the TSF profile; it is not a replacement for TSF.

Only the selected backend may transform. dodo writes the selection atomically
before starting Keyboard Hook; TSF re-reads that completed file before deciding
a key, so a switch favours a harmless pass-through gap over two transformers.
Linux has no row until it has an IBus host. macOS remains InputMethodKit/Event
Tap; see [`macos-input-method.md`](macos-input-method.md).

## The DLL's shape

`crates/dodo-ime-windows` is a `cdylib` that **Windows** loads into other
people's applications, independently of dodo — dodo neither links it nor starts
it. It links only the pure engine (`dodo-ime-core`) and the IPC contract
(`dodo-ime-ipc`), which is the same rule every native host follows: no gpui, no
HTTP client, nothing from the UI application, ever, in somebody else's input
path.

Its TSF edit session performs **marked composition** rather than injecting keys,
which is the difference between it and the Keyboard Hook fallback and the reason
only the fallback can get a browser address bar wrong. It re-reads the settings
file before each key, so selecting Keyboard Hook makes TSF pass through with no
restart of anything.

`input-method.json` and `input-method-status.json` are the two files the two
processes exchange; the contract, its single-writer rule and its version rule are
documented once, in [`macos-input-method.md`](macos-input-method.md) §8, and
apply here unchanged except for the wake mechanism — Windows uses a named event
where macOS posts a distributed notification.

## Native TSF install, reinstall, and removal

A Windows release ZIP contains:

```text
dodo.exe
input-method/dodo_ime_windows.dll
```

Select **Native TSF**, then press **Install** (or **Reinstall**). dodo copies
the DLL to `%APPDATA%\dodo\input-method\dodo_ime_windows.dll` and invokes the
standard 64-bit `regsvr32.exe` with `/s`. The DLL's `DllRegisterServer` writes
only `HKCU\Software\Classes\CLSID\{B97610DC-4C6B-457D-9B44-AD82B79A6789}` and
uses `ITfInputProcessorProfiles` to add its Vietnamese profile. No driver,
service, administrator prompt, or elevated helper is involved.

An in-app dodo update replaces both `dodo.exe` and this packaged sidecar, so a
later **Install/Reinstall** can consume the new DLL. It does **not** automatically
replace or re-register the `%APPDATA%` copy; registration remains an explicit
button action until captain runtime testing settles that policy.

Then select **Dodo Vietnamese** from Windows' input-language controls. Windows
may place the profile under the installed Vietnamese language's keyboard/input
method list; use the Windows Settings search for **input method** if the exact
Settings navigation differs by Windows version.

**Uninstall** reverses the registration with `regsvr32 /u /s` and removes the
copied DLL. If dodo cannot start, use an elevated prompt only if Windows itself
reports a policy restriction; ordinary per-user removal is:

```powershell
$dll = Join-Path $env:APPDATA 'dodo\input-method\dodo_ime_windows.dll'
& "$env:SystemRoot\System32\regsvr32.exe" /u /s $dll
Remove-Item $dll -ErrorAction SilentlyContinue
```

Then remove **Dodo Vietnamese** from Windows' input methods if it remains
listed. Do not delete unrelated CLSID keys or run an installer/registry cleaner.

## Keyboard Hook fallback

Select **Keyboard Hook**. It needs no install or Windows setting change, but it
starts only while dodo is running. Closing dodo, selecting Native TSF, or a
failed settings write drops the hook deterministically. It is **not** dropped
when the selected language leaves Vietnamese: while it runs it owns the
language-switch shortcut, and a hook that stopped in English could never switch
back. In a language with no engine it observes the shortcut and passes
everything else through.

The hook processes only one known plain key-down. It passes key-up, repeat,
shortcut, injected, dead-key/ligature, unknown, and error paths unchanged, and
a matched modifier-only shortcut is passed on as well — swallowing a modifier
would leave every application believing the key is held. Its own `SendInput`
output carries a private extra-info tag and is ignored on re-entry. It never
busy-loops and `Drop` calls `UnhookWindowsHookEx` before releasing callback
state. Recording a new shortcut reconfigures the one live hook rather than
installing a second, so the combination that was recorded over stops matching
immediately and without a restart.

Windows' low-level-hook callback does **not** reveal whether a normal foreground
text field is a password field. It therefore passes secure-desktop/foreground
uncertainty unchanged, but cannot promise password-field detection that the API
does not expose. Do not select Keyboard Hook for password entry; use Native TSF
instead. TSF requests only writable contexts and relies on TSF's secure-context
routing, but both claims still require the hands-on test below.

Neither backend logs, persists, sends, or exposes raw keystrokes. The only
persistent input-method data is the selected backend/language/engine settings
in `input-method.json`; neither backend records typed text.

## Where a key's case comes from (2026-08-14)

`GetKeyboardState` answers **per calling thread**, and only advances as that
thread reads key messages from its own queue. In the hook that thread is dodo's,
in the background, so its copy is frozen at whatever dodo last saw when it had
focus. Every reported Windows symptom followed from that one fact:

- Shift read as up, so `ToUnicodeEx` returned the unshifted character. No
  capital letter could reach the engine, and every rewritten syllable came back
  lowercase — visible as a letter that looked right until the rewrite changed
  it, which reads exactly like "the casing follows the wrong key".
- The engine `Modifiers` came from the same array, so they were always empty.
  A recorded shortcut must hold a command modifier and is compared exactly, so
  **no** shortcut could ever match and the language switch did nothing.
- Caps lock's toggle bit went stale for the same reason.

The hook now **builds** the 256-byte array instead of fetching it
(`models::keyboard_hook::layout_state`): the physical keys come from
`GetAsyncKeyState`, which is not queue-bound; the arriving key folds itself in,
because a low-level hook runs before Windows records the press and that press is
what a modifier-only shortcut fires on; and caps lock is tracked from one
snapshot taken while dodo still had focus. Building rather than merging matters
here — a stale array is worse than an empty one, since a leftover Control byte
makes `ToUnicodeEx` return a control character for an ordinary letter.

The TSF DLL runs on the application's own thread while it is handling the very
key press, so its snapshot should already be right; it therefore **merges** the
physical modifiers in rather than rebuilding (`keymap::merge_physical`), which
can only add a modifier the user is physically holding. It also now passes the
real scan code from `lParam` bits 16-23; `ToUnicodeEx` documents the parameter
and a zero there is not the same key on every layout.

Both hosts read the character and the modifier flags out of the **same** array,
so a `shift` flag can no longer disagree with the case of the character beside
it.

Two gates were also removed from the language switch: the hook required a
focused edit control and TSF required a writable context before either looked at
the shortcut, so a window with nowhere to type could not change language. Both
now match the shortcut first, and a switch with no composition in flight needs
no TSF edit session at all.

All of the above is pure and unit-tested from the Mac development host. **None
of it has been executed on Windows.** Step 3a below is what would prove it.

## Build and Windows verification

The TSF host is a workspace default member. On a Windows developer machine:

```powershell
cargo fmt --all --check
cargo test -p dodo-ime-windows --locked
cargo check --all-features --locked
cargo build --release --locked
pwsh scripts/package.ps1
```

`cargo test -p dodo-ime-windows` includes a Windows-native COM class-factory
harness. It does **not** install/register anything and does not generate input.

After CI has produced a ZIP, a captain should test on a disposable Windows user
account:

1. Unpack the ZIP; confirm `input-method\dodo_ime_windows.dll` is present.
2. Start `dodo.exe`, choose Native TSF, press Install, and confirm the profile
   appears and can be selected without an administrator prompt.
3. Close dodo. In Notepad, type `tieengs ` and `dduowngf `; confirm `tiếng` and
   `đường` appear. Test Backspace, arrow keys, Ctrl+S, and a password field.
3a. **Casing, under both backends and in Chrome as well as Notepad.** Type
   `DDuwowngf ` and confirm `Đường`; `Vieetj ` for `Việt`; `TIEENGS ` for
   `TIẾNG`. Then `dD` for `Đ` and `Dd` for `đ` — the case follows the second
   `d`, which is the shared engine's rule and not a Windows one. Turn caps lock
   on and repeat one of them. Finally confirm the tray mark is legible: switch
   the taskbar between light and dark in Settings, restart dodo, and check the
   dodo is not a black smudge on either.
4. Reopen dodo, choose Keyboard Hook, keep dodo open, and repeat the Notepad
   test. Confirm key-up/repeat/shortcuts still behave normally and closing dodo
   stops transformation.
5. Switch between both backends repeatedly, then press Uninstall and confirm
   TSF disappears while Keyboard Hook still needs no cleanup.
6. On the Input method pane, click the language-switch field and press
   `Ctrl+Shift+Space`; the field should read `Ctrl Shift Space`. In Notepad,
   confirm it cycles the enabled languages under **both** backends, that a
   language with no engine types through, and that it still switches back.
7. Record `Alt+Space` over it. Confirm `Ctrl+Shift+Space` no longer switches
   anything — with no restart — and that `Alt+Space` does. Restart dodo and
   confirm the recorded shortcut is still `Alt Space`.
8. Record `Ctrl+Shift` on its own (hold both, then release). Confirm it
   switches under both backends, that the modifiers still reach applications —
   `Ctrl+C` in Notepad still copies — and that turning Beep on sounds once per
   switch. Whether TSF delivers a bare modifier to `OnKeyDown` at all is the
   open question here; `keymap::key_event` names them, but only this step can
   say whether the host is asked. If Native TSF does not switch and Keyboard
   Hook does, that is the answer and it matches the macOS bundle's known
   limitation in `macos-input-method.md` §8a.

No Windows runtime typing, key casing, language switching, tray-icon rendering,
registration, profile visibility, secure-context behaviour, or release-archive
run has been verified from the Mac development host. The Windows CI runner compiles/tests the host but does not select a
profile or type into another application; captain testing remains the runtime
arbiter.
