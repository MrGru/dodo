# Blocked Decision Tree — What to Do When Stuck

**Stage:** Phase 4 (Implementation)

---

## "A `gpui`/`gpui-component` API doesn't do what I expect"

1. Check the pinned source at `~/.cargo/git/checkouts/gpui-component-*/<rev>/crates/ui/src`
   (rev from `Cargo.lock`) — it is the reference, not crates.io docs (there is no published
   version) and not an older tutorial.
2. Load the `gpui-component-recipes` skill; it records which of the checkout's own `skills/` are
   stale.
3. If still stuck, the widget genuinely may not support what's needed — say so rather than
   hand-rolling a workaround that fights the framework (see `docker/views/detail.rs`'s module doc
   for a worked example of a hand-rolled scrim that had to be abandoned for `window.open_dialog`).

## "A test fails for a reason unrelated to my change"

1. Stop. Do not silently skip or delete the assertion.
2. Confirm with `git stash` / a clean checkout whether the failure pre-dates your change.
3. If it pre-dates your change: it's a pre-existing defect, out of scope for the current story —
   note it, don't fix it as a drive-by inside an unrelated commit.
4. If your change caused it: fix the regression before proceeding.

## "`cargo clippy` or `cargo fmt` fails and I don't understand why"

1. Read the message — do not add a crate-level `#[allow]`; the codebase has none by design.
2. If the fix genuinely isn't clean, allow it at the specific definition with a one-line reason
   inline, matching the two existing precedents in the codebase (see `CLAUDE.md`'s note on the
   "two surviving suppressions").

## "I need to touch `src/database/` and I'm not sure if it stays self-contained"

1. Run `grep -rn '^use crate::' src/database/ | grep -vE 'crate::(database,i18n,app_icon,paths)'`
   before and after your change.
2. If it's non-empty after your change, the design is wrong — route through an existing
   `database::` re-export or reconsider the feature, rather than reaching into another tool
   directly.

## "The story is a placeholder (E3.8/E4.6/E5.7) and I'm not sure how much to build"

1. Read the owning module's `mod.rs` doc comment for exactly what was deliberately deferred and
   why — build only that, not a broader feature nobody asked for.
2. If ambiguous, build the smallest real version and leave the rest as a still-labeled "coming
   soon" control, per `product-brief.md` §1.6's posture (a stated cut is not a bug to silently
   over-fix into a much larger feature).

## "I'm not sure whether something needs a new ADR"

1. If it changes a decision recorded in `architecture-decisions.md`, or introduces a new
   cross-cutting pattern (a new persisted file, a new outside-world crate, a new sandbox), it needs
   one.
2. If it's a straightforward extension inside an existing pattern (a new request tab, a new list
   page), it doesn't.

## "The Windows or Linux build/release path is involved"

1. Do not assume CI green means verified on a real host — `docs/release.md`'s "What 'verified'
   means" is the honest record; a change here should be flagged for manual verification, not
   assumed correct.
