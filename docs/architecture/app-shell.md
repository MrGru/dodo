# The application shell

`src/` is the shell and the cross-feature services, and nothing else. Every *tool* lives in a
feature crate under `crates/`; `src/main.rs` aliases each one back to its former module name, so a
call site still reads `crate::cleaner::CleanerView` and the seam is invisible from the consumer's
side. Those aliases each carry a comment saying why that crate exists — read them rather than this
page if the question is "why is this a crate".

```text
src/main.rs        startup order, the crate aliases, the `paths` seam, app lifecycle
src/app.rs         the root view and the dialog layer
src/layout.rs      the sidebar, the pane, the width rule, quick navigation's routing
src/tools.rs       the tool table: one row per tool
src/settings/      the Settings dialog
src/session/       session.json — see docs/architecture/persistence.md
src/quick_nav/     paste routing; a feature, not a tool
src/tray/          the menu bar / notification-area item; a feature, not a tool
src/assets.rs      the rust-embed filters
src/build_info.rs  what build.rs embedded
src/window_icon.rs the runtime half of the application icon
src/i18n_lint.rs   test-only; stays at the top of src/ because its include_str! paths are relative
```

`src/tools.rs` is worth one sentence of orientation before you open it: it is a **table, not a
registry** — `View` stays an ordinary persisted enum, matched on exhaustively, with no trait
objects, no distributed slices, no build script and nothing discovered at runtime, because the win
being bought is co-location and not dynamism. Its module doc is the authority on what a row
carries; `dodo-tool-view` is the checklist for changing one. It has nothing to do with the
repo-root `tools/` directory, which holds the standalone `update-manifest` crate.

## Startup

`run_app` in `src/main.rs` has an ordering that is load-bearing and commented line by line:
`gpui_component::init` first, then the modules that bind keys (they have to run afterwards to win
the key-binding tie), then `session::load` and `settings::apply_session` **before the window
opens**, so the first frame is already the user's theme rather than a flash of the default. Every
`init` there returns `()` on purpose: a failed tray, updater, Docker, quick-nav or input-method
init must never stop dodo starting.

## Lifecycle, and the two halves that are counter-intuitive

**A release Windows build is a GUI-subsystem binary.** That is what keeps a console window from
sitting behind the app, and it costs the process valid standard handles — which is why
`attach_parent_console` exists and is called on the `--version` / `--build-info` path alone. A
debug `cargo run` keeps its console so panics stay visible.

The consequence that catches everyone is on the *other* side of that binary: **no shell waits for a
GUI-subsystem process at all**, so every Windows smoke test must run it through
`Start-Process -Wait`, never PowerShell's `&`. Capturing into a variable does **not** make the
shell wait; believing it did is what cost v0.1.5 its Windows archive, and the same wrong claim sat
in three comments and one doc before being corrected. "What 'verified' means" in
`docs/release.md` has the panic, the run IDs and the fix, and the two release workflows carry it in
their own comments.

**Closing the single window quits the app through `QuitMode::LastWindowClosed`** — GPUI's own
check, run after the window is removed and its close observers have run, not a callback that
force-quits — plus a macOS-only `cmd-w` binding, needed because dodo installs no menu bar for that
shortcut to hang off. The tray overrides the mode to `QuitMode::Explicit` at runtime once its icon
exists, and deliberately not statically; `src/main.rs`'s comment above `with_quit_mode` explains
why that ordering is the difference between a working app and an unquittable one.

The Windows `--build-info` path has run on a release runner. Interactive window lifecycle on
Windows still needs captain testing.
