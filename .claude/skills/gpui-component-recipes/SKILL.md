---
name: gpui-component-recipes
description: Verified gpui-component widget APIs for dodo - text inputs and code editors (InputState/Input), inline diagnostics, Select dropdowns, dialogs and the Settings panel, Sidebar menus, Buttons, Icons, the trait imports each one needs, and how gpui decides which key binding wins. Load before writing or editing any render/new method that builds a gpui-component widget, before adding or changing a key binding, focus handle or key context, when a widget call does not compile ("no method named ...", "trait not in scope"), when a widget builds but nothing appears on screen, or when a keystroke reaches the wrong handler or none at all.
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

## Key bindings: depth decides who wins, and `!` is how you say "not here"

Everything below is gpui's, not the widget library's, but it is where dodo's key handling lives
and it has bitten a round already. `crates/gpui/src/keymap.rs` and `keymap/context.rs` are the
source; `src/quick_nav/mod.rs` is the worked example, and its tests drive the predicates directly
with `KeyContext::parse` + `KeyBindingContextPredicate::depth_of`, which needs no window.

- **A binding with `None` context is treated as the *deepest* match, not the shallowest.**
  `Keymap::binding_enabled` returns `Some(contexts.len())` for one. So a context-less binding beats
  the focused input's own bindings — bind `p` that way and the letter disappears from every text
  field in the app. Give every app-level binding a context.
- **gpui collects *all* matching bindings, sorts them deepest-context-first, and dispatches each
  in turn until one stops propagating** (`Window::dispatch_key_event`). That is what lets a
  shallow binding be a fallback: `InputState::escape` calls `cx.propagate()` unless it has a
  completion popup or an IME composition to close, so an `escape` bound at an ancestor context runs
  only when the input declined it. Ties at equal depth go to whichever was registered *later* —
  hence every `dodo::init` running after `gpui_component::init`.
- **`!Foo` is evaluated against the whole dispatch path**, not just the node being tested
  (`KeyBindingContextPredicate::Not`). So `MyPane && !Input` means "somewhere under MyPane, with no
  text input anywhere between the focused element and the root" — the honest way to express a
  vim-style normal mode, because it is derived from real focus rather than from a flag you maintain.
- **With nothing focused, the dispatch path is the window root alone.**
  `focus_node_id_in_rendered_frame` falls back to `root_node_id()` when `window.focus` is `None`,
  so *none* of your `key_context`s are in the path and none of your bindings match. If a binding
  must work before the user has clicked anything, give the view a `FocusHandle`, `track_focus` it
  on the element carrying the context, and `window.focus(&handle, cx)` in its constructor.
- **A dialog is not inside your pane.** `Root::render_dialog_layer` is mounted by `DodoApp::render`
  as a *sibling* of `Layout`, so a dialog's focus path contains none of the pane's contexts. You do
  not need to exclude dialogs from a pane-scoped binding; they are already out of reach.

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

### `code_editor` highlights only the languages this build compiles

Highlighting lives behind gpui-component's `tree-sitter` cargo features, and without any of
them `gpui_component::highlighter` compiles to `wasm_stub.rs`, whose
`SyntaxHighlighter::highlight` returns an empty vec — a gutter, indent guides, auto-indent,
find/replace and diagnostics, but no colour. That was dodo's state until the API Explorer
needed a coloured response body.

`dodo/Cargo.toml` enables **JSON, HTML, YAML and JavaScript** (the `syntax-highlighting`
feature) plus **SQL** (its own `sql-highlighting` feature, kept separate because it is by far
the most expensive). Every other language string falls back to `Language::Plain` and renders
uncoloured — that is a graceful default, not a bug. Each language is separately feature-gated
and matched by `highlighter::Language::from_name`, so adding one is a feature flag plus a
`BodyKind` variant, not new highlighter code. `["tree-sitter-languages"]` would enable all ~35
grammars; it was deliberately not used.

### `set_highlighter` destroys a highlighter and builds none — always pair it with `refresh`

The name reads like "re-point this editor at another grammar", and that is what dodo's Database
Explorer assumed. What it actually does is set the language, drop the `Option<SyntaxHighlighter>`
to `None` and cancel the in-flight parse task. **Nothing schedules a new one.** Only two things
build a highlighter: an edit (`replace_text_in_range`, forced) and a render with the private
`_pending_update` flag set. `set_value` / `replace_all` set that flag; `set_highlighter` does not.

So the two rules, both learned the expensive way — the SQL editor shipped drawing black text:

