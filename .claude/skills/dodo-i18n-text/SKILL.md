---
name: dodo-i18n-text
description: The rule for every string a dodo user reads - it goes through `Str`, never a bare literal in view code - plus the exact steps for adding one (including parameterized strings), what legitimately stays English, and the two `cargo test` guards that enforce this. Load before writing or changing any user-facing text in dodo: a button label, a title, a placeholder, a description, an error message, a dropdown option. Also load when `view_code_draws_no_untranslated_literals` or an `i18n::tests` assertion fails.
---

The mechanism is `src/i18n.rs` and it is short — read it, do not take a summary of it from here.
This skill is the *rule* around it and the reason the rule needs enforcing.

## Why this exists

The `match` in `Str::text` makes a **missing translation** impossible: adding a language will not
compile until every string has a row. It does nothing about the failure that actually shipped —
someone wrote `.label("Decode JWT")` in a view and never involved `Str` at all. There is nothing
for the compiler to object to, so it reached the screen and was found by looking at it.

Two `cargo test` guards now close that (see *Guards* below). Neither replaces the rule.

## The rule

**Every string the user reads goes through `Str`.** In view code that means `t(Str::Foo, cx)` and
never a bare literal in a `.child` / `.label` / `.title` / `.description` / `.placeholder` /
`SettingItem::new` argument.

Exactly three kinds of literal are exempt, and they are exempt because a user never reads them
*as language*:

- **The product name, `"Dodo"`.** Untranslated in every language. It is the sole entry in the
  guard's `ALLOWED` list.
- **Identifiers that happen to be strings** — element ids (`Button::new("open-settings")`),
  code-editor language ids (`.code_editor("json")`), `ThemeRegistry` keys (the `THEMES` array),
  dropdown *values* (`"en"`, `"16"`), `Language::code()`. These must stay stable when the language
  changes; that is the whole point of them.
- **Developer text** — `eprintln!`, `expect`, `panic!`. Nobody is meant to see it.

Technical terms are **not** an exception to the rule. Base64, JWT, JSON, hex and URL are the same
words in English and Vietnamese, but a label made of them is still a label: `Str::FormatHex` exists
and renders `"Hex"` in both columns. Put the term in `Str` and give both languages the same text —
the guard has a way to say "identical on purpose" (below). Writing `.label("Hex")` instead is what
this skill exists to prevent, because the next such label will not be a technical term.

## Adding a string

1. A `Str` variant, in the section comment it belongs to.
2. One arm per language in `Str::text`. The compiler names the ones you missed.
3. Call it: `t(Str::Foo, cx)` → `SharedString`.
4. Add a sample to `samples()` in `src/i18n.rs`'s `mod tests` and a slot in `position()`.
   Step 4 is not optional — `position()` is exhaustive, so **the build fails until you do it.**
   Use `plain()` for prose, `term()` for a string that is deliberately identical in every
   language, `with()` for one carrying runtime values.

### Parameterized strings

This is the part people get wrong. A message carrying a runtime value is a **variant with fields**,
and each language formats the whole sentence:

```rust
Str::InvalidHexDigit { digit: char, position: usize }
```

Never `format!("{translated_prefix}{english_tail}")` and never a translated string with values
concatenated on. Word order differs between languages; a prefix cannot express that. `Str::text`
returns `Cow<'static, str>` precisely so parameterized arms can `format!` while plain ones stay
static.

Register these with `with(Str::Foo(SENTINEL), &["sentinel-text"])`, which asserts every language
actually interpolates the value. An arm that quietly drops its `{placeholder}` is a real bug this
has caught.

**Store errors as `Str`, not as rendered text.** Both tool views hold `error: Option<Str>` and call
`t()` in `render`, so a banner already on screen re-translates when the language changes. A
`SharedString` field would freeze it.

## What stays English, and why

- **Third-party parser detail.** serde_json's, base64's and `from_utf8`'s own messages arrive as
  the `detail: String` field of the surrounding variant. The frame around them is translated; the
  detail is not, because there is nothing to translate it with. This is deliberate, not a gap.
- **gpui-component's own strings** — dialog buttons, the Copy/Cut/Paste context menu, the settings
  search placeholder. They come from the library's `rust_i18n` catalogue, which ships
  en/zh-CN/zh-HK/zh-TW/it and **no Vietnamese**. They cannot be fixed by adding a `Str`. The
  library does support an app-supplied catalogue; `dodo-theming-settings` records what adopting
  that would cost. Until then, a Vietnamese user sees English on library chrome and that is known.

## Known quirk: the JSON diagnostic keeps its language

The inline wavy-underline hover message in the JSON formatter is rendered once and pushed into
`InputState` when the parse fails. It is not rebuilt each frame, so **switching language leaves it
in the old language until the next Format.** The error *banner* above the editor re-translates
immediately; only the hover text lags. Do not "fix" this by rendering the banner from a stored
`SharedString` to match — that would make both wrong instead of one.

The general form of this trap — library widgets that take a string at construction and cache it —
and the `sync_language` treatment for it are in `dodo-theming-settings`.

## Guards

```sh
cargo test i18n          # both guards
```

- `i18n::tests` — every `Str` variant, in every language, is non-empty; parameterized arms
  interpolate their values; and each variant is held to its declared kind, so a Vietnamese arm
  pasted from English fails and a `term()` that later diverges also fails.
- `i18n_lint::tests::view_code_draws_no_untranslated_literals` — scans `src/layout.rs`,
  `src/json_formatter.rs`, `src/encoder_decoder.rs` and `src/settings.rs` for a string literal
  sitting directly in a text-sink argument. It classifies by **position, not content**, which is
  why ids, format strings and registry keys do not trip it. `src/i18n_lint.rs`'s module doc states
  what it cannot see; it errs towards silence by design.

**A failing guard means the code is wrong.** The fix is a new `Str` variant and a `t()` call — it
is a two-line change and it is always the right one. Do not add to `ALLOWED`, do not delete a name
from `TEXT_SINKS`, and do not reach for `term()` to silence an untranslated string. Widening
`TEXT_SINKS` when a new widget takes display text is the one change that makes the guard stronger,
and is welcome.
