---
name: dodo-theming-settings
description: How dodo's themes, font size, border radius and language actually work - where vendored theme JSON lives, how it reaches ThemeRegistry, why writing gpui_component::Theme applies live, and how to add a localized string or language. Load when adding or changing a setting, adding/removing a theme, adding user-visible text that should be translated, or when a settings change does not take effect until restart.
---

Two files own all of this: `src/settings.rs` (the dialog and the app-level state it edits) and
`src/i18n.rs` (localization). Nothing is persisted across restarts — every setting resets to its
default on launch. Adding persistence means introducing storage that does not exist today.

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

`src/i18n.rs` is deliberately tiny: an enum of strings, a match arm per (string, language) pair,
and a `Language` global.

- **Add a string**: a `Str` variant + one arm per language in `Str::text`. Call it as
  `t(Str::Foo, cx)`, which returns `SharedString`.
- **Add a language**: a `Language` variant, a row in `Language::ALL`, arms in `code()` and
  `label()`, and a column in every `Str::text` row — the compiler enumerates the ones you missed.
  `code()` is the stable dropdown value; `label()` is the language's name *in* that language.

`Language::set` does `cx.set_global(self)` then `cx.refresh_windows()`, the same live-apply
trick as `Theme`. Because `t()` is called during `render`, already-painted strings pick up the
new column on the next paint — there is no catalogue reload and no missing-key path.

Not everything is localized: tool titles in `src/layout.rs` (`View::title`) and the button and
banner text inside each tool module are hard-coded English. Only the sidebar group label and the
Settings dialog go through `t()`.
