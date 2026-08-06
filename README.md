# dodo

A Rust desktop GUI application built on [Zed Industries'](https://github.com/zed-industries/zed)
[GPUI](https://www.gpui.rs/) framework and the [gpui-component](https://github.com/longbridge/gpui-component)
widget library.

## Status

**Pre-1.0, under active development.** dodo opens a single centered window with a
collapsible sidebar; selecting a sidebar item switches the main pane to that tool.

What ships today:

- **Json formatter** - pretty-prints pasted JSON at a chosen indent width, with
  the parse error shown inline as a diagnostic when the input is invalid.
- **Encoder / Decoder** - Base64 (standard and URL-safe), URL percent-encoding
  and Hex in both directions, plus a JWT inspector that splits a token into its
  header, payload and signature (decode only - no signature verification).
- **API Explorer** - an HTTP client: several request tabs, each with its own
  method, URL, query parameters, headers, a body in seven shapes (JSON, text,
  XML, HTML, form-data with file uploads, URL-encoded, binary) and Bearer /
  Basic / API-key authorization, sent asynchronously (Cmd/Ctrl+Enter or the Send
  button) and answered with a status badge, timing, size, response headers,
  parsed cookies and a syntax-highlighted body that also renders JSON as a tree.
  Pre-request and post-response scripts run in a QuickJS sandbox behind a
  consent gate, reporting into the Console and Tests tabs. Saved collections and
  environments persist to disk; history is per session. The request on screen can
  be generated as cURL, `fetch`, `axios` or `XMLHttpRequest`. OAuth 2.0 is
  labelled in the UI as a later step; scripts deliberately have no network
  access, and collections cannot be reordered by dragging.
- **Docker** - a Docker/Podman manager talking to the Docker Engine API (honours
  `DOCKER_HOST`, else the local Docker or Podman socket). **Containers**,
  **Images**, **Volumes** and **Networks** are all real list pages with colored
  status badges, live CPU %, published ports, relative times, instant search,
  multi-filter popovers, compose grouping, selection with bulk actions, per-row
  lifecycle actions (Delete confirms first), keyboard row navigation, right-click
  menus and background refresh every five seconds; an unreachable engine shows an
  error state with Retry. Clicking a row's name opens a read-only dialog with the
  engine's own Inspect JSON, plus a bounded log tail for containers. Exec /
  Terminal, Create / Pull / Build, stats beyond live CPU % and favourites are
  disabled controls labelled "Coming soon".
- **Database Explorer** - connect to PostgreSQL, SQLite, MySQL / MariaDB or
  Redis (a connection form that can also be filled from a pasted URI), browse a
  lazily loaded object tree, and run queries in tabbed SQL editors with
  protocol-level cancellation, `EXPLAIN` on PostgreSQL, a bounded result grid,
  cell/row copy and streamed CSV / JSON export. A table or view opens a detail
  surface with Data, Columns, Indexes, Constraints and DDL, its data server-paged
  and editable — add, edit, duplicate and delete rows accumulate as pending
  changes shown for confirmation, then commit or roll back in one transaction,
  and only where the result carries real primary/unique-key identity. Saved
  queries, bounded query history and a catalog search persist per connection.
  Redis is read-only; result columns cannot be sorted, and there is no
  autocomplete.
- **Quick navigation** - vim-shaped, and works across all of the above. Whenever
  no input is focused, `Cmd+V` / `Ctrl+V` — or just `p` — reads the clipboard,
  works out what the text is, and opens the tool that handles it with the value
  already loaded: JSON goes to the formatter and is formatted, Base64 and JWTs go
  to the Encoder / Decoder decoded, a `curl` command opens a new API Explorer tab
  built from it, and a database URI opens the matching saved connection or
  creates one. `Esc` inside a text field leaves it and gets you back to that mode.
  When nothing is recognised confidently, nothing happens. It can be switched off
  in Settings, where each format's matching pattern can also be edited; those
  settings are the one thing in that dialog that survives a restart.

The sidebar footer holds **Settings** (language, theme, font size, border radius,
the script-execution policy and quick navigation) and **Check for updates**, an
in-app updater that downloads a release, verifies its SHA-256, installs it and
restarts. It can check on its own at startup, but never downloads anything
without you pressing a button.

## Tech stack

- **[gpui](https://www.gpui.rs/)** and **gpui_platform** - the GPUI UI framework,
  pulled directly from the Zed git repository.
- **[gpui-component](https://github.com/longbridge/gpui-component)** - third-party
  GPUI widget library (sidebar, buttons, icons, theming), pulled directly from git.
- **[rust-embed](https://crates.io/crates/rust-embed)** - embeds SVG icons into the
  binary at build time.
- **[anyhow](https://crates.io/crates/anyhow)** - error handling.
- **[regex](https://crates.io/crates/regex)** - quick navigation's editable
  detector patterns. Already in the graph through `gpui`, so it is a direct
  dependency rather than a new one; it has no backtracking, which is what makes
  running a user-supplied pattern from a key handler safe.
- **[reqwest](https://crates.io/crates/reqwest)** - the HTTP client behind the API
  Explorer and the in-app updater, built with rustls rather than the platform TLS
  stack, so no OpenSSL is needed.
- **[bollard](https://crates.io/crates/bollard)** - the Docker module's Docker Engine
  API client (local unix socket, no TLS, so no OpenSSL), driven from a small
  **[tokio](https://crates.io/crates/tokio)** runtime on the background executor.
- **[rquickjs](https://crates.io/crates/rquickjs)** - the QuickJS sandbox the API
  Explorer's pre-request and post-response scripts run in.
- **[postgres](https://crates.io/crates/postgres)**,
  **[rusqlite](https://crates.io/crates/rusqlite)**,
  **[mysql](https://crates.io/crates/mysql)** and
  **[redis](https://crates.io/crates/redis)** - the Database Explorer's four
  drivers, all blocking clients run on the background executor. PostgreSQL's TLS
  goes through rustls for the same reason as above.

See [`Cargo.toml`](Cargo.toml) for exact dependency sources. Note that `gpui`,
`gpui_platform`, and `gpui-component` are all fetched from git rather than
crates.io.

## Licence

dodo's own source is [MIT](LICENSE).

The binary is statically linked and contains third-party code under other
licences, including **GPL-3.0-or-later** crates reached through `gpui`
(`ztracing`, `zlog`, `ztracing_macro`). What that means for distributing a built
binary is an **open question that has not been decided**.
[`THIRD-PARTY-NOTICES.md`](THIRD-PARTY-NOTICES.md) records both the dependency
licences and that open question; read it before redistributing a build.

## Prerequisites

- A recent Rust toolchain that supports **edition 2024** (Rust 1.85 or newer).
  Install via [rustup](https://rustup.rs/).
- Network access on first build, since several dependencies are fetched from git.

Platform-specific system requirements for building GPUI apply; see the
[GPUI / Zed documentation](https://github.com/zed-industries/zed) for details.

## Build and run

```sh
# Run the app
cargo run

# Or build without running
cargo build
```

This opens a 900x620 centered window mounting the `DodoApp`.

## Project structure

```
.
├── Cargo.toml          # Package metadata and dependencies
├── build.rs            # Embeds build metadata (and the Windows .ico)
├── docs/               # Build optimization and release engineering
├── scripts/            # Packaging and icon generation
├── tools/              # Release-only crates, excluded from the package
├── src/
│   ├── main.rs         # Entry point: GPUI init, --version/--build-info, the window
│   ├── app.rs          # DodoApp: top-level view holding the Layout
│   ├── layout.rs       # Sidebar + main pane; the View enum that lists the tools
│   ├── settings.rs     # The Settings dialog
│   ├── i18n.rs         # Every user-facing string, in each supported language
│   ├── paths.rs        # Where persisted files live, per platform
│   ├── json_formatter.rs
│   ├── encoder_decoder.rs
│   ├── api_explorer/   # HTTP client   ┐
│   ├── docker/         # Docker/Podman ├ each: models, services, state,
│   ├── database/       # Databases     ┘ components, views
│   ├── updater/        # The in-app updater, same five layers
│   ├── quick_nav/      # Clipboard detection and the normal-mode key bindings
│   ├── app_icon.rs     # AppIcon enum mapping icon names to embedded SVG paths
│   ├── assets.rs       # rust-embed AssetSource that loads embedded icons
│   └── window_icon.rs  # Runtime window/Dock icon and the Linux app_id
└── assets/
    ├── branding/       # Source artwork and the 1024px master
    ├── macos|windows|linux/  # Platform icons generated from that master
    ├── icons/          # SVG icons embedded into the binary
    └── themes/         # Theme JSON embedded into the binary
```
