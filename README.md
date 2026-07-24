# dodo

A Rust desktop GUI application built on [Zed Industries'](https://github.com/zed-industries/zed)
[GPUI](https://www.gpui.rs/) framework and the [gpui-component](https://github.com/longbridge/gpui-component)
widget library.

## Status

**Early-stage.** dodo opens a single centered window with a collapsible sidebar;
selecting a sidebar item switches the main pane to that tool.

Tools available today:

- **Json formatter** - pretty-prints pasted JSON at a chosen indent width, with
  the parse error shown inline as a diagnostic when the input is invalid.
- **Encoder / Decoder** - Base64 (standard and URL-safe), URL percent-encoding
  and Hex in both directions, plus a JWT inspector that splits a token into its
  header, payload and signature (decode only - no signature verification).
- **API Explorer** - an HTTP client: several request tabs, each with its own
  method, URL, query parameters and headers, sent asynchronously (Cmd/Ctrl+Enter
  or the Send button) and answered with a status badge, timing, size, the
  response headers and a syntax-highlighted body. Request Body, Auth and Scripts,
  the response Cookies, Tests and Console tabs, and saved collections are marked
  in the UI as arriving in a later step.
- **Docker** - a Docker/Podman manager talking to the Docker Engine API (honours
  `DOCKER_HOST`, else the local Docker or Podman socket). The **Containers** page
  lists containers with colored status badges, live CPU %, published ports,
  relative last-started times, instant search, and per-row Start / Stop / Restart
  / Delete actions (Delete confirms first); an unreachable engine shows an error
  state with Retry. **Images**, **Volumes** and **Networks** are placeholder pages
  arriving in a later round, alongside compose grouping, filters and bulk actions.

## Tech stack

- **[gpui](https://www.gpui.rs/)** and **gpui_platform** - the GPUI UI framework,
  pulled directly from the Zed git repository.
- **[gpui-component](https://github.com/longbridge/gpui-component)** - third-party
  GPUI widget library (sidebar, buttons, icons, theming), pulled directly from git.
- **[rust-embed](https://crates.io/crates/rust-embed)** - embeds SVG icons into the
  binary at build time.
- **[anyhow](https://crates.io/crates/anyhow)** - error handling.
- **[reqwest](https://crates.io/crates/reqwest)** - the API Explorer's HTTP client,
  built with rustls rather than the platform TLS stack, so no OpenSSL is needed.
- **[bollard](https://crates.io/crates/bollard)** - the Docker module's Docker Engine
  API client (local unix socket, no TLS, so no OpenSSL), driven from a small
  **[tokio](https://crates.io/crates/tokio)** runtime on the background executor.

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
├── src/
│   ├── main.rs         # Entry point: initializes GPUI, opens the window, mounts DodoApp
│   ├── app.rs          # DodoApp: top-level view holding the Layout
│   ├── layout.rs       # Sidebar + main pane; sidebar collapse-mode demo
│   ├── app_icon.rs     # AppIcon enum mapping icon names to embedded SVG paths
│   ├── assets.rs       # rust-embed AssetSource that loads embedded icons
│   └── api_explorer/   # The HTTP client tool: models, services, state, views
└── assets/
    └── icons/          # SVG icons embedded into the binary
```
