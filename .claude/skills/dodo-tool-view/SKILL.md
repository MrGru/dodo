---
name: dodo-tool-view
description: End-to-end checklist for adding a new tool to dodo's sidebar (a new src/<tool>.rs module or crate, one row in the tools! table in src/tools.rs, and its icon SVG and AppIcon variant), plus the rule deciding whether a tool's root fills the pane or is scrolled by it. Load when asked to add, rename, reorder or remove a tool view, when a new sidebar entry does not appear or renders blank, or when part of a tool page is unreachable at a small window size (clipped, cut off, will not scroll).
---

A tool is a self-contained module under `src/` — or, once it outgrows one file, its own crate
under `crates/` — exposing an entity with `new(&mut Window, &mut Context<Self>)` plus `Render`.

**`src/tools.rs` is where a tool is declared, and it is one row.** The `tools!` table there
generates the `View` enum, `View::ALL`, `View::title` / `icon` / `code` / `codes` / `lookup` /
`for_detector`, and the `Panes` struct holding one entity per tool — everything that used to be
five hand-maintained edits in `src/layout.rs`. `layout.rs` is now the shell *around* whatever the
table declares: the sidebar, the pane, the width rule and quick navigation's routing. Views are
constructed **once**, by `Panes::new` from `Layout::new`, and kept alive for the process, so
switching tabs preserves editor contents and scroll position — never rebuild a view on selection.

Read `src/tools.rs`'s module doc before adding a row; it is the authority on what each field of a
row means and on the two rules that outrank tidiness (a `code:` is a compatibility surface, and
`hosts:` is the only way to say a tool is platform-conditional).

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
   `tools::tests::no_two_tools_wear_the_same_icon` fails if you borrow one. Borrowing is the easy
   mistake — the input method drew `Globe`, which is the API Explorer's, until it stopped being a
   settings page and moved into the sidebar beside it. `scripts/generate-icons.py` is **not**
   involved: it derives the *application* icon from `assets/branding/`, and `assets/icons/*.svg`
   are hand-written and committed.
5. **`src/tools.rs`** — **one row** in the `tools!` table, in the position you want the tool to
   have in the *default* sidebar order:

   ```rust
   YourTool {
       code: "your-tool",
       title: shell::Text::YourToolTitle,
       icon: AppIcon::YourTool,
       pane: your_tool: crate::your_tool::YourTool,
   }
   ```

   That row is the `View` variant, the entry in `View::ALL`, the arms of `View::title`,
   `View::icon` and `View::code`, the `Entity<YourTool>` field on `Panes`, its constructor and the
   main-pane arm — all of them generated, so there is nothing that can be silently forgotten. The
   only requirement on the type is the one every tool already meets: `new(window, cx)` plus
   `Render`.

   Three things about a row are decisions rather than syntax:

   - **`code:` is a compatibility surface**, not just another string. It is what `session.json`
     stores, so pick a kebab-case identifier, never a title, and never reuse a code that has
     shipped for a different tool. Since the Features settings page it is also the tool's identity
     in the user's stored sidebar order and on/off list, so changing a shipped code does not merely
     send whoever had it open back to the default — it drops the tool out of their order and puts
     it back at its default position, switched on. `View::shown` (over `Features::active`) resolves
     anything it does not recognise to the first tool the sidebar *does* list, so a removed or
     renamed tool degrades to "opens on something" rather than failing to start.
     `tools::tests::every_declared_tool_has_its_own_stable_code` pins the exact set; if you are
     editing that test to make it pass rather than to add a line, stop.
   - **Where you put the row is the default order** — not the order the sidebar draws, which is the
     user's and lives in `Layout::features`. It decides where the tool appears for someone who has
     never reordered anything and, through `Features::resolve`, which existing tool it turns up
     *next to* for someone who has.
   - **A tool that only exists on some platforms takes `hosts:`, and nothing else.**
     `hosts: any(target_os = "macos", target_os = "windows")` on the Input method's row is the
     worked example. Do **not** put a `#[cfg]` on the row: `hosts:` is answered by
     `View::available`, a `const fn` over `cfg!(..)`, so the tool's variant, title, icon and code
     stay compiled and asserted on *every* target while `View::ALL` is const-filtered down to what
     this build has. That matters because two of dodo's four release targets cannot be compiled
     from a Mac — a mistake that only exists on the branch you cannot build is a mistake the
     captain finds. Only the three lines naming the view type stay `#[cfg]`-gated, because the type
     itself does not exist there, and the generated `cfg(not(..))` arm keeps `Panes::place`
     exhaustive on every target. `tools::tests::platform_probe` compiles both halves of all of that
     from any machine, using rows whose `hosts:` is `all()` (true everywhere) and `any()` (false
     everywhere).

     Use `hosts:` only when the tool genuinely cannot work elsewhere (the Input method has
     implementations only on macOS and Windows); a tool that is merely *unfinished* on a platform shows a "Coming
     later" pane instead, which is what `crates/dodo-cleaner/src/` does. Nothing else breaks:
     `Features::resolve` drops a stored tool the running build does not have and hands it back
     beside its default neighbour on a build that does, so one `session.json` moves between
     platforms without losing anything permanently.

   **You do not have to touch the Features settings page.** It is generated from `View::codes()`,
   so a new tool appears there with a switch and its two move buttons for free — and is switchable
   off like any other. Nothing needs to opt in, and nothing should try to opt out: a tool that
   cannot be hidden would be a special case in `session::models::features` and there are none.

   **`src/layout.rs` is untouched by an ordinary tool.** The two things that would still take an
   edit there are the two that are not properties of a tool: a *pasted payload* (step 7), and
   *starting something when the tool becomes active* — Docker's polling is the only existing case.
   `Layout::activate` is the normal path for the second, but a restored session opens straight onto
   the tool with no click, and there is no `self` to call `activate` on inside the constructor, so
   `Layout::new` tells the Docker entity by hand right after `Panes::new`; copy that.

