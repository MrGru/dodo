# References — Sources This Folder Is Built On

**Source:** `CLAUDE.md`
**Stage:** Phase 3 (Solutioning)

---

## Source materials this folder is built on

- `CLAUDE.md` at the project root — the authoritative, hand-maintained source of truth. Everything
  in `_bmad-output/planning-artifacts/` and `_bmad-output/implementation-artifacts/` is a summary
  of it, not a replacement for it.
- `README.md` — the user-facing description of dodo (note: as of this writing it understates what
  API Explorer and Docker actually ship; `CLAUDE.md` and the source are more current — see the
  ground-truth status table in `../../implementation-artifacts/sprint-status.yaml`).
- `docs/build-optimization.md` — release profile, size measurements, linker findings, the
  dependency report, startup review.
- `docs/release.md` — CI, the release workflow, packaging, verification, the application icon, the
  in-app updater, future signing/notarisation.
- Each module's own `mod.rs` doc comments (`src/api_explorer/mod.rs`, `src/docker/mod.rs`,
  `src/database/mod.rs`, `src/updater/mod.rs`) — the authority on that module's structure and cuts.
- `Cargo.toml`, `Cargo.lock`, `deny.toml`, `THIRD-PARTY-NOTICES.md`, `LICENSE` — dependency sourcing,
  license posture, and the open GPL-3.0 distribution question.
- `.claude/skills/*/SKILL.md` — detailed, verified knowledge loaded at the moment of need
  (`gpui-component-recipes`, `dodo-tool-view`, `dodo-i18n-text`, `dodo-theming-settings`,
  `dodo-build-validate`).

## Artifacts produced by this folder

| Artifact | Location | Read by an implementing agent? |
|---|---|---|
| Product Brief | `planning-artifacts/product-brief.md` | Skim — narrative only |
| PRD | `planning-artifacts/PRD.md` | Yes — FR/NFR table and Definition of Done |
| Architecture Spine | `planning-artifacts/ARCHITECTURE-SPINE.md` | Skim — component map |
| Architecture Decisions | `planning-artifacts/architecture-decisions.md` | Yes — canonical ADRs |
| Epics & Stories | `planning-artifacts/epics/E*.md` + `implementation-artifacts/stories/e*.md` | Yes |
| Sprint status | `implementation-artifacts/sprint-status.yaml` | Yes — current state |
| Agent Brief | `implementation-artifacts/AGENT_BRIEF.md` | Yes — load first, after `CLAUDE.md` |

The agent does not need to read every file in this folder top to bottom. It needs: `CLAUDE.md` +
`AGENT_BRIEF.md` + the next story's dossier + the relevant skill.
