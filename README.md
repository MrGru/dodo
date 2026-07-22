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

See [`Cargo.toml`](Cargo.toml) for exact dependency sources. Note that `gpui`,
`gpui_platform`, and `gpui-component` are all fetched from git rather than
crates.io.

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
