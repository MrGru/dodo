# Contributing to dodo

Everything a contributor needs that is not on the front page: the local checks,
and a tour of where the code lives.

## Local checks

CI runs formatting and clippy as **blocking** jobs, so a push that fails either
is a wasted round trip. The repo ships a `pre-push` hook that runs the same
checks locally and refuses the push if any of them fails. Git does not version
`.git/hooks`, so enable it once per clone:

```sh
git config core.hooksPath .githooks
```

That is the whole setup — there is no install script, and the setting is local
to your clone, so nobody is opted in without doing this. Note that it points git
at `.githooks` for *all* hooks, so anything you keep in `.git/hooks` stops
running.

The hook runs, in this order and stopping at the first failure:

```sh
cargo fmt --all --check
cargo clippy --all-targets --locked -- -D warnings
cargo test --locked
```

**Cost.** Cheapest first, so a formatting slip is caught in well under a second.
With a warm `./target`, a green run of all three takes roughly **15 seconds**
after a normal edit. It is much worse when the cache is cold — after
`cargo clean`, a `Cargo.lock` change, or a toolchain bump, expect **several
minutes**, because clippy and the test binary both have to build the whole
dependency graph. The hook deliberately does not set its own `CARGO_TARGET_DIR`:
sharing `./target` with everyday `cargo build` is what keeps the warm case cheap.
A push that only deletes remote refs skips the checks entirely, since it uploads
no code.

**Bypass.** `git push --no-verify` skips the hook for one push. That is a
supported way to use it — pushing a WIP branch, or a push whose failure you
already know about. CI still runs the same checks.

**`cargo update` is never a side effect.** `Cargo.lock` is the only pin on the
four git dependencies (`gpui`, `gpui_platform`, `gpui-component`,
`gpui-component-assets`); updating it silently jumps them to upstream HEAD.
Change it only in its own reviewed commit, and pass `--locked` otherwise.

## Repository tour

```
.
├── .githooks/          # Tracked git hooks; see "Local checks" above
├── Cargo.toml          # Package metadata, workspace members, dependencies
├── build.rs            # Embeds build metadata (and the Windows .ico)
├── docs/               # Architecture, build optimization, release and platform notes
├── scripts/            # Packaging, bundling and icon generation
├── tools/              # Release-only crates, excluded from the workspace
├── crates/             # Workspace crates, sharing the one Cargo.lock
│   ├── dodo-i18n/            # Every user-facing string, in each supported language
│   ├── dodo-app-icon/        # AppIcon enum mapping icon names to embedded SVG paths
│   ├── dodo-paths/           # Where persisted files live, per platform
│   ├── dodo-dialog-slot/     # The one-dialog-at-a-time slot
│   ├── dodo-{api-explorer,cleaner,database,docker,input-method,updater}/  # Feature crates
│   ├── dodo-{json-formatter,encoder-decoder}/   # Single-file feature crates
│   └── dodo-ime-core/                           # Pure input-method engine
├── src/
│   ├── main.rs         # Entry point: GPUI init, --version/--build-info, the window
│   ├── app.rs          # DodoApp: top-level view holding the Layout
│   ├── layout.rs       # Sidebar + main pane
│   ├── tools.rs        # The tool table — one row per sidebar tool
│   ├── settings/       # The Settings dialog
│   ├── quick_nav/      # Clipboard detection and the normal-mode key bindings
│   ├── session/        # session.json: appearance, window, open tool, tool list
│   ├── tray/           # Menu bar / notification-area item, and Start with OS
│   ├── assets.rs       # rust-embed AssetSource that loads embedded icons and themes
│   └── window_icon.rs  # Runtime window/Dock icon and the Linux app_id
└── assets/
    ├── branding/       # Source artwork and the 1024px master
    ├── macos|windows|linux/  # Platform icons generated from that master
    ├── icons/          # SVG icons embedded into the binary
    └── themes/         # Theme JSON embedded into the binary
```

[`architecture/workspace-layout.md`](architecture/workspace-layout.md) is the authority on *why* the crates are
split the way they are, and on which shape a new crate should take.
[`AGENTS.md`](../AGENTS.md) at the repo root routes to the rest of `docs/` by task.
