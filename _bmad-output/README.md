# BMAD Document Tree — dodo

**Generated from:** `CLAUDE.md` and the `dodo` source tree (this repository), replacing a prior
generation that documented an unrelated Python project ("Cowork Local v3.0" / CoworkAthon) — see
`CHANGELOG.md` for that history and why it was replaced.
**Folder name:** `_bmad-output/` (BMAD canonical name, used as-is — no rename)
**Methodology:** BMAD-METHOD (4-phase spec-driven development)
**Manifest:** see `bmad.config.yaml` at project root for the canonical artifact → file map

This folder is a **secondary, generated view** of `dodo`. `CLAUDE.md` at the project root remains
the authoritative, hand-maintained source of truth for how the codebase actually works; these
planning artifacts summarize and cross-reference it in BMAD's standard shape, and must be updated
whenever they drift from it rather than the other way around.

---

## TL;DR — 30-second orientation

| Question | Answer |
|---|---|
| What is this folder? | A BMAD-compliant artifact repository describing `dodo`, generated for planning/onboarding use |
| Where do I start as a new agent? | `../CLAUDE.md` first, then `implementation-artifacts/AGENT_BRIEF.md` here |
| What is the source of truth for how the code works? | `../CLAUDE.md` and the module `mod.rs` doc comments it points to — not this folder |
| What is this folder the source of truth for? | The planning shape — epics, stories, sprint status, decisions log |
| Where does BMAD's own runtime config live? | `_bmad/` at project root (next to this folder) |

---

## Layout

```
project-root/
├── _bmad-output/                              ← THIS FOLDER (spec-driven artifacts)
│   ├── bmad.config.yaml (mirror at root)       manifest — artifact → file map
│   ├── README.md                               you are here
│   ├── CHANGELOG.md                            changes to this folder over time
│   ├── .bmad-gitignore                         patterns to gitignore
│   │
│   ├── planning-artifacts/                     PHASE 1-3 — what dodo is & why
│   │   ├── product-brief.md                    problem, vision, scope
│   │   ├── prfaq-dodo.md                       narrative framing (no competition/launch context)
│   │   ├── PRD.md                              functional + non-functional requirements
│   │   ├── addendum.md                         updates to PRD since first pass
│   │   ├── decision-log.md                     product/scope decisions
│   │   ├── validation-report.md                PASS/CONCERNS/FAIL gate
│   │   ├── DESIGN.md                           UX visual spine
│   │   ├── EXPERIENCE.md                       UX behavioral spine
│   │   ├── ARCHITECTURE-SPINE.md               system context + component map
│   │   ├── architecture-decisions.md           ADR index (ADR-001..)
│   │   ├── epics/                              one file per epic (E0..E9)
│   │   ├── dependency-graph.md                 epic/story dependency graph
│   │   ├── implementation-readiness-check.md   PASS/CONCERNS/FAIL gate
│   │   ├── project-context.md                  rules a contributor/agent must follow
│   │   ├── glossary.md                         domain terms
│   │   └── references.md                       sources
│   │
│   └── implementation-artifacts/               PHASE 4 — how it was/ is being built
│       ├── AGENT_BRIEF.md                      entry point for an implementing agent
│       ├── sprint-status.yaml                  machine-readable per-story tracking
│       ├── agent-execution-loop.md             per-story BMAD cycle
│       ├── execution-order.md                  critical path
│       ├── blocked-decision-tree.md            when stuck
│       ├── completion-criteria.md              "done" definition
│       ├── 5-second-prompt.md                  one-liner for the agent
│       ├── success-vision.md                   what success looks like
│       ├── execution-practice.md               practical guide
│       ├── cut-first-order.md                  what to drop under slip
│       ├── risk-register.md                    risks and mitigations
│       ├── v31-roadmap.md                      forward roadmap
│       ├── env-vars-and-config.md              env-vars this app actually reads
│       ├── open-questions.md                   unresolved questions (e.g. GPL distribution)
│       ├── architecture-constraints.md         per-commit rules (fmt/clippy/--locked/i18n guard)
│       │
│       ├── stories/                            per-story dossiers, one per epic story
│       ├── review/                              code review drops (per story)
│       ├── validation/                          validation reports (per story)
│       ├── retrospectives/                      one per epic
│       └── course-corrections/                  sprint-change-proposal-*.md
│
├── _bmad/                                      BMAD installer-owned runtime config
├── docs/                                       long-lived project knowledge (build-optimization.md, release.md)
└── bmad.config.yaml                            top-level manifest of _bmad-output/ contents
```

---

## What changed in the dodo regeneration

The folder previously held ~400 files describing "Cowork Local v3.0", a Python AI-coworker app with
no relationship to this repository (confirmed: zero references to it anywhere in `src/`, and the
folder itself is untracked in git). That content has been replaced:

