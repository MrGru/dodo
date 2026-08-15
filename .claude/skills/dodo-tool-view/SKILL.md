---
name: dodo-tool-view
description: End-to-end checklist for adding a new tool to dodo's sidebar (a new src/<tool>.rs module, the View enum entry, its icon SVG and AppIcon variant, and wiring it into Layout), plus the rule deciding whether a tool's root fills the pane or is scrolled by it. Load when asked to add, rename, reorder or remove a tool view, when a new sidebar entry does not appear or renders blank, or when part of a tool page is unreachable at a small window size (clipped, cut off, will not scroll).
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
4. **`crates/dodo-app-icon/src/lib.rs`** — add an `AppIcon` variant and its arm in `IconNamed::path`
   (`Self::Foo => "icons/foo.svg"`). The path is what reaches the asset source; the variant name
   is arbitrary. Watch the existing `Palette => "icons/palatte.svg"` — filename typo, variant
   spelled correctly. **The glyph has to be the tool's own**, not one already on another row:
   collapsed to the rail the icon is the entire row, and
   `layout::tests::no_two_tools_wear_the_same_icon` fails if you borrow one. Borrowing is the easy
   mistake — the input method drew `Globe`, which is the API Explorer's, until it stopped being a
   settings page and moved into the sidebar beside it. `scripts/generate-icons.py` is **not**
   involved: it derives the *application* icon from `assets/branding/`, and `assets/icons/*.svg`
   are hand-written and committed.
5. **`src/layout.rs`** — five edits, four of which the compiler will demand:
   - a `View` variant;
   - bump the arity and contents of `const ALL: [View; N]` (this one is silent if you forget —
     the menu simply will not list your tool). `ALL` is the **default** order, not the order the
     sidebar draws: since the Features settings page landed, that is the user's, held by
     `Layout::features`. Where you insert your tool decides where it appears for someone who has
     never reordered anything, and — through `Features::resolve` — which existing tool it turns up
     next to for someone who has;
   - an arm in `View::title` and in `View::icon`;
   - **an arm in `View::code`**, which is what `session.json` stores so the tool can be reopened
     on next launch. This one is a **compatibility surface**, not just another match arm: pick a
     kebab-case identifier, never a title, and never reuse a code that has shipped for a different
     tool. `View::shown` (over `Features::active`) resolves anything it does not recognise to the
     first tool the sidebar *does* list, so a removed or renamed tool degrades to "opens on
     something" rather than failing to start. Changing a shipped code now costs more than it used
     to: it is also the tool's identity in the user's stored order and on/off list, so everyone who
     had reordered or hidden it gets it back at its default position, switched on.
   - a `Entity<YourTool>` field on `Layout`, initialised in `Layout::new`, and an arm in the
     main-pane `match self.active` inside `Layout::render`.

   **A tool that only exists on one platform is a `cfg` on every one of those**, and
   `View::InputMethod` is the worked example — the variant, the field, the initialiser and each
   `match` arm carry `#[cfg(target_os = "macos")]`, and `const ALL` is written out twice because
   stable Rust has no attribute on an array element. Do that only when the tool genuinely cannot
   work elsewhere (it installs an InputMethodKit bundle); a tool that is merely *unfinished* on a
   platform shows a "Coming later" pane instead, which is what `crates/dodo-cleaner/src/` does. Nothing else
   breaks: `Features::resolve` drops a stored tool the running build does not have and hands it
   back beside its default neighbour on a build that does, so one `session.json` moves between
   platforms without losing anything permanently.

   **You do not have to touch the Features settings page.** It is generated from `View::codes()`,
   so a new tool appears there with a switch and its two move buttons for free — and is switchable
   off like any other. Nothing needs to opt in, and nothing should try to opt out: a tool that
   cannot be hidden would be a special case in `session::models::features` and there are none.

   One more thing if the tool needs to *start something* when it becomes active — Docker's polling
   is the existing case. `Layout::activate` is the normal path, but a restored session opens
   straight onto the tool with no click, and there is no `self` to call `activate` on inside the
   constructor. `Layout::new` builds the Docker entity first and tells it by hand; copy that.

6. **`crates/dodo-i18n/`** — `View::title` returns a `Str`, not a string, so the tool needs a variant for
   its sidebar title (in the `shell` area, which owns the sidebar) and one for every label inside
   it (in the tool's own area, which is a new `crates/dodo-i18n/src/<tool>/` directory of four
   small files: `mod.rs`, `en.rs`, `vi.rs`, `samples.rs`, plus one line in the `areas!` macro in
   `crates/dodo-i18n/src/lib.rs`). Load
   `dodo-i18n-text` before writing that text; `cargo test i18n` will fail until each new variant
   has a sample.

