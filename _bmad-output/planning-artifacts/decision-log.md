# Decision Log

**Source:** BMAD canonical `decision-log.md` artifact for `bmad-prd`
**Stage:** Phase 2 (Planning)

## Purpose

Chronological log of every product/scope decision made about this `_bmad-output/` folder itself
and, where recorded in `CLAUDE.md`, about dodo's product scope. Each entry captures context,
options considered, decision, consequences, and where it's recorded in the codebase.

## Entries

### 2026-08-03 | `_bmad-output/` regenerated to describe dodo instead of an unrelated project
- **Context**: This folder held ~400 files describing "Cowork Local v3.0" (CoworkAthon), a Python
  AI-coworker app with zero relationship to this repository — confirmed via `grep -ril cowork
  src/` (no hits) and `git log -- _bmad _bmad-output` (untracked, no history here). The folder had
  clearly been generated for a different project and copied in by mistake.
- **Options**: (a) leave it as-is since it's untracked and technically harmless, (b) delete it
  outright since dodo already has `CLAUDE.md` as its authority, (c) regenerate the planning
  artifacts to actually describe dodo, right-sized to dodo's real scope rather than mechanically
  preserving the original's file count.
- **Decision**: (c), per explicit user choice. Epics went from 46 (cowork) to 10 (E0-E9, matching
  dodo's real modules); stories from ~400 to ~42 (mostly retrospective "done" stories for shipped
  work, plus a handful of forward stories for real, documented gaps). Hackathon-specific and
  dual-IDE-monorepo-specific artifacts (`fci-judging-prep.md`, `demo-slot-structure.md`,
  `legacy-task-mapping.md`, `dual-ide-folder-layout.md`, `plan-review-log.md`) were dropped rather
  than translated, since dodo has no hackathon, no two-IDE layout, and no prior `cowork.plan.md` to
  review. Fake Python/npm SBOM files under `security/` were removed and replaced with a pointer to
  dodo's real `deny.toml`/`THIRD-PARTY-NOTICES.md`.
- **Consequences**: This folder is now a secondary, generated view of `CLAUDE.md` — it must be
  updated to match `CLAUDE.md` whenever the two drift, never the other way around.
- **Owner**: User-requested, executed via direct edit (not `bmad-dev-story`, since there is no
  BMAD dev-story automation wired into dodo's own build).
- **Cross-ref**: `../README.md`, `../CHANGELOG.md`.

### (inherited from `CLAUDE.md`) | The GPL-3.0-or-later distribution question stays open
- **Context**: `gpui -> sum_tree -> ztracing -> zlog` pulls GPL-3.0-or-later into every dodo build;
  dodo's own source is MIT.
- **Options**: (a) decide distribution posture now and document a conclusion, (b) leave the
  question open and keep `cargo deny` reporting it.
- **Decision**: (b) — `THIRD-PARTY-NOTICES.md` records the chain; `deny.toml` deliberately carries
  no `allow`/`exceptions` entry.
- **Consequences**: Nobody may write a settled conclusion into the repo without actually resolving
  the question first.
- **Owner**: Recorded in `CLAUDE.md`; no owner name given in the source material.

### (inherited from `CLAUDE.md`) | `Windows build (windows-x64)` and `macos-x64` stay experimental/non-blocking
- **Context**: `windows-x64`'s one real run failed on a `#[cfg(unix)]`-only bollard connector,
  fixed by the platform split in `docker/services/engine.rs`, but not yet confirmed green; the
  Windows release path has never run on a real Windows host at all.
- **Options**: (a) claim these rows verified because CI is green, (b) mark them experimental and
  state plainly what has and hasn't actually run.
- **Decision**: (b) — `docs/release.md`'s "What 'verified' means" is the honest record.
- **Consequences**: A future contributor cannot mistake "CI passed" for "this was run on the target
  platform."
- **Owner**: Recorded in `CLAUDE.md` and `docs/release.md`.