| Change | What / why |
|---|---|
| **Planning artifacts rewritten** | product-brief, PRD, ARCHITECTURE-SPINE, architecture-decisions, glossary, dependency-graph, project-context, references, DESIGN, EXPERIENCE, decision-log, validation-report, implementation-readiness-check, addendum, prfaq — all now describe dodo's actual product and architecture, grounded in `CLAUDE.md` and the source tree. |
| **Epics replaced** | 46 Cowork epics → 10 epics (E0-E9) matching dodo's real modules: core shell, JSON Formatter, Encoder/Decoder, API Explorer, Docker, Database Explorer, Updater, Theming/Settings/i18n, Persistence, Build & Release/Licensing. |
| **Stories right-sized** | ~400 Cowork story files → 39 stories matching what dodo actually has (35 marked `done`, retrospectively, since dodo already ships most of this) plus 4 forward-looking stories for real, documented gaps (Docker's disabled Exec/Create/Stats/Favorites controls, the Database Explorer's stated cuts, API Explorer's OAuth2 placeholder, the open GPL-3.0 distribution question). |
| **Hackathon/dual-IDE artifacts dropped** | `fci-judging-prep.md`, `demo-slot-structure.md`, `legacy-task-mapping.md`, `dual-ide-folder-layout.md`, `plan-review-log.md` — these described a hackathon submission and a Claude+Cursor dual-IDE monorepo layout that dodo has neither of. See `CHANGELOG.md`. |
| **Fake SBOM removed** | `security/sbom-cyclonedx.json` and `security/sbom-spdx.json` listed Python/npm packages (Authlib, VitePress, Django...) that don't exist in this Rust codebase; `security/vuln-deny-list.txt` referenced `pip-compile`/`npm audit`. dodo's real license/dependency posture lives in `deny.toml` and `THIRD-PARTY-NOTICES.md` at the repo root; `security/README.md` now points there instead of duplicating fake data. |
| **Config layer fixed** | `bmad.config.yaml` claimed the folder had been renamed to `bmad/`; the folder on disk is `_bmad-output/`. Paths now match reality. |

---

## Reading order for an implementing agent

1. `../CLAUDE.md` — the actual authority on how the code works
2. `implementation-artifacts/AGENT_BRIEF.md` — entry point into this folder
3. `planning-artifacts/project-context.md` — rules to follow
4. `planning-artifacts/glossary.md` — terms
5. `implementation-artifacts/sprint-status.yaml` — current state
6. Pick a story: `implementation-artifacts/stories/e*.md`
7. If blocked: `implementation-artifacts/blocked-decision-tree.md`

The full context pyramid per story:

```
PRD.md (why)
  + ARCHITECTURE-SPINE.md (how the system fits together)
  + architecture-decisions.md (which decisions are settled)
  + epics/E*.md (which epic owns this)
  + project-context.md (rules it must follow)
  + architecture-constraints.md (per-commit rules)
  + stories/e*.md (the task at hand)
```

---

## BMAD workflow ↔ file mapping

| Workflow | Writes to |
|---|---|
| `bmad-product-brief` | `planning-artifacts/product-brief.md` |
| `bmad-prfaq` | `planning-artifacts/prfaq-dodo.md` |
| `bmad-prd` | `planning-artifacts/PRD.md` + `addendum.md` + `decision-log.md` |
| `bmad-validate` (PRD) | `planning-artifacts/validation-report.md` |
| `bmad-ux` | `planning-artifacts/DESIGN.md` + `EXPERIENCE.md` |
| `bmad-architecture` | `planning-artifacts/ARCHITECTURE-SPINE.md` |
| `bmad-create-epics-and-stories` | `planning-artifacts/epics/E*.md` + `implementation-artifacts/stories/e*.md` |
| `bmad-check-implementation-readiness` | `planning-artifacts/implementation-readiness-check.md` |
| `bmad-sprint-planning` | `implementation-artifacts/sprint-status.yaml` |
| `bmad-create-story` | `implementation-artifacts/stories/{key}.md` |
| `bmad-dev-story` | working code + tests |
| `bmad-code-review` | `implementation-artifacts/review/{key}-code-review.md` + `review-status.yaml` |
| `bmad-validate` (story) | `implementation-artifacts/validation/{key}-validation-report.md` + `validation-status.yaml` |
| `bmad-correct-course` | `implementation-artifacts/course-corrections/sprint-change-proposal-{date}.md` |
| `bmad-retrospective` | `implementation-artifacts/retrospectives/retrospective-epic-{num}-{date}.md` |
| `bmad-sprint-status` | updates `implementation-artifacts/sprint-status.yaml` |

---

## Verification

```bash
# count files
find _bmad-output/ _bmad/ docs/ -type f | wc -l

# verify story-key consistency
diff <(ls _bmad-output/implementation-artifacts/stories/ | sed 's/-.*$//' | sort -u) \
     <(grep -oP '\bE[0-9]+\.[0-9]+' _bmad-output/implementation-artifacts/sprint-status.yaml | tr 'E' 'e' | sort -u)
```

Expected: story-key diff is empty.
