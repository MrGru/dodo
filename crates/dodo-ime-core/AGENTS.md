# `dodo-ime-core`

dodo's own input method, as pure logic: a normalized `KeyEvent` / `EngineAction` vocabulary
(`src/core/`) plus a Vietnamese engine speaking Telex and VNI (`src/languages/vietnamese/`).

Read `src/lib.rs` first — it owns why this is a crate rather than a module (the engine stays
independent of gpui and platform input APIs), the no-typing-history rule, the pass-the-key-through
rule, and the `--example telex` runner. `src/languages/vietnamese/mod.rs` and `syllable.rs` own the
Vietnamese rules themselves and are unusually complete; read them rather than a paraphrase.

This file holds the four things that span the crate.

## Never widen `purity_lint`'s allow-list

The crate boundary now guards what `purity_lint.rs` used to assert on paper, so the lint's remaining
job is the one thing `Cargo.toml` cannot do: **a dependency is one line and nothing warns**. The
allow-list turns adding one — *including a sibling workspace crate* — into a failing test, and
`the_scan_covers_every_file` proves the check reads every file.

When something outside is needed, **pass the value in at the boundary**. That is why persistence
and platform input listeners live in `dodo-input-method`: this lint forbids `serde::` and `gpui::`
by test.

The module-wide `#![allow(dead_code)]` this code carried as `src/input_method/` is gone — not
because it was wired up, but because everything is `pub` in a library and therefore reachable.

## The engine is semantic; the schemes decide only which key

A letter is `(base, mark, case)` and the tone belongs to the syllable, its position recomputed at
render time — so `toas` + `n` becomes `toán` without anything relocating a mark. `InputScheme` is a
plain enum for the same reason: Telex and VNI produce identical `Transform`s and share every rule
about Vietnamese, so neither file contains one.

Three rules are subtle enough to name before you open the files:

- **A doubled letter key states the case of the letter it marks** (the captain's call, 2026-08-14).
  `dD` is `Đ` and `Dd` is `đ`; `aA`/`Aa` read the same way, because the second press *is* that
  letter again and is the user's latest word on it. `Syllable::retypes_last_letter` is the whole
  rule and **both its conditions are load-bearing** — the key has to spell the letter (so `w`, the
  tone letters and every VNI digit leave case alone, and `Dd` is `đ` while `D9` is `Đ`), and nothing
  may have been typed since (so a stroke reaching back over a word keeps the case the user typed:
  `Did` is `Đi`). It lives in `syllable.rs` and not in `telex.rs`, which decides *which key* and
  nothing else. Undoing the mark restores the case with it, which is why `Ddd` is `Dd`.
- **Every modifier reaches back over the current syllable**, the stroke included since 2026-08-08
  (`did` is `đi`; `add` is still `add`, because the rule is about the *initial* letter). A scheme
  file that decides position itself rather than asking `Syllable::mark_target` is how that was wrong
  for a round.
- **Undoing a modifier reaches back too, and adjacency decides its shape.** This is the rule that
  makes `window` type `window`: a repeat cancels the letter *its own key* made (`Letter::source`,
  never the rendered text), collapsing to one literal when nothing was typed since (`ww` → `w`) and
  otherwise putting the earlier key back **where it stands** while the new one still types itself
  (`ưindo` + `w` → `window`, not `indow`). A directly-marked letter's cancel therefore asks the
  **last** letter, not `mark_target`: `windoư`'s nucleus is a bare `i` that can carry no horn, so
  there was no target to ask and a second `w` grew `windoưư`.

**The accepted price** is stated in `vietnamese::tests`: a Latin word whose keys spell a *valid*
Vietnamese syllable is composed and stays composed, because the word-boundary restore in `rules`
only rescues invalid ones. So `dodo` types `đô`. Unikey does the same; it is not a bug to fix.

## The corpus tests derive the keys, never the answer

`languages/vietnamese/corpus.rs` holds ~460 real words as **answers** and derives both key sequences
from them, so tone placement is never fed to the thing being tested. Add words there rather than
adding hand-written key sequences somewhere else.

## `LanguageId` is the shared keyboard-language identity

It is what dodo's menu bar and Input method pane mean by "which language", and it is persisted
through `input-method.json`. English and Japanese pass keys through until their engines exist.
`ActiveLanguages` defaults to English/Vietnamese and is the one menu and cycle set;
`LanguageSwitch` persists its key, modifiers and optional beep beside it. See
`crates/dodo-input-method/AGENTS.md` for the persistence contract.
