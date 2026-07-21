# Project agent memory

This file is the project's committed home for project-intrinsic agent knowledge: build, test, release, architecture, and sharp-edge notes that should travel with the code.

- Add durable project-specific notes here as they are discovered through real work.

## gpui-component widget notes (git dep, non-obvious)

Source of truth is the cargo git checkout under
`~/.cargo/git/checkouts/gpui-component-*/<rev>/crates/ui/src` (rev pinned in `Cargo.lock`).

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
