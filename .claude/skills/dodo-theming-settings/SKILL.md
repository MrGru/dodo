---
name: dodo-theming-settings
description: How dodo's themes, font size, border radius and language switching actually work - where vendored theme JSON lives, how it reaches ThemeRegistry, why writing gpui_component::Theme applies live, how to add a language, and which library widgets cache their strings and need sync_language. Load when adding or changing a setting, adding/removing a theme or a language, or when a settings change does not take effect until restart. For adding or changing user-facing text, load `dodo-i18n-text` instead.
---

Two files own all of this: `src/settings.rs` (the dialog and the app-level state it edits) and
`src/i18n.rs` (localization).

**Everything on this page is persisted across restarts except one setting.** Theme, font size,
border radius and language are saved to `session.json` — the captain asked for session restoration
on 2026-08-06 — and re-applied by `settings::apply_session`, which `main` calls after
`session::load` and **before the window opens**, so the first frame is already the user's theme
rather than a flash of the default. `src/session/mod.rs` is the authority; this skill only owns how
the settings themselves work.

**The exception is `Run scripts`, and it is deliberate.** `ScriptPolicy` still resets to the
cautious `Ask for imported` at every launch: it is the gate in front of running code that arrived
inside someone else's collection file, not a preference about how the app looks, and "I allowed
this once" is not "allow it every morning". Its approvals *are* persisted, per script, in
`script-consent.json`. **Do not start persisting it** — that is the captain's call, not an
implementation detail, and a security default that silently stops resetting is exactly the kind of
change nobody notices until it matters.

**Two globals back this page, and a new setting has to pick one.** Appearance and language write to
the `Session` global (`session.json`); the Quick navigation page is built by `settings.rs` but every
value on it lives in the `QuickNav` global (`quick-nav.json`), which predates session restoration
and stays where it is. Either way the arrangement is the same and is the one to copy: a store behind
a trait in a `services/` module, a versioned document that refuses a higher version, load and save
on the background executor, coalesced writes, and a `SettingField` here whose closures read and
write the global. Do **not** add storage to `settings.rs` itself; it has none and deliberately keeps
none.

**The Features page is the one that picks neither, and it is worth knowing why before copying it.**
Which tools the sidebar lists is `Layout`'s state, because changing it has to move the main pane off
a tool that has just stopped being listed — so `settings::open` is handed a `WeakEntity<Layout>` and
the page calls `Layout::set_tool_enabled` / `move_tool`, which persist through `Session` and fix the
open tool as one act. Three consequences, all of which bite:

- **The handle is weak and must not be read while the page is constructed.** `settings::open` is
  reached from a click listener that has `Layout` leased; a read there panics at runtime with no
  compile error. `gpui-component-recipes` has the general form.
- **A change must `cx.refresh_windows()`.** The control lives in the dialog layer, a different
  entity, so `cx.notify()` on `Layout` repaints the sidebar and leaves the row the user just pressed
  showing its old value. `Layout::tool_list_changed` does it; `QuickNav::edit` does the same for the
  same reason.
- **A row is built by hand, not by `SettingField`.** A `SettingItem` is a label with a control beside
  it and no way to reach the row, so it can carry a switch but not a drag handle, a drop target or a
  position — and the position is the feature. `SettingItem::render` hands over the whole row instead.
  Reordering is gpui's own drag-and-drop (`on_drag` on the handle, `drag_over` + `on_drop` on the
  row — the primitives the library's dock tab bar uses) **plus** move-up/move-down buttons, which
  are the keyboard path a drag has none of. Ship both; the buttons are not a fallback.

**Enter does not press a button in this dialog; Space does.** `Button` is a tab stop and gpui
synthesizes a click from either key on a focused element, but `Dialog` binds `enter` to
`ConfirmDialog` in its own key context and the default `on_ok` returns `true`, which closes the
card. That is true of every control in the Settings dialog and always has been — do not "fix" it
from inside a page.

The rules themselves are nowhere near `settings.rs`: `session::models::features` is pure and holds
all of them, including the floor of one visible tool. Put a new rule there, not in the page.

## Themes

Theme JSON is vendored verbatim from gpui-component's own `themes/` directory into
`assets/themes/*.json` and embedded by `src/assets.rs` (`rust-embed`, `#[include =
"themes/**/*.json"]`). `Assets::themes()` iterates them; `settings::init` feeds each file to
`ThemeRegistry::load_themes_from_str`.

**Ordering is load-bearing.** `settings::init(cx)` must run *after* `gpui_component::init(cx)` —
that is what creates the `ThemeRegistry` global (`gpui_component::init` → `theme::init` →
`registry::init` → `cx.set_global(ThemeRegistry::default())`). `src/main.rs` calls them back to
back with a comment saying so.

Three facts that bite:

