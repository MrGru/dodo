# Security & Licensing — Where dodo's Real Data Lives

This directory previously held a fake CycloneDX/SPDX SBOM and a vulnerability deny-list for an
unrelated Python project (`Authlib`, `PyYAML`, `Pillow`, `VitePress`/`vue`, `pip-audit`, `npm
audit` — none of which are dependencies of this Rust/Cargo codebase). Those files have been
removed rather than translated, because dodo has no equivalent SBOM-generation tooling today —
inventing fake data for a process dodo doesn't run would be worse than having none.

## Where the real information actually lives

- **License and dependency-graph policy**: `../../deny.toml` (repo root) — `cargo deny` runs
  against it; it deliberately carries no `allow`/`exceptions` entry for the GPL-3.0-or-later chain
  reached through `gpui` (see `THIRD-PARTY-NOTICES.md`), so that chain keeps being reported rather
  than silently accepted.
- **Third-party license notices and the open distribution question**:
  `../../THIRD-PARTY-NOTICES.md` (repo root) — records the verified `gpui -> sum_tree -> ztracing
  -> zlog` GPL-3.0-or-later chain and states plainly that what it means for distributing a built
  binary is an open question, not yet decided (see `planning-artifacts/epics/E9-build-release-licensing.md`,
  story E9.4).
- **Dependency sourcing**: `../../Cargo.toml` / `../../Cargo.lock` — `Cargo.lock` is the only pin
  on the four git dependencies (`gpui`, `gpui_platform`, `gpui-component`, and a fourth); see
  `architecture-decisions.md` ADR-001.
- **A software bill of materials, if one is ever wanted**: dodo currently generates none. A future
  `cargo-cyclonedx`-style SBOM step would be a new, real story, not a file added here manually.