6. **`crates/dodo-i18n/`** — `View::title` returns a `Str`, not a string, so the tool needs a variant for
   its sidebar title (in the `shell` area, which owns the sidebar) and one for every label inside
   it (in the tool's own area, which is a new `crates/dodo-i18n/src/<tool>/` directory of four
   small files: `mod.rs`, `en.rs`, `vi.rs`, `samples.rs`, plus one line in the `areas!` macro in
   `crates/dodo-i18n/src/lib.rs`). Load
   `dodo-i18n-text` before writing that text; `cargo test i18n` will fail until each new variant
   has a sample.

7. **`src/quick_nav/` — only if the tool can accept a pasted value.** Optional, and skipping it
   costs nothing: the tool simply is not a quick-navigation target. If it should be one, it is
   three small edits plus one field on the row you already wrote — a `Detector` variant with its
   arm in `models/detect.rs`, a `Route` variant in `models/route.rs` with its arm in
   `Route::detector`, `pastes: [YourDetector]` on the tool's row in `src/tools.rs` (which is the
   whole of `View::for_detector` now, and is exhaustive over `Detector` by the compiler), and an
   arm in `Layout::apply_route`, which is the one place a route meets a `View` and the one thing
   the table cannot carry — every tool unpacks its payload differently. Three rules there are not
   negotiable: **where the tool's format already has a real
   parser, attempting that parser is the detector** (a regex is not an improvement on a tested
   parser); **the position you insert into `Detector::ORDER` has to be argued in that module's
   doc** — the order is most-specific-first and every existing position is forced by an overlap
   below it, and it is emphatically *not* the sidebar's order, which the user can now drag around;
   and **a `pastes:` entry must sit on the row whose tool the route actually lands in**, because
   `Layout::allowed_detectors` reads `View::for_detector` *before* detection to drop the detectors
   of switched-off tools. Get that wrong and hiding one tool silences another's paste, silently.

`cargo build` now catches every step: the row *is* the `View::ALL` entry, so the "builds but no
sidebar row appears" failure the old five-edit shape had is not reachable. `cargo test` is still
the one that catches a missing i18n sample (step 6).

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
