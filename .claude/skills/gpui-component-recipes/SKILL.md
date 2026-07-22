---
name: gpui-component-recipes
description: Verified gpui-component widget APIs for dodo - text inputs and code editors (InputState/Input), inline diagnostics, Select dropdowns, dialogs and the Settings panel, Sidebar menus, Buttons, Icons, and the trait imports each one needs. Load before writing or editing any render/new method that builds a gpui-component widget, when a widget call does not compile ("no method named ...", "trait not in scope"), or when a widget builds but nothing appears on screen.
---

Pinned revision: `gpui-component` **3c270ed** (see `Cargo.lock`). Source of truth is
`~/.cargo/git/checkouts/gpui-component-*/3c270ed/crates/ui/src`. Every snippet below was
compiled against that revision.

Upstream ships its own skills at `<checkout>/skills/{gpui,gpui-component}/`. They are good on
GPUI fundamentals (entities, elements, focus, actions) — read them for that. They are **stale on
three points at this revision**: it is `window.open_dialog` / `close_dialog`, not
`open_modal` / `close_modal`; the module is `gpui_component::setting` (singular), not
`settings`; and `SelectState` has no `selected_item()` — use `selected_index(cx)` or
`selected_value()`.

## Overlays are mounted by us, never by `Root`

`Root::render` paints only its child view plus the tooltip and native-menu overlays.
`window.open_dialog(..)` merely pushes onto `Root::active_dialogs`; the builder closure runs
**only** from `Root::render_dialog_layer`. Omit that call and the dialog opens in state and is
never painted — the click looks dead with no error anywhere. `src/app.rs` (the first-level view
under `Root`) is where it belongs:

```rust
impl Render for DodoApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let dialog_layer = Root::render_dialog_layer(window, cx);
        div().size_full().child(self.layout.clone()).children(dialog_layer)
    }
}
```

The same contract holds for `Root::render_sheet_layer` and `Root::render_notification_layer`;
add each the day a sheet or notification is used, or it fails the same silent way.

## Trait imports

Method-not-found on a widget is almost always a missing trait, and the trait is rarely where you
would guess:

| Method | Trait | Path |
|---|---|---|
| `.primary()` `.ghost()` `.danger()` `.link()` | `ButtonVariants` | `gpui_component::button` — **not** the crate root |
| `.xsmall()` `.small()` `.large()` | `Sizable` | `gpui_component` |
| `cx.theme()` | `ActiveTheme` | `gpui_component` |
| `.font_bold()` and every `font_*` weight, `.h_flex()`, `.v_flex()`, `.paddings()`, `.debug_red()`, `.popover_style()` | `StyledExt` | `gpui_component` — these are **not** gpui `Styled` methods, despite looking like it |
| `.when()` / `.when_some()` | `FluentBuilder` | `gpui::prelude` |
| `.w()` `.rounded()` `.bg()` `.flex_1()` `.min_h_0()` | `Styled` | already in `gpui::*` |

`Hsla::opacity(f32)` is an **inherent** gpui method — `cx.theme().danger.opacity(0.1)` needs no
import. (`gpui_component::Colorize` also defines `opacity` plus `divide`/`invert`/`lighten`; only
import it if you want those.)

## Text input and multi-line code editor

`InputState::new` and `set_value` both need `&mut Window`, so any view holding one must be
constructed as `new(window: &mut Window, cx: &mut Context<Self>)` and the window threaded down
from `Layout::new`.

```rust
use gpui_component::input::{Input, InputState};

// single line
let name = cx.new(|cx| {
    InputState::new(window, cx).placeholder("Your name").default_value("Ada")
});

// multi-line editor with a gutter
let editor = cx.new(|cx| {
    InputState::new(window, cx)
        .code_editor("json")   // must come first — it *replaces* the mode
        .multi_line(true)
        .line_number(true)
        .soft_wrap(true)
        .placeholder("Paste JSON here.")
});
```

`code_editor(lang)` already implies `multi_line: true`, `line_number: true`, indent guides,
folding, auto-indent, find/replace and a `DiagnosticSet` — dodo restates the first two only for
readability. Order matters, though: `line_number()` carries
`debug_assert!(mode.is_code_editor() && mode.is_multi_line())`, so calling it before
`code_editor()`, or after `multi_line(false)`, panics in debug builds.

Read and write:

```rust
let text = self.editor.read(cx).value().to_string();   // SharedString
let rope = self.editor.read(cx).text();                // &Rope, for diagnostics

self.editor.update(cx, |state, cx| {
    state.set_value(text, window, cx);   // clears undo history
    // state.replace_all(text, window, cx);  // same replace, but undoable — prefer for "Format"
    cx.notify();
});
```

React to typing instead of polling on a button press:

```rust
cx.subscribe(&self.name, |this, state, event: &InputEvent, cx| {
    if matches!(event, InputEvent::Change) { let v = state.read(cx).value(); }
}).detach();
```

