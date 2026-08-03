# Changelog — `_bmad-output/` folder

All notable changes to this BMAD artifact folder are documented here.

## [2026-08-03] — Regenerated to describe dodo

### Context

This folder was found to contain ~400 files describing "Cowork Local v3.0" (CoworkAthon), a
Python AI-coworker desktop app — a project with no relationship to `dodo` (the Rust/GPUI app
actually in this repository). Confirmed via `grep -ril cowork src/` (zero hits) and
`git status`/`git log -- _bmad _bmad-output` (both directories untracked, no history here). It
had evidently been generated for a different project and copied into this repo by mistake.

A second, distinct mismatch was found in `../_bmad/` (the BMAD tool installation itself, one level
up from this folder): its installer-managed config (`config.toml`, `core/config.yaml`,
`bmm/config.yaml`, `tea/config.yaml`) named a **third**, different project —
`project_name = "PB-02 Medicare Clinic Appointment Booking App"`, `output_folder =
"_bmad-output-pb02"` — matching neither Cowork Local nor dodo. All four files were corrected to
`project_name = "dodo"` / `output_folder = "_bmad-output"` so the tool's own config, this folder's
name, and the actual codebase all finally agree. `communication_language = "Vietnamese"` was left
unchanged as a plausible genuine user preference, not a project-identity artifact.

### Changed — everything below replaces content that described the unrelated project

- **`bmad.config.yaml`, `README.md`**: corrected to describe dodo, and to match the folder that is
  actually on disk (`_bmad-output/`) rather than a claimed `bmad/` rename that never happened here.
- **`planning-artifacts/`**: `product-brief.md`, `PRD.md`, `ARCHITECTURE-SPINE.md`,
  `architecture-decisions.md`, `glossary.md`, `dependency-graph.md`, `project-context.md`,
  `references.md`, `DESIGN.md`, `EXPERIENCE.md`, `decision-log.md`, `validation-report.md`,
  `implementation-readiness-check.md`, `addendum.md` rewritten from `CLAUDE.md` and a direct
  source audit. `prfaq-coworkathon.md` replaced by `prfaq-dodo.md` (a hackathon PRFAQ has no
  equivalent audience or launch context in dodo).
- **`planning-artifacts/epics/`**: 46 Cowork epics replaced by 10 (E0-E9) matching dodo's real
  modules (core shell, JSON Formatter, Encoder/Decoder, API Explorer, Docker, Database Explorer,
  Updater, Theming/Settings/i18n, Persistence, Build & Release/Licensing).
- **`implementation-artifacts/`**: `AGENT_BRIEF.md`, `agent-execution-loop.md`, `execution-order.md`,
  `blocked-decision-tree.md`, `completion-criteria.md`, `5-second-prompt.md`, `success-vision.md`,
  `execution-practice.md`, `cut-first-order.md`, `risk-register.md`, `v31-roadmap.md`,
  `env-vars-and-config.md`, `open-questions.md`, `architecture-constraints.md`, and
  `sprint-status.yaml` rewritten to reflect dodo's actual, audited implementation status rather
  than an 11-day hackathon schedule for a two-developer team.
- **`implementation-artifacts/stories/`**: ~400 Cowork story files replaced by 39, matching what
  dodo actually has — 35 already `done` (retrospective dossiers for shipped work, grounded in a
  direct source audit, not assumed from README.md), 3 `placeholder-by-design` (E3.8 OAuth2 auth
  type, E4.6 Docker's remaining controls, E5.7 Database Explorer's remaining cuts), and 1
  `open-question` (E9.4, the GPL-3.0 distribution decision — not an engineering task).
- **`bmb-workflows/bmad-dev-story/{workflow.md,workflow.yaml}`**: rewritten from a Python-scripted,
  two-developer automation pipeline (a selector script, a 13-item PySide6-specific grep gate, a
  Cursor slash command — none of which exist in dodo) to the loop that actually applies: a single
  agent following `agent-execution-loop.md`, gated by `cargo fmt`/`clippy`/`test` and the
  `src/database/` self-containment check.
- **`security/`**: removed `sbom-cyclonedx.json` and `sbom-spdx.json` (fake data — they listed
  Python/npm packages like `Authlib` and `VitePress` that are not dependencies of this Rust
  codebase) and `vuln-deny-list.txt` (referenced `pip-compile`/`npm audit`, which dodo doesn't use).
  Replaced with `security/README.md` pointing to dodo's real license/dependency posture
  (`deny.toml`, `THIRD-PARTY-NOTICES.md`) rather than inventing fake SBOM data for tooling dodo
  doesn't run.
- **Dropped outright** (hackathon- or dual-IDE-monorepo-specific, with no dodo equivalent):
  `implementation-artifacts/fci-judging-prep.md`, `demo-slot-structure.md`,
  `legacy-task-mapping.md` (mapped old task IDs from a `cowork.plan.md` dodo never had),
  `dual-ide-folder-layout.md` (a Claude+Cursor monorepo layout dodo doesn't use),
  `implementation-artifacts/tests/{e2e-run-report-2026-07-15.md,test-summary.md}`,
  `implementation-artifacts/validation/done-stories-retest-2026-07-13.md`, and
  `implementation-artifacts/course-corrections/{E2.1-PRECOMMIT-FALSEPOS-2026-07-11.md,
  env-blocker-2026-07-11.md}` — all point-in-time logs (test runs, retests, escalations) from the
  unrelated project's own history, found in a second pass after the first `grep -ril cowork`
  verification turned them up.
- **`implementation-artifacts/installed-modules.md`**: its Tier 3 ("project-specific skills")
  described eight fictional `cowork-*` slash commands; replaced with dodo's actual five
  `.claude/skills/` entries. Tiers 1/2/4 (generic BMAD core/workflow/TEA tooling) were accurate and
  kept.
- **`.bmad-gitignore`**, **`implementation-artifacts/review/README.md`**: path references corrected
  from `bmad/...` to `_bmad-output/...` to match the actual folder name.
- **`../bmad.config.yaml`** (project root): created — this file's own header and `README.md` both
  described a root-level mirror of `_bmad-output/bmad.config.yaml` "for easy discovery," but no
  such file actually existed anywhere in the repository even before this regeneration. It now does.

### Notes

- This regeneration was a deliberate choice among three options presented to the user (leave
  as-is, delete outright, or regenerate) — regeneration was chosen explicitly.
- Story/epic *count* was right-sized to dodo's actual scope rather than mechanically preserved at
  the original's volume — inventing ~360 additional stories to hit the old file count would have
  meant describing work dodo never had.
- `CLAUDE.md` at the project root remains the authority. This folder is a derived, secondary view;
  update it to match `CLAUDE.md` whenever the two drift, never the reverse.
- A pre-existing, unrelated documentation gap was found during this work and is flagged (not
  silently fixed) in `validation-report.md` and `open-questions.md`: dodo's top-level `README.md`
  understates current functionality (it describes several already-shipped API Explorer and Docker
  features as "arriving later").
