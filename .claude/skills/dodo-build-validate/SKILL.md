---
name: dodo-build-validate
description: How to build, test and actually verify a change in dodo - the edition-2024/toolchain requirement, what the four git-sourced dependencies mean for first builds and for cargo update, the gpui glob-import that breaks #[test] and how to write unit tests anyway, and what can and cannot be checked without a human at the window. Load before running cargo for the first time in a session, before adding tests, when a build or `cargo test` fails oddly, or when asked whether a UI change works.
---

## Toolchain

`edition = "2024"` in `Cargo.toml` and `.rustfmt.toml`, so Rust **1.85 or newer**. There is no
`rust-toolchain.toml`; whatever `rustc` is on PATH is used. macOS is the only platform exercised
so far.

## The four git dependencies

`gpui` and `gpui_platform` come from `zed-industries/zed`; `gpui-component` and
`gpui-component-assets` from `longbridge/gpui-component`. None of the four names a `rev`, `tag`
or `branch` in `Cargo.toml`, so **`Cargo.lock` is the only thing pinning them**:

| crate | pinned rev |
|---|---|
| `gpui`, `gpui_platform` | zed `a1230fc` |
| `gpui-component` | `3c270ed` |
| `gpui-component-assets` | `b004e59` (a *different* rev of the same repo — hence two cargo checkouts) |

Consequences:

- **Never run `cargo update` casually.** It moves all four to whatever the default branch's HEAD
  is at that moment. Both are fast-moving pre-1.0 projects; the result is a large, unreviewed API
  break with no changelog. Update deliberately, alone, in its own commit.
- A first build fetches ~500 MB of Zed source and compiles ~800 crates. Budget tens of minutes
  and several GB in `target/` (currently ~3.8 GB); do not assume a build is hung.
- The pinned gpui-component checkout is the reference for every widget question:
  `~/.cargo/git/checkouts/gpui-component-*/3c270ed/crates/ui/src`. It also carries the upstream
  authors' own skills under `<checkout>/skills/`.
- `block v0.1.6` emits a future-incompatibility warning on every build. It comes from a
  transitive dep; ignore it.

## Unit tests: `#[test]` resolves to gpui's macro, not std's

`cargo test` with any plain `#[test]` in a module that does `use gpui::*;` fails with
`recursion limit reached while expanding #[test]`, and raising `recursion_limit` turns that into
a rustc **SIGBUS** stack overflow inside `libgpui_macros`. This is not a broken toolchain.

Root cause: `gpui` re-exports `gpui_macros::test` (`pub use gpui_macros::{.., test};`). Every
dodo tool module starts with `use gpui::*;`, and `mod tests { use super::*; }` pulls that glob in,
so `#[test]` resolves to gpui's proc macro, which re-emits `#[test]` and recurses forever.

**Unit tests are perfectly viable** — just keep the glob out of the test module by importing what
you need by name:

```rust
#[cfg(test)]
mod tests {
    use super::{decode_hex, encode_hex};   // NOT `use super::*`

    #[test]
    fn hex_roundtrip() {
        assert_eq!(encode_hex(b"hi"), "6869");
        assert_eq!(decode_hex("6869").unwrap(), "hi");
    }
}
```

Verified: this compiles and runs (`test encoder_decoder::tests::hex_roundtrip ... ok`).
`#[::core::prelude::v1::test]` also works if you truly need `use super::*`. Use `#[gpui::test]`
deliberately when you want a `TestAppContext`, never by accident. There is no reason to reach for
a scratch crate to test pure logic.

The tests that exist today are the localization guards in `src/i18n.rs` and `src/i18n_lint.rs`
(`cargo test i18n`); see `dodo-i18n-text` for what they enforce. The pure, testable logic that is
still uncovered sits at the bottom of the tool modules — `encode_hex`, `decode_hex`,
`decode_base64`, `decode_url`, `split_jwt`, `JsonFormatter::pretty_print`.

## Verifying a change

```sh
cargo build       # type/borrow/trait errors — the only fully automated gate
cargo run         # opens a 900x620 centered window
```

`cargo build` is the real safety net here: the widget builders are strongly typed, so a
misused API is a compile error, and `Layout`'s `match self.active` is exhaustive. It cannot
catch layout collapse, an unmounted overlay layer (see `gpui-component-recipes`), a
silently-empty icon, or a theme colour with no contrast.

For a visual check without a human, `cargo run` in the background and then
`screencapture -x <path>.png` produces a usable full-screen grab on macOS — enough to confirm the
window opened, the sidebar rendered and the right pane is populated. Driving clicks
(open the Settings dialog, switch tools, type into an editor) is **not** automatable here; when a
change is only observable behind an interaction, say so and ask for a human pass rather than
claiming it verified.

`cargo fmt --check` currently **fails on pre-existing code** (`src/encoder_decoder.rs`,
`src/json_formatter.rs`, `src/layout.rs`). Do not treat that as regression from your change, and
do not reformat those files as a drive-by — it buries the real diff.