```rust
// Right: on a change, and paired.
self.editor.update(cx, |state, cx| {
    state.set_highlighter(language, cx);
    state.refresh(cx);   // "the next render re-runs syntax highlighting", per its own doc
});
```

- **Never call it from `render`.** It `cx.notify()`s, which guarantees another frame, which wipes
  the highlighter that frame built — a loop in which no coloured frame ever survives. Guard it so
  it fires only when the language actually changed
  (`database::state::editor::EditorLanguage`), or call it from the change handler instead
  (`api_explorer::state::request::apply_body_language` says so in its own doc comment).
- **Never call it alone.** Without a following `refresh` — or a `set_value` right after, which is
  how `api_explorer::state::tab` gets away with it — the editor stays uncoloured until the user's
  next keystroke.

`src/database/state/editor.rs`'s module doc is the full diagnosis, including why the colour
appeared for one frame after Format and never while typing.

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

### A confirmation must be `open_alert_dialog`; `Dialog::on_ok` alone draws no button

**`Dialog` renders a footer only when it is given one.** `.on_ok(..)` / `.on_cancel(..)` set
`button_props`, but `DialogButtonProps::render_ok` / `render_cancel` are called from exactly one
place in the library — `AlertDialog::into_dialog`, which supplies the default OK/Cancel footer
when the author set none. `Dialog`'s own `RenderOnce` never calls them. So

```rust
window.open_dialog(cx, |dialog, _, cx| dialog.title(..).child(..).on_ok(..));  // WRONG
```

opens a card with a message and **no confirm button at all**. The close cross runs `on_cancel`,
the backdrop dismisses, Escape cancels — every control the user can see refuses. `on_ok` is
reachable only by the `enter` binding in the `Dialog` key context, so the action appears to do
nothing and a synthetic-keystroke test "confirms" it works. That shipped in dodo: deleting a
saved connection did nothing at all (`src/database/views/database.rs`, `confirm_delete`'s guard
test records the diagnosis).

Use the alert dialog, as `docker::views::containers::confirm_delete` and the three confirmations
in `database::views::database` now do:

```rust
window.open_alert_dialog(cx, move |alert, _, cx| {
    let view = view.clone();
    alert
        .title(t(Str::DbDeleteConnectionTitle, cx))
        .description(t(Str::DbDeleteConnectionMessage(name.clone()), cx))
        .button_props(
            DialogButtonProps::default()
                .ok_text(t(Str::DbDeleteConnection, cx))
                .ok_variant(ButtonVariant::Danger)
                .cancel_text(t(Str::DbCancel, cx))
                .show_cancel(true),     // default is false — no Cancel button otherwise
        )
        .on_ok(move |_, _, cx| { view.update(cx, |this, cx| this.confirm(cx)); true })
});
```

`DialogButtonProps` is `gpui_component::dialog`, `ButtonVariant` is `gpui_component::button`.
**Do not call `window.close_dialog(cx)` inside `on_ok`**: returning `true` makes the library call
it, and doing both pops two entries off `Root::active_dialogs` — harmless with one dialog open,
not with two.

A dialog whose body is your own entity (the connection form, the updater, `docker::views::detail`)
is unaffected: its buttons live in the body it renders, not in the footer.

None of this can be caught at runtime on macOS. `Root::new` calls
`macos_accessibility::install_window_hit_test_forwarder`, which dereferences a real `NSView`
(the `not(test)` cfg on it is *gpui-component's* test profile, not dodo's), so a GPUI test window
cannot host a `Root` and there is no dialog layer to drive. Guard it by scanning source, the way
`i18n_lint` does.