7. **`src/quick_nav/` — only if the tool can accept a pasted value.** Optional, and skipping it
   costs nothing: the tool simply is not a quick-navigation target. If it should be one, it is
   four small edits and no more — a `Detector` variant with its arm in `models/detect.rs`, a
   `Route` variant in `models/route.rs` with its arm in `Route::detector`, an arm in
   `View::for_detector`, and an arm in `Layout::apply_route`, which is the one place a route meets
   a `View`. Three rules there are not negotiable: **where the tool's format already has a real
   parser, attempting that parser is the detector** (a regex is not an improvement on a tested
   parser); **the position you insert into `Detector::ORDER` has to be argued in that module's
   doc** — the order is most-specific-first and every existing position is forced by an overlap
   below it, and it is emphatically *not* the sidebar's order, which the user can now drag around;
   and **`View::for_detector` must name the same tool the route lands in**, because
   `Layout::allowed_detectors` reads it *before* detection to drop the detectors of switched-off
   tools. Get that wrong and hiding one tool silences another's paste, silently.

`cargo build` catches every step except the `ALL` array. If the tool builds but no sidebar row
appears, that is the one you missed.

## Things worth knowing before you start

- **Tool titles are localized.** `View::title` returns a `Str`, rendered by callers with
  `t(view.title(), cx)`. A new tool therefore needs a `Str` variant per title, plus one for every
  label, button, placeholder and error message it shows — see `dodo-theming-settings`, which also
  covers parameterized messages and the widgets whose strings do not refresh on their own.
- **The main pane is a scroll container with a floor under it, and your root's height decides
  whether that container can ever scroll.** `layout::main_pane` is the container and
  `layout::tool_box` is the box your view is handed — both are functions with the whole rule in
  their doc comments, and both are asserted by unit tests. What you have to choose is which of two
  shapes your tool is:
  - **A tool that fills the pane** (every tool but one): root `v_flex().size_full()`, with
    `.flex_1().min_h_0()` on whatever absorbs the leftover height, and its own `overflow_scroll()`
    on whatever can exceed it — see `crates/dodo-docker/src/views/containers.rs`. Omitting `min_h_0` makes a
    multi-line editor grow past the window instead of scrolling.
  - **A page that is as tall as its content** — a column of settings rows, like
    `input_method::views::input_method_view::page_root`: root `v_flex().w_full()` and **no
    height at all**. The pane then scrolls it, which is the only thing that reaches a row past the
    bottom edge at a small window.

  Do not give a content-height page `size_full`. A height of 100% is a *definite* height, and gpui
  measures a scroll container's content as the bounding box of its **direct** children — so a page
  pinned to the pane reports the pane's height however long it really is, and the surplus rows are
  clipped with no scrollbar to reveal them. That was the Input method page's bug, fixed on
  2026-08-13.

  Two consequences worth knowing either way: your view is **never** narrower than `MAIN_MIN_WIDTH`
  or shorter than `MAIN_MIN_HEIGHT` (`src/layout.rs` — 520×360 today, and
  `WindowOptions::window_min_size` stops a resize drag before even that); and on any ordinary
  window a filling tool's box is exactly the pane, so the outer container has no scroll range and
  consumes no wheel events from yours.
- **The sidebar opens collapsed to icons**, and collapses again on its own if the user expands it
  and then narrows the window past `AUTO_COLLAPSE_WIDTH`. So a new tool's row is an *icon* first
  and a label second: the icon has to read on its own, and `View::title` is what the collapsed row
  shows as its tooltip. `SidebarState` in `src/layout.rs` documents why that rule is
  edge-triggered — do not turn it into a plain `width <` test in `render`.
- **Error surfaces** are hand-rolled banners, not a library component — copy
  `EncoderDecoder::error_banner` (`danger` border, `danger.opacity(0.1)` background). Only the
  JSON formatter also pushes an inline diagnostic, and that needs a `code_editor` input; see
  `gpui-component-recipes`.
- **The sidebar can be empty of your tool entirely.** Since the Features page, a user may switch it
  off, so nothing may assume its row exists or that it is reachable — and if it starts something
  when it becomes active (Docker's polling), that thing must also stop when it stops being the
  active tool, which `Layout::activate` already handles for every tool.
- Update `README.md`'s "Tools available today" list; it is the only user-facing inventory.
