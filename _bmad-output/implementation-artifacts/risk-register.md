# Risk Register

**Stage:** Phase 4 (Implementation)

**Note:** Cross-cutting technical risks and their mitigations are the canonical version in
`PRD.md` §2.4 and `architecture-constraints.md`. This document restates them from an
operational "what to watch" angle rather than duplicating the mitigation detail.

---

| ID | Risk | Watch for | Mitigation owner (module) |
|---|---|---|---|
| **RISK-1** | `gpui`/`gpui-component` drift upstream unexpectedly | A `cargo update` run outside its own reviewed commit | Whoever reviews dependency changes |
| **RISK-2** | Windows/`macos-x64` release path silently assumed verified | A change to `updater/services/installers/` or `.github/workflows/release.yml` without a stated manual-verification note | `docs/release.md` maintainer |
| **RISK-3** | A future dependency change reintroduces the `rusqlite`/`sqlx` conflict | Adding any new SQL crate to `Cargo.toml` | Whoever proposes the dependency |
| **RISK-4** | GPL-3.0-or-later distribution question gets answered by omission | Any edit to `deny.toml`'s `allow`/`exceptions` or to `THIRD-PARTY-NOTICES.md`'s "open question" framing | Whoever has the authority to actually decide E9.4 |
| **RISK-5** | A script sandbox escape or hang | Any change to `services/script/quickjs.rs`'s intrinsic allowlist or its 2 s/16 MiB/256 KiB caps | API Explorer maintainer |
| **RISK-6** | A stored credential becomes more exposed than documented | Any change to `database/models/connection.rs` or the API Explorer's secret-variable masking that doesn't update the UI notice alongside it | Database Explorer / API Explorer maintainer |
| **RISK-7** | A "coming soon" placeholder gets silently half-built | A PR that partially implements E4.6/E5.7/E3.8 without updating the owning module's `mod.rs` doc and this folder's `sprint-status.yaml` together | Whoever picks up the story |
| **RISK-8** | `PageBuffer`'s no-`LIMIT`-injection guarantee regresses | Any change to `database/models/page.rs` or a driver's query execution path | Database Explorer maintainer |
| **RISK-9** | `README.md` drifts further from actual shipped functionality | Any new feature landing without a corresponding `README.md` update | Whoever ships the feature (currently an existing, pre-regeneration gap — see `validation-report.md`) |

## What this folder does not track

Deadline risk, venue/network risk, and demo-rehearsal risk from the predecessor document do not
apply — dodo has no fixed release date, no live demo event, and no judging panel.
