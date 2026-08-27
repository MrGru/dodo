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

Four rules are subtle enough to name before you open the files:

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
  for a round. **Reaching back is an inference, though, and adjacency is a statement** (the
  captain's call, 2026-08-21): a modifier read from the shape of the word lands only over something
  `rules::is_valid_syllable` calls possible, which leaves the `d` that ends `download` alone while
  `did` is still `đi`; a key next to the letter it marks was read from nothing and lands regardless.
  `Syllable::intended_mark_target` is both halves and both schemes ask it, so `d9`/`di9` behave as
  `dd`/`did` do. A stated **stroke** goes further and takes the raw record out of play, so the
  spell-check restore cannot revoke it — `ddm` is `đm` and `ddc` is `đc`, the abbreviations. Only
  the stroke: `đ` is a letter of the alphabet a user can spell out, whereas a doubled *vowel* is
  stated just as loudly and `book` must still come back as `book`.
- **Undoing a modifier reaches back too, and adjacency decides its shape.** This is the rule that
  makes `window` type `window`: a repeat cancels the letter *its own key* made (`Letter::source`,
  never the rendered text), collapsing to one literal when nothing was typed since (`ww` → `w`) and
  otherwise putting the earlier key back **where it stands** while the new one still types itself
  (`ưindo` + `w` → `window`, not `indow`). A directly-marked letter's cancel therefore asks the
  **last** letter, not `mark_target`: `windoư`'s nucleus is a bare `i` that can carry no horn, so
  there was no target to ask and a second `w` grew `windoưư`.
- **A reverting key is accounted for exactly once, and `Syllable::raw` is the ledger that says
  so.** `raw` is not a transcript of the keys: a revert types its letter immediately, so that key
  is discharged (`Syllable::spend_reverting_key`) and the three reconstructions that rebuild the
  letters *from* `raw` cannot type it again — `insstead` is `instead`, not `insstead`. Only the
  *collapsing* shape discharges; `MarkOutcome::SourceRestored` puts the earlier key back **and**
  types the current one, so both entries stand or `window` loses its `w`. The engine cannot tell a
  deliberate escape from an English word that genuinely doubles the letter — `error` and `exxtra`
  are the same keystroke shape — so it honours the cancellation the user already saw.

**The accepted price** is stated in `vietnamese::tests`: a Latin word whose keys spell a *valid*
Vietnamese syllable is composed and stays composed, because the word-boundary restore in `rules`
only rescues invalid ones. So `dodo` types `đô` and `dad` types `đa`. Unikey does the same; it is
not a bug to fix, and it is the reason the plausibility rule above is the whole guard there is —
a word list of English exceptions is not on the table.

The second price is the doubling one, and it is a **decision** rather than a defect (the captain's
call, 2026-08-27): an English word whose repeated letter *is* a Telex control comes out one letter
short (`arrow` types `arow`, `offer` types `ofer`), and the way to type it is to spell the doubling
out — `arrrow`, `offfer`. The two readings cannot both be had, and the proof is in
`the_cancellation_reading_costs_english_words_that_double_a_control` in `vietnamese::tests`:
`effort` and `exxtra` reach the decision in step-for-step identical engine state and differ only in
which letters they are made of. The rejected reading — hand back every keystroke — was measured,
and it deletes the Telex escape itself (`marr` becomes `marr`, so `mar` is unreachable), which
`a_doubled_control_still_cancels_for_vietnamese` now pins. **Do not "fix" `arrow` without reading
both.**

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