- **A file is a `ThemeSet`, not a theme.** Each JSON has a `name` for the set and a `themes: []`
  array whose entries carry their own `name`. `src/settings.rs`'s `THEMES` const lists those
  *inner* names, never file names. The 12 vendored files currently register **24** themes while
  the dialog offers 16 — the rest are registered and selectable via `ThemeRegistry` but simply
  not listed. Add a name to `THEMES` to expose one.
- **`load_themes_from_str` silently skips a theme whose name is already registered**
  (`if !self.themes.contains_key(&theme.name)`). Duplicate names mean the first file loaded wins,
  with no error.
- **"Default Light" / "Default Dark" are built in**, from the library's `default-theme.json`, not
  from `assets/themes/`. `theme::init` starts the app on Default Light.

Applying a theme (`settings::set_theme`) reads the config out of the registry and calls
`Theme::apply_config`. That method also overwrites `font_size`, `radius` and `radius_lg` from
whatever the theme file specifies, so dodo re-asserts the user's own font size and radius
immediately afterwards. Keep that re-assert if you touch `set_theme`.

## Font size, radius, and why changes are live

Appearance settings deliberately have **no state struct of dodo's own**. `font_size`, `radius`,
`radius_lg` and every colour are public fields on the library's global `gpui_component::Theme`,
which the whole app already renders from. The dialog writes the global and repaints:

```rust
fn set_font_size(size: f32, cx: &mut App) {
    Theme::global_mut(cx).font_size = px(size);
    cx.refresh_windows();
}
```

That is the entire "apply live" mechanism — `Theme::global_mut` + `cx.refresh_windows()`. There
is no observer, no event, and nothing to subscribe to.

`Theme::font_size` scales the *whole* UI, not just body text: `Root::render` calls
`window.set_rem_size(cx.theme().font_size)`, so every rem-based dimension in every widget
follows it.

`radius_lg` (dialogs, notifications, popovers) is a separate field from `radius`. dodo sets both
to the same value so that picking 0px squares off overlays too; set only `radius` and dialogs
stay rounded while buttons go square.

A new appearance setting is therefore: a `SettingField` whose getter reads `Theme::global(cx)`
and whose setter writes `Theme::global_mut(cx)` then calls `cx.refresh_windows()`. See
`gpui-component-recipes` for the `SettingField` / `SettingPage` API and its search-keyword
gotcha. Dropdown *values* are stable identifiers (`"en"`, `"16"`), never localized labels, so a
stored choice does not change meaning when the language does.

## Localization

**Adding or changing a user-facing string is `dodo-i18n-text`'s subject, not this skill's** — the
rule, the steps, the exemptions and the two `cargo test` guards all live there. This section covers
only what localization has to do with *settings*: switching the language, and adding a new one.

**Add a language**: a `Language` variant, a row in `Language::ALL`, arms in `code()` and `label()`,
and a column in every `Str::text` row (and in `JwtPart::name`) — the compiler enumerates the ones
you missed. `code()` is the stable dropdown value; `label()` is the language's name *in* that
language, so it is deliberately not translated. Expect `cargo test i18n` to then fail on any row
you filled in by pasting the English text; that is the guard working.

`Language::set` does `cx.set_global(self)` then `cx.refresh_windows()`, the same live-apply
trick as `Theme`. Because `t()` is called during `render`, already-painted strings pick up the
new column on the next paint — there is no catalogue reload and no missing-key path.

**The catch: some library widgets cache strings instead of re-rendering them.** `InputState`
placeholders and `SelectState` item labels live inside library entities and are *not* rebuilt
each frame, so `refresh_windows()` alone leaves them in the old language. Both tool views hold a
`language: Language` field and call `sync_language(window, cx)` at the top of `render`, which
pushes new text in when that field goes stale (`set_placeholder`; `set_items` followed by
`set_selected_index`, because `set_items` alone leaves the closed dropdown showing the old
label). Any new widget that takes a string at construction time needs the same treatment.
(`dodo-i18n-text` records the one string that is knowingly left out of this and why.)

**gpui-component's own strings cannot be fixed here.** Dialog buttons, context-menu entries
(Copy/Cut/Paste), and the settings search placeholder come from the library's `rust_i18n`
catalogue (`<checkout>/crates/ui/locales/ui.yml`), which ships en/zh-CN/zh-HK/zh-TW/it and no
Vietnamese. The library *does* support an app-supplied catalogue — `<checkout>/docs/docs/i18n.md`
— via an app-side `locales/ui.yml` under a `gpui_component:` namespace, `rust_i18n::i18n!` at our
crate root, `rust_i18n::extend!(gpui_component)` before `gpui_component::init`, and
`rust_i18n::set_locale` wired to `Language::set`. That is a second, separate mechanism; adopt it
only deliberately.
