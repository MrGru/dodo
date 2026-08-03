# Epic E7 — Theming, Settings & i18n

**Source:** `src/settings.rs`, `src/i18n.rs`, `src/i18n_lint.rs`
**Stage:** Phase 3 (Solutioning)
**Status:** Done

---

Cross-cutting: every view in E1-E6 depends on `Str` and the theme registry existing.

| Story | Title | Status | Depends on |
|---|---|---|---|
| **E7.1** | Theme registry sourced from vendored JSON; writing `gpui_component::Theme` applies a change live, with no restart; font size, border radius, and language switch the same way | Done | E0.1 |
| **E7.2** | Every user-facing string routed through `Str`, enforced by two `cargo test` guards (no bare literal in view code; no untranslated literal reaches the screen); `ScriptPolicy` global starts each launch at "Ask for imported" regardless of the prior session | Done | E7.1 |

**AC (E7.1):** None of appearance/font-size/border-radius/language persist across restarts — that
scope is stated explicitly, distinct from the five files that do persist (E8).

**AC (E7.2):** A PR that adds a label without going through `Str` fails a `cargo test` run, not
just a review comment.
