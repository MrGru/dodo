# Epic E0 — Core shell & app lifecycle

**Source:** `src/main.rs`, `src/app.rs`, `src/layout.rs`
**Stage:** Phase 3 (Solutioning)
**Status:** Done

---

Everything else in dodo renders inside this shell. No dependencies; every other epic depends on it.

| Story | Title | Status | Depends on |
|---|---|---|---|
| **E0.1** | Startup sequence — `gpui_component::init`, module `init` calls, `DodoApp` mounted in a 900x620 centered window | Done | — |
| **E0.2** | `QuitMode::LastWindowClosed` (GPUI's own post-removal check) + macOS-only `cmd-w` binding, since dodo installs no menu bar | Done | E0.1 |
| **E0.3** | `dodo --version` / `--build-info` print embedded build metadata and exit before any window opens; `attach_parent_console` restores standard handles on a GUI-subsystem Windows binary | Done | E0.1 |

**AC (E0.2):** Closing the single window quits the app; the check runs after the window is
removed, never as a callback that force-quits.

**AC (E0.3):** `--version`/`--build-info` produce visible output even from a Windows release build
with no console attached, and never initialize GPUI.
