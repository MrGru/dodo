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

Select **Keyboard Hook**. It needs no install or Windows setting change, but
it starts only while dodo is running and Dodo's keyboard input language is
Vietnamese. Closing dodo, selecting Native TSF, changing language away from
Vietnamese, or a failed settings write drops the hook deterministically.

The hook processes only one known plain key-down. It passes key-up, repeat,
shortcut, injected, dead-key/ligature, unknown, and error paths unchanged. Its
own `SendInput` output carries a private extra-info tag and is ignored on
re-entry. It never busy-loops and `Drop` calls `UnhookWindowsHookEx` before
releasing callback state.

Windows' low-level-hook callback does **not** reveal whether a normal foreground
text field is a password field. It therefore passes secure-desktop/foreground
uncertainty unchanged, but cannot promise password-field detection that the API
does not expose. Do not select Keyboard Hook for password entry; use Native TSF
instead. TSF requests only writable contexts and relies on TSF's secure-context
routing, but both claims still require the hands-on test below.

Neither backend logs, persists, sends, or exposes raw keystrokes. The only
persistent input-method data is the selected backend/language/engine settings
in `input-method.json`; neither backend records typed text.

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
4. Reopen dodo, choose Keyboard Hook, keep dodo open, and repeat the Notepad
   test. Confirm key-up/repeat/shortcuts still behave normally and closing dodo
   stops transformation.
5. Switch between both backends repeatedly, then press Uninstall and confirm
   TSF disappears while Keyboard Hook still needs no cleanup.

No Windows runtime typing, registration, profile visibility, secure-context
behaviour, or release-archive run has been verified from the Mac development
host. The Windows CI runner compiles/tests the host but does not select a
profile or type into another application; captain testing remains the runtime
arbiter.