Render it inside your own bordered box; the editor fills whatever it is given:

```rust
div().flex_1().min_h_0()
    .rounded(cx.theme().radius).border_1().border_color(cx.theme().border)
    .child(
        Input::new(&self.editor)
            .font_family(cx.theme().mono_font_family.clone())
            .text_size(cx.theme().mono_font_size)
            .size_full(),
    )
```

### `code_editor` highlights only the three languages this build compiles

Highlighting lives behind gpui-component's `tree-sitter` cargo features, and without any of
them `gpui_component::highlighter` compiles to `wasm_stub.rs`, whose
`SyntaxHighlighter::highlight` returns an empty vec — a gutter, indent guides, auto-indent,
find/replace and diagnostics, but no colour. That was dodo's state until the API Explorer
needed a coloured response body.

`dodo/Cargo.toml` now enables exactly **`tree-sitter` (JSON), `tree-sitter-html` and
`tree-sitter-yaml`**. Every other language string falls back to `Language::Plain` and renders
uncoloured — that is a graceful default, not a bug. Each language is separately feature-gated
and matched by `highlighter::Language::from_name` (`"json"`, `"html"`, `"yaml"`, `"text"`), so
adding one is a feature flag plus a `BodyKind` variant, not new highlighter code.
`["tree-sitter-languages"]` would enable all ~35 grammars; it was deliberately not used.

`InputState::set_highlighter(lang, cx)` re-points an existing editor at another grammar without
rebuilding it — how the response viewer switches per `Content-Type`.

## Inline diagnostics (wavy underline)

`diagnostics_mut()` returns `Some` **only** in `code_editor` mode. Positions are
`gpui_component::input::Position` (a re-export of `lsp_types::Position`): 0-based line and
character, so subtract 1 from anything 1-based like a `serde_json` error.

```rust
use gpui_component::highlighter::{Diagnostic, DiagnosticSeverity};
use gpui_component::input::Position;

self.editor.update(cx, |state, cx| {
    let rope = state.text().clone();
    if let Some(diagnostics) = state.diagnostics_mut() {
        diagnostics.reset(&rope);   // reset(&rope), not clear(), when re-anchoring to new text
        diagnostics.push(
            Diagnostic::new(Position::new(line, col)..Position::new(line, col + 1), message)
                .with_severity(DiagnosticSeverity::Error),
        );
    }
    cx.notify();
});
```

`DiagnosticSeverity` defaults to `Hint`, so always `.with_severity(..)`. To wipe them use
`diagnostics.clear()`.

## Select (dropdown)

```rust
use gpui_component::select::{Select, SelectState};
use gpui_component::IndexPath;   // crate root, not `select::`

let items: Vec<SharedString> = LABELS.iter().map(|s| SharedString::from(*s)).collect();
// 2nd arg is the initial selection; None means nothing selected.
let choice = cx.new(|cx| SelectState::new(items, Some(IndexPath::default()), window, cx));

// render
Select::new(&self.choice).small().w(px(140.))

// read — map the row back onto your own const array, the Select only knows labels
let row = self.choice.read(cx).selected_index(cx).map(|ip| ip.row);
```

The delegate is any `Vec<T>` where `T: SearchableListItem` (`String`, `SharedString`,
`&'static str` are implemented). To act on change rather than on a later button press:

```rust
cx.subscribe(&self.choice, |this, state, event: &SelectEvent<Vec<SharedString>>, cx| {
    let SelectEvent::Confirm(value) = event;
}).detach();
```

## Dialog and the Settings panel

`gpui_component::setting` is a complete settings UI — sidebar, search box, right pane. Do not
hand-roll one. `Dialog` already provides a close button, Escape, and overlay-click dismissal.

```rust
use gpui_component::setting::{SettingField, SettingGroup, SettingItem, SettingPage, Settings};
use gpui_component::WindowExt as _;

window.open_dialog(cx, |dialog, _, cx| {
    dialog.title("Settings").w(px(760.)).child(
        div().w_full().h(px(440.)).child(
            Settings::new("dodo-settings").sidebar_width(px(200.)).pages(pages(cx)),
        ),
    )
});
```

Fields are get/set closure pairs over `&App` / `&mut App`, so the state they edit must live in a
**global**, never in the element:

```rust
SettingField::dropdown(
    vec![(SharedString::new_static("en"), SharedString::new_static("English"))],
    |cx: &App| Language::current(cx).code().into(),
    |value: SharedString, cx: &mut App| Language::from_code(&value).set(cx),
)
.default_value("en")
```

Constructors: `switch` / `checkbox` (→ `SettingField<bool>`), `input` / `dropdown` /
`scrollable_dropdown` / `element` / `render` (→ `SettingField<SharedString>`), `number_input`
(→ `SettingField<f64>`). Use `scrollable_dropdown` for long lists — the plain `dropdown` popup
does not scroll and pushes options below the fold.

