---
name: dodo-tool-view
description: End-to-end checklist for adding a new tool to dodo's sidebar (a new src/<tool>.rs module, the View enum entry, its icon SVG and AppIcon variant, and wiring it into Layout). Load when asked to add, rename, reorder or remove a tool view, or when a new sidebar entry does not appear or renders blank.
---

A tool is a self-contained module under `src/` exposing an entity with
`new(&mut Window, &mut Context<Self>)` plus `Render`. `src/layout.rs` owns the `View` enum that
drives both the sidebar menu and the main pane. Views are constructed **once** in `Layout::new`
and kept alive for the process, so switching tabs preserves editor contents and scroll position —
never rebuild a view on selection.

## Checklist

1. **`src/<tool>.rs`** — model it on `src/json_formatter.rs` (simplest) or
   `src/encoder_decoder.rs` (multiple editors, mode switching). Constructor signature must be
   `pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self`, even if you do not need the
   window yet: `InputState::new` requires it and retrofitting it means touching every caller up
   to `DodoApp::new`.
2. **`src/main.rs`** — add `mod <tool>;`, alphabetically among the existing `mod` lines.
3. **`assets/icons/<name>.svg`** — 24×24 Lucide outline
   (`fill="none" stroke="currentColor" stroke-width="2"`). gpui rasterizes the file and uses it as
   an **alpha mask** tinted with the element's text colour (`window.paint_svg` →
   `render_alpha_mask`), so colours inside the file are discarded entirely; only coverage
   survives. A multi-colour or solid-filled icon becomes a blob.
   No build step — `src/assets.rs` embeds `assets/icons/**/*.svg` via `rust-embed`.
4. **`src/app_icon.rs`** — add an `AppIcon` variant and its arm in `IconNamed::path`
   (`Self::Foo => "icons/foo.svg"`). The path is what reaches the asset source; the variant name
   is arbitrary. Watch the existing `Palette => "icons/palatte.svg"` — filename typo, variant
   spelled correctly.
5. **`src/layout.rs`** — four edits, three of which the compiler will demand:
   - a `View` variant;
   - bump the arity and contents of `const ALL: [View; N]` (this one is silent if you forget —
     the menu simply will not list your tool);
   - an arm in `View::title` and in `View::icon`;
   - a `Entity<YourTool>` field on `Layout`, initialised in `Layout::new`, and an arm in the
     main-pane `match self.active` inside `Layout::render`.

`cargo build` catches every step except the `ALL` array. If the tool builds but no sidebar row
appears, that is the one you missed.

## Things worth knowing before you start

- **Tool titles bypass i18n.** `View::title` returns a hard-coded `&'static str`, while the
  sidebar group label and the Settings dialog go through `t(Str::X, cx)` (see
  `dodo-theming-settings`). Adding a tool does not require a `Str` variant; matching the rest of
  the app's localisation would.
- **The main pane is a plain flex child**, `div().flex_1().min_h_0()`. Your view gets
  `size_full()` inside it, so give the root of your `Render` a `v_flex().size_full()` and put
  `.flex_1().min_h_0()` on whatever should absorb the leftover height. Omitting `min_h_0` makes a
  multi-line editor grow past the window instead of scrolling.
- **Error surfaces** are hand-rolled banners, not a library component — copy
  `EncoderDecoder::error_banner` (`danger` border, `danger.opacity(0.1)` background). Only the
  JSON formatter also pushes an inline diagnostic, and that needs a `code_editor` input; see
  `gpui-component-recipes`.
- Update `README.md`'s "Tools available today" list; it is the only user-facing inventory.