**The dialog layer is a stack, and nothing in the library asks whether your dialog is already
showing.** `open_dialog` pushes, every time. That is right for a confirmation raised over an
editor, and wrong for a dialog reachable from two unrelated places — Settings (the sidebar footer
and the menu bar item) and the updater (the sidebar footer and a background check) both shipped
two identical stacked cards. `src/dialog_slot.rs` is the shared fix: a marker type per dialog,
`claim` before building anything, `release` from `on_close`. Two rules come with it. `release`
clears a flag and must **never** be paired with a second `close_dialog` — `on_close` fires *after*
the library's own single pop, so releasing there is free, while a dialog closing itself from its
own button (the updater's **Later**) releases beside its one `close_dialog` call. And the window,
not the flag, is the authority: `decide_open` clears a flag that no dialog backs, so a missed
release cannot make the dialog unopenable for the rest of the session. Reach for it only when
there really are two ways in; every other dodo dialog is opened from a control the modal overlay
covers, so it cannot be asked twice.

**`Dialog` is also the only correct way to be modal.** It paints an `anchored().snap_to_window()`
layer that is `.occlude()`d and `cx.stop_propagation()`s the backdrop, binds `escape` in its own
`Dialog` key context, `focus_trap`s the card, and restores the previously focused handle on close.
A hand-rolled `div().absolute().inset_0()` scrim does **none** of that: it covers only its
positioned ancestor, and an empty `on_mouse_down` closure registers a listener that swallows
nothing, so clicks and hovers still reach whatever is behind it. `crates/dodo-docker/src/views/detail.rs`'s
module doc is the worked example.

Two things bite when the dialog body is your own view:

- **The body must be an entity**, not a struct rendered by the page. `Root::render_dialog_layer`
  builds the dialog from its own closure, so nothing there observes the page entity and a page
  `cx.notify()` does not repaint the dialog. Hand `open_dialog` an `Entity<YourView>` (as
  `settings::open` does) and let *its* `cx.notify()` do the work.
- **That body entity may not read the page entity while it is being constructed.** `open` is
  reached from a click listener, so the page is *leased* for the whole call; a
  `page.read(cx)` inside the body's `new` — even indirectly, via a helper that later works fine —
  panics at runtime with `cannot read <Page> while it is already being updated`. There is no
  compile error and no warning. Seed the body with **plain data the caller reads for it**
  (`environments_editor::open` takes the scope's name and variables; `docker::views::detail` takes
  a `DetailRequest`), and read the page only from the body's own later listeners, where the lease
  is gone. Deferring does not help: `cx.defer_in`'s closure is handed `&mut Self`, so the page is
  leased there too.
- **State the body's width; do not use `w_full`.** A percentage width only resolves against an
  ancestor with a *definite* width, and inside the dialog's nested wrappers it resolves to `auto`
  — which content-sizes the body to its widest child, leaving section headings, rules and code
  editors short. Use `.content(|content, _, _| content.child(div().w(card_w - px(32.))…))`:
  `.content()` avoids the `overflow_y_scrollbar` box that plain `.child()` children are wrapped
  in, and `px(32.)` is `Dialog`'s own default `Edges::all(16)` padding. `Dialog` also computes
  `left` from the width it was given, so an over-wide card is pushed off-centre rather than
  clipped — shrink the width against `window.viewport_size()` *before* building the dialog.

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

### A `SettingField::input` item must be `.layout(Axis::Vertical)`

An input is the only control in this dialog wide enough to break its own row.
`fields/string.rs` gives it `w_64` — a **fixed 256px** — in a horizontal row, where `switch` and
`checkbox` are content-sized, `dropdown` is a content-sized `Button` and `number_input` is `w_32`.
`SettingItem::render_item` then lays the row out as `h_flex().justify_between().gap_3()` with the
label column at `flex_1().max_w_3_5()` and the control in a bare `div().id("field")` that **nothing
shrinks**. So the row needs `256 + 12 + 0.6 * row` to hold both, and dodo's rows are 494px wide:
at `DIALOG_WIDTH` the input is laid out at x=524.5 with width 256, reaching 780.5px inside a 726px
panel — outside the card, where it is clipped. That shipped in the Quick navigation page's first
round and is what `settings::input_item` now prevents; `settings::row_layout` measures both halves
against a real frame.

Stacked, the control is `w_full` and therefore bounded by the row at every width. The library
reaches for the same stacked layout by itself — `Settings::render` wraps the page in a
`container_query` and flips to `Axis::Vertical` at `STACKED_LAYOUT_MAX_WIDTH` — but only once the
whole panel has dropped to **480px**, which a 760px dialog never does. `render_item` honours a
per-item `Vertical` and a container-wide `Vertical` overrides a per-item `Horizontal`, so setting
it per item is safe.

Note the measuring technique, which generalises: `container_query` lays its child out with
`AvailableSpace::Definite`, so a settings page's widths are real numbers even though a dialog
cannot be hosted in a GPUI test window on macOS. Render the panel into a `div` of the width the
dialog would hand it, tag elements with `.debug_selector(|| "…")` and read them back with
`VisualTestContext::debug_bounds`. Library internals cannot be tagged — stand in for the field
with `SettingField::render`, which hands your closure the `RenderOptions` (including the resolved
`layout`) the real field would get.

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
    .child(SidebarGroup::new(t(Str::Tools, cx)).children(self.menu(cx)))
    .footer(/* a plain v_flex — see below */)
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

### Collapsed, the rail is 48px and every inset is countable

The collapsed width is `COLLAPSED_WIDTH` in `sidebar/mod.rs` — **48px, and not exported**, so
dodo restates it. Getting anything to line up with the collapsed tool icons is arithmetic, and
all of it lives inside the library:

| box | expanded inset | collapsed inset |
|---|---|---|
| `Sidebar`'s `#inner` (holds the groups) | `px_3` | `p_2` |
| `Sidebar`'s `#header` / `#footer` wrappers | `px_3` | `px_2` |
| `SidebarMenuItem`'s own row | `p_2`, `h_7` | `p_2`, `justify_center` |

So a collapsed menu row gets a 31px box and centres its 16px icon in it. **`SidebarHeader` and
`SidebarFooter` each add a *second* `p_2`** (plus a hover highlight spanning the whole block),
which halves that box to 15px and puts anything inside it out of reach of the rail's centre.
`Sidebar::header`/`footer` take `impl IntoElement`, so the fix is to pass your own element and
skip the wrapper — `src/layout.rs`'s `footer_button` is the worked example, and its doc comment
carries the numbers.

### A `SidebarMenuItem` has no tooltip — wrap it in your own `SidebarItem`

There is no tooltip field and no builder for one at this revision, and `SidebarMenu::children`
accepts nothing but a `SidebarMenuItem`, so it cannot be added from inside the menu either.
(The upside: there is no library tooltip to duplicate.) `SidebarItem` is a **public trait**
(`Collapsible + Clone` plus one `render`) and `SidebarGroup<E: SidebarItem>` takes any
implementation, so a thin wrapper is the way in:

```rust
impl SidebarItem for ToolItem {
    fn render(self, id: impl Into<ElementId>, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let id = id.into();
        div()
            .id(SharedString::from(format!("tool-tip-{id}")))   // `tooltip` is StatefulInteractiveElement
            .w_full()
            .when(self.collapsed, |this| {
                this.tooltip(move |window, cx| Tooltip::new(title.clone()).build(window, cx))
            })
            .child(self.item.collapsed(self.collapsed).render(id, window, cx))
    }
}
```

`SidebarGroup` already stacks its children with the same `gap_2` `SidebarMenu` uses, so dropping
`SidebarMenu` in favour of `SidebarGroup::children` changes nothing that is drawn. Forward the
`collapsed` flag the group hands you — it is how the row knows to render as an icon *and* how
the wrapper knows to show the tooltip. This is still one flat row per tool: `dodo-tool-view`
explains why nesting stays out.

## Button and Icon

```rust
Button::new("format-json").primary().small().label("Format")
    .on_click(cx.listener(|this, _, window, cx| this.format(window, cx)))

Button::new("copy").ghost().icon(AppIcon::Binary).tooltip("Copy")
```

`.icon()` and `SettingPage::icon()` take `impl Into<Icon>`, and `impl<T: IconNamed> From<T> for
Icon` means any `AppIcon` variant goes in directly — `AppIcon::Json`, no wrapper. Where you need
a standalone element, `Icon::new(AppIcon::Settings)` (that is what `AppIcon::view()` in
`crates/dodo-app-icon` returns). Note the library's own `Icon::view(cx)` / `IconName::view(cx)` return
`Entity<Icon>` instead; dodo's same-named helper does not.

### `justify_*` on a `Button` aligns nothing; its padding depends on `.icon()` vs `.child()`

Two traps that look like styling and are not:

- **`Button` wraps its contents in `h_flex().size_full().items_center().justify_center()`.**
  That inner box fills the button, so `.justify_start()` on the button itself is a **no-op** for
  where the icon and label land — it only moves a box that is already 100% wide. To left-align a
  wide button's contents, give *your own child* `.w_full()` and let its own flex do it.
- **Padding is chosen by whether the button has children.** `.label(..)`/`.child(..)` take the
  "normal button" branch (`h_8` and **`px_4`** at the default size); an icon-only button
  (`.icon(..)` with no label and no child) takes `size_8` instead. So a button built with
  `.child(h_flex().child(icon))` is *not* an icon button, and its 16px side padding is wider
  than a 48px collapsed sidebar rail can hold. Override with `.px_0()`/`.px_2()`; the user
  refinement is applied after the size branch, so it wins.

`Button::tooltip(impl Into<SharedString>)` exists and takes plain text — no `Tooltip::new` and no
closure. Reach for it before hand-rolling one on a wrapper.
