# Installed BMAD Modules — Registry

This file documents what's available in this BMAD-METHOD installation. The actual binaries are
under `_bmad/` (configured by `_bmad/_config/manifest.yaml`): `core` and `bmm` (both built-in,
v6.10.0) and `tea` (Test Engineering Architect, v1.19.1, external). Tiers 1/2/4 below are generic
to that installation and not specific to any project; Tier 3 is dodo's own project-specific skill
set, corrected below (the version previously here described a different project's skills, none of
which exist in this repository's `.claude/skills/`).

## Tier 1 — BMAD core skills (always available)

| Skill | Trigger | When to invoke |
|---|---|---|
| `bmad-help` | `/bmad-help` | Context-aware next-action menu |
| `bmad-advanced-elicitation` | `/bmad-advanced-elicitation` | After any artifact draft, to refine it |
| `bmad-party-mode` | `/bmad-party-mode` | When several agent perspectives should debate an approach in one session |
| `bmad-review-adversarial-general` | `/bmad-review-adversarial-general` | End of any artifact draft |
| `bmad-review-edge-case-hunter` | `/bmad-review-edge-case-hunter` | Before a code review |
| `bmad-shard-doc` | `/bmad-shard-doc` | When a source doc needs splitting |
| `bmad-index-docs` | `/bmad-index-docs` | After a major doc change |
| `bmad-spec` | `/bmad-spec` | Distill an intent into a spec kernel |
| `bmad-brainstorming` | `/bmad-brainstorming` | New feature ideation |
| `bmad-investigate` | `/bmad-investigate` | Forensic investigation when something's wrong |

## Tier 2 — BMAD canonical workflows

| Workflow | Produces |
|---|---|
| `bmad-product-brief` | `planning-artifacts/product-brief.md` |
| `bmad-prfaq` | `planning-artifacts/prfaq-dodo.md` |
| `bmad-prd` | `PRD.md` + `addendum.md` + `decision-log.md` |
| `bmad-ux` | `DESIGN.md` + `EXPERIENCE.md` |
| `bmad-architecture` | `ARCHITECTURE-SPINE.md` |
| `bmad-create-epics-and-stories` | `epics/E*.md` + `stories/e*.md` |
| `bmad-check-implementation-readiness` | PASS/CONCERNS/FAIL |
| `bmad-sprint-planning` | `sprint-status.yaml` |
| `bmad-create-story` | `stories/{key}.md` |
| `bmad-dev-story` | working code + tests |
| `bmad-code-review` | `review/{key}-code-review.md` |
| `bmad-validate` | `validation/{key}-validation-report.md` |
| `bmad-correct-course` | `course-corrections/sprint-change-proposal-{date}.md` |
| `bmad-sprint-status` | updates `sprint-status.yaml` |
| `bmad-retrospective` | `retrospectives/retrospective-epic-{num}-{date}.md` |

## Tier 3 — dodo's project-specific skills

These are dodo's real skills, at `.claude/skills/<name>/SKILL.md` — not slash commands, but
skills loaded automatically when their trigger fires (see `CLAUDE.md`'s skills table):

| Skill | Load it when |
|---|---|
| `gpui-component-recipes` | Writing/editing any `render`/`new` building a gpui-component widget; a widget call doesn't compile; a widget builds but nothing appears |
| `dodo-tool-view` | Adding, renaming, reordering, or removing a sidebar tool |
| `dodo-i18n-text` | Writing or changing any text a user reads |
| `dodo-theming-settings` | Adding/changing a setting, theme, or language |
| `dodo-build-validate` | First `cargo` invocation of a session, or verifying a UI change actually works |

There is no `/dodo-run-story`, `/dodo-standup`, or similar slash-command automation installed in
this repo — an implementing agent follows `agent-execution-loop.md` manually.

## Tier 4 — TEA (Test Engineering Architect) module

Actually installed per `_bmad/_config/manifest.yaml` (v1.19.1); whether it's been exercised
against dodo specifically is not recorded anywhere in this repo — treat it as available tooling,
not as evidence any of its workflows have run here.

| Workflow | Purpose |
|---|---|
| `teach-me-testing` | Progressive testing education |
| `test-design` | Risk-based test planning |
| `framework` | Scaffold a test framework |
| `ci` | Multi-stage CI/CD quality pipeline |
| `atdd` | Acceptance-TDD tests |
| `automate` | Expand automation coverage |
| `test-review` | Final test quality audit |
| `nfr-assess` | NFR evidence audit |
| `trace` | Coverage traceability |
