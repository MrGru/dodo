<img src="assets/branding/dodo-256.png" alt="" width="112" align="right">

# dodo

A native desktop app that puts the tools a developer keeps reaching for in one
window: format some JSON, decode a token, fire an HTTP request, look inside a
container, query a database, clean up a disk. One collapsible sidebar, one tool
per row, no browser tab and no Electron.

Written in Rust on Zed's GPUI framework — a native window, no web runtime.
Paste a JSON blob, a JWT, a `curl` command or a database URI anywhere in the app
and dodo opens the tool that handles it with the value already loaded.

**Pre-1.0 and under active development.** It is useful daily; it is not
finished, and this page says which parts are which.

## Tools

**JSON formatter** — pretty-print at 2, 3 or 4 spaces. Invalid input surfaces the
parser error inline, as a diagnostic at the offending location.

**Encoder / Decoder** — Base64 (standard and URL-safe), URL percent-encoding and
Hex, both directions, plus a JWT inspector that splits a token into header,
payload and signature. Decode only — signatures are not verified.

**API Explorer** — a tabbed HTTP client. Per-tab method, URL, query parameters,
headers, a body in seven shapes (JSON, text, XML, HTML, multipart form-data with
file uploads, URL-encoded, binary) and Bearer / Basic / API-key auth. Responses
come back with status, timing, size, headers, parsed cookies and a highlighted
body that also renders JSON as a tree. Pre-request and post-response scripts run
in a QuickJS sandbox behind a consent gate and report into Console and Tests
tabs. Collections and environments persist; the request on screen generates
cURL, `fetch`, `axios` or `XMLHttpRequest`.

**Docker** — Docker or Podman over the Engine API (`DOCKER_HOST`, else the local
socket). Containers, Images, Volumes and Networks are full list pages with
status badges, live CPU %, search, multi-filter popovers, compose grouping, bulk
and per-row lifecycle actions, keyboard navigation and a five-second background
refresh; clicking a row opens the engine's own Inspect JSON, plus a bounded log
tail for containers. A fifth page detects the container runtimes installed on
the host itself — Docker, Podman Machine, Kubernetes, containerd — and starts or
stops them; Kubernetes is deliberately read-only there.

**Database Explorer** — PostgreSQL, SQLite, MySQL / MariaDB and Redis, from a
connection form that can be filled from a pasted URI. Browse a lazily loaded
object tree, run queries in tabbed editors with protocol-level cancellation and
a bounded result grid, and export as streamed CSV or JSON. A table or view opens
Data, Columns, Indexes, Constraints and DDL, with server-paged data that can be
edited: row changes accumulate as pending edits, are shown for confirmation, and
commit or roll back in one transaction — only where the result carries real
primary/unique-key identity. Redis is read-only.

**Cleaner** — safety-first disk cleanup: scan a category, review what was found,
move it to the Trash. Scanning never deletes, cleanup is allow-list bounded, and
nothing leaves the review step without being selected. macOS lists fourteen
categories, Windows and Linux eight each — exactly what each platform's scanners
implement. See [`docs/cleaner/`](docs/cleaner/) for the safety model, privacy
posture and the known limitations of each scanner.

**Input method** (macOS and Windows) — a Vietnamese Telex/VNI engine driven by
Event Tap on macOS and Keyboard Hook on Windows. It works while Dodo is running;
macOS requires Accessibility permission. The pane selects input languages, the
switch shortcut and Vietnamese engine settings. Linux has no sidebar row until
it has an implementation.

### Around the tools

- **Quick navigation.** With no input focused, `Cmd/Ctrl+V` — or just `p` —
  reads the clipboard, works out what it is and opens the matching tool with the
  value loaded. `Esc` leaves a text field and gets you back to that mode. When
  nothing is recognised confidently, nothing happens. Switchable off, and each
  format's pattern is editable in Settings.
- **Settings.** Language (English, Vietnamese), 16 themes, font size, border
  radius, script-execution policy, quick navigation, Start with OS (macOS and
  Windows), and Features — which tools the sidebar lists and in what order.
- **Menu bar / notification area** (macOS and Windows). Closing the window
  leaves dodo and its input method running behind the tray icon; **Quit Dodo** is
  the full shutdown path.
- **Check for updates.** An in-app updater that downloads a release, verifies its
  SHA-256, installs it and restarts. It can check silently at startup, but never
  downloads anything without a button press.

Window geometry, the open tool, the sidebar state and every setting survive a
restart, except the script-execution policy, which deliberately asks again each
launch.

## Install

