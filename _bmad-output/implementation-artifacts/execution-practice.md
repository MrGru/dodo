# Execution Practice — Practical Guide

**Stage:** Phase 4 (Implementation)

---

## The short answer

`CLAUDE.md` is dense but not huge — reading it in full at the start of a session is normal and
expected (it says so explicitly: "Read `src/main.rs` for the startup sequence and `src/layout.rs`
for the view model"). The practice that *does* matter, carried over from this folder's predecessor
because it's still good advice for any long-running agent session:

1. **Load a skill only when its trigger fires**, not preemptively — each skill's own file says so
   ("written to be read at the moment of need, not up front").
2. **Read a module's `mod.rs` before its individual files** when working inside `api_explorer/`,
   `docker/`, or `database/` — it is the authority on that module's structure, not something to
   re-derive from grepping every file.
3. **One story per focused unit of work.** A session that tries to carry three unrelated stories at
   once accumulates unrelated build/test noise that makes it harder to tell which change caused
   which test failure.
4. **Don't re-paste `CLAUDE.md` into every message** once it's been read — refer back to specific
   sections by name instead of repeating the whole file, the same economy argument the predecessor
   document made about its own (much larger) plan document.

## Anti-patterns to avoid

| Anti-pattern | Why it's wrong | Instead |
|---|---|---|
| Guessing a `gpui-component` API instead of loading `gpui-component-recipes` | Produces code that doesn't compile or silently doesn't render | Load the skill; check the pinned source checkout |
| Writing a bare string literal in a view | Fails an i18n `cargo test` guard | Load `dodo-i18n-text` before writing any label |
| Running `cargo update` "just to fix a build issue" | Silently jumps the four git dependencies to upstream HEAD | Never run it as a side effect; it's its own reviewed commit if ever needed |
| Marking a UI change done after only `cargo check` | Type-checking isn't the same as the feature working | Load `dodo-build-validate` and actually run it |
| "Fixing" a stated placeholder (E4.6, E5.7, E3.8) without reading why it was deferred | Risks reintroducing something deliberately cut for a real architectural reason | Read the owning module's `mod.rs` doc first |

## Where this folder's predecessor's advice doesn't transfer

The predecessor document (written for a different, much larger Python project under a hard
11-day deadline) also covered: fresh IDE sessions per story to manage a 3,000-line planning
document's token cost, a two-developer daily standup/cut-decision cadence, and slash-command
automation (`/cowork-run-story` etc.). None of that infrastructure exists in dodo — there is no
comparable token-cost problem (`CLAUDE.md` is a fraction of the size), no recorded team structure,
and no slash-command automation installed in this repo. Don't invent any of it as a side effect of
an unrelated story.
