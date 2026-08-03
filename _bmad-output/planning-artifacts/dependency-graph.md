# Dependency Graph — Epic & Story Relationships

**Source:** `CLAUDE.md`, module `mod.rs` files
**Stage:** Phase 3 (Solutioning)

---

## Architectural dependency graph (E0-E9)

Unlike a sprint plan racing a single deadline, dodo's epics are mostly already shipped — this graph
describes *architectural* dependency (what a change to one epic can break), not a build-order
schedule:

```
E0 (core shell: window, sidebar, Layout, View enum, QuitMode)
  │
  ├──► E1 (JSON Formatter)              — leaf tool, no dependents
  ├──► E2 (Encoder/Decoder)             — leaf tool, no dependents
  ├──► E3 (API Explorer)  ──► reads E7 (i18n/Str), E8 (collections.json, environments.json,
  │                                       script-consent.json persistence)
  ├──► E4 (Docker)        ──► reads E7 (i18n/Str)
  ├──► E5 (Database Explorer) ──► reads E7 (i18n/Str), E8 (connections.json persistence)
  │
E6 (Updater)     ──► reads E8 (updater.json persistence), independent of E3/E4/E5
E7 (Theming/Settings/i18n) ──► cross-cutting: every view in E1-E6 depends on it
E8 (Persistence/data_dir)  ──► cross-cutting: E3, E5, E6 each own one persisted file
E9 (Build/Release/Licensing) ──► depends on E0-E8 existing (packages and releases the whole app);
                                   owns the update.json manifest E6 consumes and the icon E0's
                                   window/E9's packaging both need
```

**Hard edges:**

- **E0 → everything** — no tool renders without the shell and the `View` enum entry.
- **E7 → every view in E1-E6** — a view that draws a bare string literal fails the i18n guard test.
- **E8 → E3/E5/E6** — API Explorer's collections/environments/consent, Database's connections
  (including stored passwords), and the updater's config all depend on the `data_dir()` +
  versioned-store pattern existing first.
- **E9 → E0-E8** — a release packages the whole app; the update manifest (E9) is what E6's pipeline
  reads.

**Soft edges (same-file sequencing within a module, to avoid merge conflicts on future work):**

- Within E3: `services/send.rs` (the pipeline) and `services/script/quickjs.rs` (the sandbox) are
  both touched by any change to hook ordering.
- Within E5: `state/tree.rs` (`Forest`) and `services/mod.rs` (`Driver`) are both touched by adding
  a node kind or a capability.

## Why there is no sprint-cadence DAG

The predecessor document this replaced modeled a day-by-day, two-developer build schedule toward a
hackathon deadline. dodo has no such deadline and, per `git log`, has already shipped most of E0-E9
across several rounds; `implementation-artifacts/sprint-status.yaml` tracks story status
(done/planned/placeholder), not a critical-path countdown. Future stories (the OAuth2 auth type,
any of Docker's or the Database Explorer's stated cuts, the GPL-3.0 distribution decision) are
independent of each other and can land in any order — none blocks another.