Grab an archive from the [latest release](https://github.com/MrGru/dodo/releases/latest).
Builds are **not code-signed or notarised**.

**macOS** (`dodo-v<version>-macos-arm64-app.tar.gz`, or `-macos-x64-` on Intel):

```sh
tar -xzf dodo-v<version>-macos-arm64-app.tar.gz
xattr -dr com.apple.quarantine dodo.app
mv dodo.app /Applications
```

Take the `-app` archive rather than the plain-binary one: the Vietnamese input
method ships inside the bundle, and Start with OS needs an installed app bundle
on macOS 13 or later. The plain `dodo-v<version>-macos-<arch>.tar.gz` is just the
binary, for terminal use.

**Linux** (`dodo-v<version>-linux-x64.tar.gz`) — the archive holds the binary
plus a `share/` tree laid out for installation:

```sh
tar -xzf dodo-v<version>-linux-x64.tar.gz
cd dodo-v<version>-linux-x64
cp -r share ~/.local/          # desktop entry and icons
./dodo
```

Installing `share/` is not decoration: a Wayland compositor matches the window
against `dodo.desktop` to find the icon.

**Windows** (`dodo-v<version>-windows-x64.zip`) — unzip and run `dodo.exe`.

**Verify a download.** Every release publishes `SHA256SUMS` alongside a
`.sha256` sidecar per archive:

```sh
sha256sum -c SHA256SUMS      # or: shasum -a 256 -c SHA256SUMS
```

`dodo --build-info` prints the commit, tag, build time, target and rustc version
the binary was produced from.

**Platform honesty.** All four targets are built, packaged and archive-verified
by CI on every release ([`scripts/verify-release.sh`](scripts/verify-release.sh)
checks that each archive unpacks, contains the expected files and runs). Only
**macOS arm64** has been run on a real desktop. The macOS x64, Linux and Windows
archives are produced and checked, but their install paths are unproven — expect
rough edges and please report them. [`docs/release.md`](docs/release.md) records
exactly what has and has not been verified.

## Build from source

Requires a Rust toolchain supporting **edition 2024** (1.85 or newer) via
[rustup](https://rustup.rs/), and network access on the first build — `gpui`,
`gpui_platform` and `gpui-component` are fetched from git. Platform requirements
for building GPUI apps apply.

```sh
cargo run
```

Six feature crates can also be launched on their own, in a window containing
nothing but that view:

```sh
cargo run -p dodo-cleaner        --example cleaner        --locked
cargo run -p dodo-docker         --example docker         --locked
cargo run -p dodo-database       --example database       --locked
cargo run -p dodo-api-explorer   --example api_explorer   --locked
cargo run -p dodo-json-formatter --example json_formatter --locked
cargo run -p dodo-encoder-decoder --example encoder_decoder --locked
```

These are the same views the app mounts, reading the same data directory and
real machine state. There is also a terminal harness for the Vietnamese engine:

```sh
cargo run -p dodo-ime-core --example telex
```

Contributors: [`docs/contributing.md`](docs/contributing.md) has the pre-push
hook and a tour of the repository.

## Status

Pre-1.0. Every tool listed above works today; the gaps are named where a user
would look for them rather than hidden — OAuth 2.0 in the API Explorer, Exec /
Create / Pull / Build in Docker, column sorting and autocomplete in the Database
Explorer, and an IBus host for Linux input. Persisted formats
(`session.json`, saved collections, connections) are still free to change before
1.0, though the in-app updater carries them forward.

## Tech stack

[gpui](https://www.gpui.rs/) and gpui_platform (Zed's UI framework) with the
[gpui-component](https://github.com/longbridge/gpui-component) widget library,
both from git and pinned only by `Cargo.lock`. Around them:
[reqwest](https://crates.io/crates/reqwest) with rustls for HTTP and updates,
[bollard](https://crates.io/crates/bollard) on a small
[tokio](https://crates.io/crates/tokio) runtime for the Docker Engine API,
[rquickjs](https://crates.io/crates/rquickjs) for the script sandbox,
[postgres](https://crates.io/crates/postgres),
[rusqlite](https://crates.io/crates/rusqlite),
[mysql](https://crates.io/crates/mysql) and
[redis](https://crates.io/crates/redis) for the database drivers, and
[rust-embed](https://crates.io/crates/rust-embed) for the icons and themes baked
into the binary. TLS goes through rustls rather than the platform stack, so no
OpenSSL is needed. [`Cargo.toml`](Cargo.toml) is the authority, and says why each
dependency is there.

## Licence

dodo's own source is [MIT](LICENSE).

The binary is statically linked and contains third-party code under other
licences, including **GPL-3.0-or-later** crates reached through `gpui`
(`ztracing`, `zlog`, `ztracing_macro`). What that means for distributing a built
binary is an **open question that has not been decided**.
[`THIRD-PARTY-NOTICES.md`](THIRD-PARTY-NOTICES.md) records both the dependency
licences and that open question; read it before redistributing a build.
