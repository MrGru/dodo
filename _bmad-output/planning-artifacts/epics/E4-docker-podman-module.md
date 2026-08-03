# Epic E4 — Docker / Podman module

**Source:** `src/docker/` (`models/`, `services/`, `state/`, `components/`, `views/`)
**Stage:** Phase 3 (Solutioning)
**Status:** Done (feature-complete as of round 6), four placeholders remain by design

---

Depends on E0 (shell) and E7 (i18n). `docker::init` registers this module's key bindings, scoped to
the `DockerList` key context.

| Story | Title | Status | Depends on |
|---|---|---|---|
| **E4.1** | Engine discovery hand-rolled per platform (DOCKER_HOST → `/var/run/docker.sock` → macOS `podman machine` → bollard's Podman defaults), because bollard's own defaults miss a dangling socket symlink and a macOS podman machine's real path | Done | E0.1 |
| **E4.2** | Containers page: list, colored status, live CPU%, ports, relative start time, search, compose grouping, filters, bulk actions, Start/Stop/Restart/Delete | Done | E4.1 |
| **E4.3** | Images/Volumes/Networks real list pages (round 3 replaced their placeholders) | Done | E4.1 |
| **E4.4** | Inspect dialog for all four resource types + container log viewer, as a `window.open_dialog` entity | Done | E4.2 |
| **E4.5** | Flat sidebar (Docker is one top-level item; its four pages live on its own vertical rail) + 5 s polling only while the section is active | Done | E4.2 |
| **E4.6** | Exec/Terminal, Create/Pull/Build, Stats beyond live CPU%, Favorites | **Deliberately not built** (visible disabled controls) | E4.2 |

**AC (E4.4):** The detail dialog uses `window.open_dialog`, not a hand-rolled page-level scrim —
a `div().absolute().inset_0()` only covers the page (not the rail or sidebar), and a no-op
`on_mouse_down` closure swallows nothing unless it calls `cx.stop_propagation()` or the element
sets `.occlude()`.

**AC (E4.6, deliberate non-goal):** Each of the four remains a visibly disabled control with a
tooltip — not an absent feature and not a silent stub. Building any of them is a new story with
its own ADR, not a bug fix.
