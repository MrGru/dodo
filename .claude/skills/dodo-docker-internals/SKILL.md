---
name: dodo-docker-internals
description: Deep internals of crates/dodo-docker/ that no single file makes obvious - per-platform engine discovery and its two socket-path traps, why services/ is the only place naming bollard and the only tokio runtime dodo builds, docker::init's key-binding ordering, why POLL_INTERVAL is a constant, why the sidebar is flat with Docker's own vertical rail, and why a modal overlay must be window.open_dialog rather than a hand-rolled scrim. Load before touching anything under crates/dodo-docker/, including a "Coming soon" placeholder (Exec/Terminal, Create/Pull/Build, Stats, Favorites).
---

**`crates/dodo-docker/`** is the Docker/Podman module, and it is **feature-complete as of round 6**: four
list pages (Containers with compose grouping, filters, bulk actions; Images/Volumes/Networks),
background polling with incremental merges, keyboard navigation, row context menus, and a
read-only detail dialog — Inspect for all four resource types plus a container log viewer behind
a second tab. **`crates/dodo-docker/src/lib.rs` is the authority** — it documents the layer split, what each
round shipped, and, for the features that are still deliberately disabled "Coming soon"
placeholders (Exec/Terminal, Create/Pull/Build, Stats beyond live CPU%, Favorites), exactly where
each one plugs in. Read it before changing anything here rather than inferring the structure from
the files.

Six things about the module that are not obvious from any one file:

- **Engine discovery is hand-rolled per platform because bollard's is not enough.**
  `services/engine.rs::connect()` carries the numbered order (DOCKER_HOST → `/var/run/docker.sock`
  → macOS `podman machine` → bollard's Podman defaults) and the reasons inline; read it before
  touching connection behaviour. The two traps it encodes: `/var/run/docker.sock` is often a
  *dangling* symlink on a Mac that once ran Docker Desktop, and `connect_with_podman_defaults()`
  only probes Linux paths, so it never finds the per-user
  `$TMPDIR/podman/podman-machine-default-api.sock` a macOS `podman machine` actually listens on.
- **`services/` is the only place that may name `bollard`**, and the only place dodo
  **constructs a tokio runtime**. (The precision matters since the Database Explorer landed: the
  `postgres` crate builds a private current-thread runtime *per client*, and
  `crates/dodo-database/src/services/postgres.rs` never names `tokio` — so this is the only runtime dodo
  builds, not the only one that exists.) `bollard` is async, so `BollardEngine` drives every call
  with `Runtime::block_on` on the background executor, keeping the blocking-by-contract discipline
  `Transport` follows. Inspect
  responses cross that boundary as `serde_json::Value`, so the field extraction in
  `models/inspect.rs` stays testable without a daemon.
- **`docker::init` registers the module's key bindings** and must run from `main` after
  `gpui_component::init` — the same tie-break rule as `api_explorer::init`. Bindings are scoped to
  the `DockerList` key context; the actions themselves are declared in `crates/dodo-docker/src/lib.rs`.
- **`docker::POLL_INTERVAL` is a constant, not a setting, on purpose** (5s). Exactly one visible
  page polls (`DockerView::should_poll`), and leaving the section calls `set_section_active(false)`
  (wired in `layout.rs`), so an idle cadence never runs.
- **The sidebar is flat — every tool, Docker included, is one top-level `SidebarMenuItem` with
  no children.** Docker used to be a nested group, and that made its four pages unreachable:
  an icon-collapsed sidebar renders no children at all. The four pages now live on Docker's own
  vertical tab rail (`DockerView::render_rail`), so `View` has a single `View::Docker` variant and
  `DockerPage` owns the page identity — its `title`/`icon`/`ALL`/`DEFAULT`. Do not reintroduce
  nesting here. `layout.rs::pane_title` is why the main-pane heading still names the page
  ("Containers") while the sidebar row names the tool ("Docker").
- **A modal overlay is a `window.open_dialog`, never a scrim in the page's own tree.** The Docker
  detail surface was the second attempt at hand-rolling one and it could not block clicks: a
  `div().absolute().inset_0()` scrim only covers the *page* (not the rail or the sidebar), and a
  no-op `on_mouse_down` closure registers a listener that swallows nothing — gpui keeps
  dispatching to every hitbox under the cursor unless a listener calls `cx.stop_propagation()` or
  the element sets `HitboxBehavior::BlockMouse` (`.occlude()`). `settings::open` had it right all
  along; `views/detail.rs`'s module doc records the diagnosis, and `docker::init` deliberately
  binds no `escape` because the library `Dialog` owns dismissal. Two costs of following that
  pattern are written down there too: the dialog body must be an **entity** (a dialog layer does
  not repaint on the page's `cx.notify()`), and its width must be **stated** rather than `w_full`
  (a percentage width resolves to `auto` inside the dialog's wrappers and content-sizes the body).
