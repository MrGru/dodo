# Implementation Readiness Check

**Source:** `PRD.md` §2.3, `implementation-artifacts/architecture-constraints.md`
**Stage:** Phase 3 (Solutioning) — PASS / CONCERNS / FAIL gate

---

Unlike a project building toward one first release, dodo is a live, largely-shipped app: this
check evaluates whether the *next* piece of work (whatever story is currently open in
`sprint-status.yaml`) is ready to start, not whether the whole product is ready to ship.

## Readiness checklist (per story, before starting work)

- [ ] The story's epic and dependencies are named in `sprint-status.yaml` and all its `deps` are
      `done`.
- [ ] The relevant skill (`gpui-component-recipes`, `dodo-tool-view`, `dodo-i18n-text`,
      `dodo-theming-settings`, `dodo-build-validate`) has been identified from
      `project-context.md`'s trigger table.
- [ ] If the story touches `src/database/`, the self-contained-module invariant
      (`grep -rn '^use crate::' src/database/ | grep -vE 'crate::(database,i18n,app_icon,paths)'`
      returns nothing) is understood as a constraint, not a suggestion.
- [ ] If the story touches any persisted file, whether it needs `#[serde(default)]`-only tolerance
      (`collections.json`'s pattern) or an explicit `"version"` field with refuse-on-newer
      (every other persisted file's pattern) has been decided up front.
- [ ] If the story adds or changes any user-facing text, `dodo-i18n-text` has been loaded before
      writing it.

## Decision: PASS

Every currently-`done` epic (E0-E9) satisfies this checklist retroactively per the source audit
recorded in `sprint-status.yaml`. The folder's forward-looking stories (OAuth2 auth type, Docker's
stated placeholders, Database Explorer's stated cuts, the GPL-3.0 distribution decision) are each
independent, have no unresolved `deps`, and may be picked up in any order.

## Concerns carried forward

1. **`README.md` is stale** relative to what's actually shipped (see `validation-report.md`) —
   worth fixing as its own small, separate task, not blocking any story here.
2. **The Windows and `macos-x64` release rows are experimental/unverified on real hardware**
   (`docs/release.md`) — this affects confidence in E9 (Build/Release), not readiness to work on
   it; any change to platform-specific release code should be flagged for manual verification
   rather than assumed correct from green CI alone.
3. **The GPL-3.0 distribution question is genuinely unresolved** — a story that touches
   `deny.toml`/`THIRD-PARTY-NOTICES.md` should not resolve it as a side effect of unrelated work.
