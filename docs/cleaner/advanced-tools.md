# Advanced tools

Advanced categories are modeled in `CleanerCategory`:

- AI Apps
- Xcode Junk *(macOS only — see below)*
- Homebrew Cache *(macOS only — see below)*
- Node Tooling Cache
- Docker Cache
- Universal Binaries *(macOS only — see below)*
- Language Files

## What the window lists, and where

**On macOS the window lists all fourteen categories.** Windows and Linux list
only categories with working scanners: System Junk, User Cache, Trash Bins,
Large & Old Files, shared Node Tooling Cache, and shared Docker Cache.
Unsupported categories have no row.
Language Files stays macOS-only because there is no safe common deletion unit;
Windows Orphaned Files stays absent because generic AppData leftovers do not
prove ownership.

`CleanerCategory::hidden_for(HostOs)` is the entire switch, and it is a **pure
function of the platform**, not a `#[cfg]` split: returning an empty slice for a
host puts every category back in its section's tree, and both answers are unit
tested from whichever machine runs `cargo test` (the same reason `src/paths.rs`
reads the target triple rather than `cfg` — two of dodo's four targets cannot be
compiled from the machine this is usually written on). `CleanerCategory::ALL`
still names all fourteen everywhere, so the scanners, their tests and their
cleanup paths are untouched by the hiding. `is_visible`, `visible`/`visible_for`
and `categories_for`/`categories_for_host` are what read it, and
`core::category`'s unit tests pin every arm.

`HostOs::current` — read by `is_visible` — is the single place the compiled-for
platform enters the decision.

Because a scan is only ever started from a category's own pane, a hidden
category is not scanned at all: it has no row to select and therefore no `Scan`
button.

The per-platform scanner registries (`state::registry::default_scanners`) own
what can scan. Visibility must match them exactly: paired tests forbid both a
hidden scanner and a listed row without a scanner. Docker Cache and Node Tooling Cache are shared
alongside the four platform-specific filesystem scanners.

Universal Binaries has a second reason to be absent from Windows and Linux, and
it is why it was on the list before this became per-platform: it is
analysis-only, and its own per-item explanation says slice removal "is not yet
implemented", so the page reports a number and offers nothing to do about it.
That is a macOS caveat the captain accepted rather than a reason to hide it
there.


Future phases will keep high-risk operations out of automatic Smart Care cleanup and require explicit confirmation workflows.