Four behaviours that surprise people:

- The search box matches an item's **title, description and `keywords` only** — never its page
  or group title, and by lowercase `contains`, not fuzzily. Pass the section name as a keyword
  if searching by section should work.
- A page shows a reset button unless you give it `.resettable(false)`.
- **`Settings` is a `RenderOnce` element over `window.use_keyed_state(self.id)`.** Its search
  input and selected page are that private state, so nothing outside can read the query or set
  the page. `default_selected_index` is read *only* when the state is first created, so the only
  way to drive the selection from outside is to hand `Settings::new` a **different id** —
  dodo keeps a `nonce` in `SettingsView` for exactly that. A new id also resets the sidebar's
  resizable width, which is the price.
- `SettingField<T>` implements `Styled`, and the refinement lands on the field's own control
  (e.g. the dropdown `Button`). That is the hook for highlighting one item.
  `header_style(&StyleRefinement::default().hidden())` is the hook for hiding the built-in
  search box; the sidebar's header wrapper still contributes its `pt_3`.

## Searchable `List` — the command-palette primitive

`ListState::new(delegate, window, cx).searchable(true)` is a search input, a virtualized result
list, keyboard nav and an empty state in one widget. Reach for it before hand-rolling a popover.
`ListDelegate` gives you `perform_search` (returns a `Task`, so it may be async), `items_count`,
`render_item` (→ `ListItem`, which the list styles and wires to click-confirm for you),
`render_empty` (the "no results" state), `render_initial` (shown only while the query is empty —
return `Some(div())` to collapse the panel to just the box), `set_selected_index` and `confirm`.

- **Keyboard works even though focus sits in the inner `InputState`.** gpui returns *all*
  matching bindings sorted by context depth and tries them until one is consumed. `Input`
  registers `MoveUp`/`MoveDown` listeners **only in multi-line mode**, and its `Enter`/`Escape`
  handlers call `cx.propagate()` for a single-line input, so `up`/`down`/`enter`/`escape` all
  fall through to the `List` context.
- **Escape keeps falling through to the `Dialog`**, which binds `escape` → `CancelDialog` and
  closes. `gpui_component::actions` is `pub(crate)`, so you cannot listen for its `Cancel`. Bind
  your own action in a context tighter than the input's: `Some("YourContext > Input")` matches at
  full depth and wins the tie on registration order, provided you `cx.bind_keys` *after*
  `gpui_component::init`.
- **`ListState::set_query` does not trigger a search**, despite its doc comment. It goes through
  `InputState::set_value`, which sets `emit_events = false`, so no `InputEvent::Change` fires and
  `perform_search` never runs. Clearing programmatically means `set_query("")` **plus** resetting
  `list.delegate_mut()` by hand.
- `confirm` runs while the list entity is leased, so anything that touches the list again (such
  as clearing the query) must go through `cx.defer_in(window, ..)` or it panics with
  "cannot update … while it is already being updated".
- The virtual list sizes itself with `size_full`, so give the container a **definite height**
  when results are showing; an auto-height parent can collapse it.

## Sidebar

```rust
Sidebar::new("side-bar")
    .collapsible(SidebarCollapsible::Icon)   // Icon | Offcanvas | None
    .collapsed(self.collapsed)
    .w(px(240.))
    .header(SidebarHeader::new().child("Dodo"))
    .child(SidebarGroup::new(t(Str::Tools, cx)).child(self.menu(cx)))
    .footer(SidebarFooter::new().child(/* button */))
```

`SidebarMenuItem::on_click` hands you `(&ClickEvent, &mut Window, &mut App)` — an `&mut App`,
**not** a `Context<Self>`, so `cx.listener` does not apply. Capture the entity instead:

```rust
SidebarMenu::new().children(View::ALL.map(|view| {
    let layout = cx.entity();
    SidebarMenuItem::new(view.title())
        .icon(view.icon().view())
        .active(self.active == view)
        .on_click(move |_, _, cx| {
            layout.update(cx, |this, cx| { this.active = view; cx.notify(); });
        })
}))
```

## Button and Icon

```rust
Button::new("format-json").primary().small().label("Format")
    .on_click(cx.listener(|this, _, window, cx| this.format(window, cx)))

Button::new("copy").ghost().icon(AppIcon::Binary).tooltip("Copy")
```

`.icon()` and `SettingPage::icon()` take `impl Into<Icon>`, and `impl<T: IconNamed> From<T> for
Icon` means any `AppIcon` variant goes in directly — `AppIcon::Json`, no wrapper. Where you need
a standalone element, `Icon::new(AppIcon::Settings)` (that is what `AppIcon::view()` in
`src/app_icon.rs` returns). Note the library's own `Icon::view(cx)` / `IconName::view(cx)` return
`Entity<Icon>` instead; dodo's same-named helper does not.
