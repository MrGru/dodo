# Project agent memory

This file is the project's committed home for project-intrinsic agent knowledge: build, test, release, architecture, and sharp-edge notes that should travel with the code.

- Add durable project-specific notes here as they are discovered through real work.

## Adding a tool view

Each tool is its own module (`src/json_formatter.rs`, `src/encoder_decoder.rs`) exposing an
entity with `new(&mut Window, &mut Context<Self>)` + `Render`. `src/layout.rs` owns the
`View` enum that drives both the sidebar menu and the main pane; the doc comment on `View`
lists the exact places to touch when adding a tool. Views are constructed once in
`Layout::new` and kept alive, so switching tabs preserves editor contents.

## Assets, icons and themes

`src/assets.rs` embeds `assets/icons/**/*.svg` and `assets/themes/**/*.json`, and falls back
to `gpui_component_assets::Assets` for anything missing — that is what makes library widgets
(dropdown carets, menu check marks, the dialog close button) find their icons without us
vendoring the whole Lucide set. Our own files shadow the library's by path, so dropping
`assets/icons/search.svg` in place also replaces `IconName::Search`.

Icons are registered as variants of `AppIcon` in `src/app_icon.rs` (variant → `icons/<file>.svg`).
Themes are vendored verbatim from the upstream `themes/` directory and loaded into
`ThemeRegistry` by `settings::init` (called right after `gpui_component::init` in `main.rs`);
`src/settings.rs` lists the theme *names* it offers, which come from the `name` field inside
those JSON files, not the file names.

## Settings and app-level state

`src/settings.rs` owns the Settings dialog. Appearance settings deliberately have no state
struct of their own: font size, border radius and colours are fields on the library's global
`gpui_component::Theme`, so the dialog reads/writes that global and calls `cx.refresh_windows()`
— that is the whole "apply live" mechanism. Language is the exception (`src/i18n.rs`), a
`Language` global plus a `Str` enum with one match arm per string; `t(Str::X, cx)` looks it up.
Nothing is persisted across restarts.

## gpui-component widget notes (git dep, non-obvious)

Source of truth is the cargo git checkout under
`~/.cargo/git/checkouts/gpui-component-*/<rev>/crates/ui/src` (rev pinned in `Cargo.lock`).

- **Overlays must be rendered by us, not by `Root`** (`root.rs`, and `docs/docs/root.md`).
  `Root::render` only paints its child view plus the tooltip/native-menu overlays.
  `window.open_dialog(..)` merely pushes onto `Root::active_dialogs`; the builder closure is
  invoked *only* from `Root::render_dialog_layer`, which the first-level view under `Root` —
  `DodoApp::render` — has to call. Omit it and the dialog opens in state and is never painted:
  the click looks dead with no error anywhere. Same contract for
  `render_sheet_layer` / `render_notification_layer`; add those the day we use a sheet or a
  notification, or they will fail the same silent way.
- **Multi-line code editor**: `gpui_component::input::InputState` + `Input::new(&state)`.
  Build with `InputState::new(window, cx).code_editor("json").multi_line(true).line_number(true)`.
  Read text via `state.value()`; replace via `state.set_value(text, window, cx)`
  (`replace_all` keeps undo history). `code_editor(lang)` gives tree-sitter highlighting;
  `"json"` and `"rust"` are supported languages.
  `InputState::new` and `set_value` require `&mut Window`, so views holding an editor must be
  built with a window (see `DodoApp::new`/`Layout::new`/`JsonFormatter::new` threading `window`).
- **Inline error highlighting**: only available in `code_editor` mode. Use
  `state.diagnostics_mut()` (returns `Some` only for code editors), then `reset(&rope)` +
  `push(Diagnostic::new(start..end, msg).with_severity(DiagnosticSeverity::Error))`.
  `Diagnostic`/`DiagnosticSeverity` live in `gpui_component::highlighter`; positions are
  `gpui_component::input::Position` (= lsp_types, 0-based line/character). Renders as a wavy underline.
- **Select/dropdown**: `gpui_component::select::{Select, SelectState}`. Build
  `SelectState::new(vec_of_items, Some(IndexPath::default()), window, cx)` (2nd arg = initial
  selection). `IndexPath` is at crate root. Read choice with
  `state.selected_index(cx).map(|ip| ip.row)`. Render with `Select::new(&state)`.
- **Settings panel + modal**: `gpui_component::setting::{Settings, SettingPage, SettingGroup,
  SettingItem, SettingField}` is a whole settings UI (left sidebar, search box, right pane) —
  don't hand-roll one. Open it with `window.open_dialog(cx, |dialog, _, cx| ...)` (`WindowExt`);
  the dialog already has a close button, Escape, and overlay-click dismissal. Its search only
  matches item titles/descriptions/`keywords`, so give items their section name as a keyword
  if searching by section should work. Give a page `.resettable(false)` unless you want its
  reset button. Fields are get/set closure pairs over `&App`/`&mut App`, so state lives in a
  global, not in the element.
- **Sidebar item selection**: `SidebarMenuItem::new(..).active(bool).on_click(|_, _, cx| ...)`.
  `on_click` gets `&mut App` (not a `cx.listener`), so capture `cx.entity()` and
  `view.update(cx, ..)` to mutate the parent view.
- **Trait imports**: `ButtonVariants` (for `.ghost()`/`.primary()`) is in
  `gpui_component::button`, not the crate root. `.opacity()` (Hsla) and most `Styled` sizing
  (`.w()`, `.rounded()`, `.font_bold()`) resolve via gpui's own traits / `StyledExt`.
- **Gotcha**: `cargo test` currently crashes rustc (SIGBUS / recursion limit) while expanding
  `#[test]` against the gpui macro tree in this environment. Rely on `cargo build` + `cargo run`
  for validation; unit tests in the `dodo` bin are not viable here.

## Maintaining this file

Keep this file for knowledge useful to almost every future agent session in this project.
Do not repeat what the codebase already shows; point to the authoritative file or command instead.
Prefer rewriting or pruning existing entries over appending new ones.
When updating this file, preserve this bar for all agents and keep entries concise.
