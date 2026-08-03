# Architecture Decisions — Cowork Local v3.0

**Source:** accumulated from `cowork.plan.md` §6 + ADR-001..ADR-013 referenced in the epic files; **REFRAMED 2026-07-11** to enumerate E15/E16 decisions in the same canonical style.

> **Format:** lightweight ADR (Context / Decision / Consequences). Each ADR has a stable ID so the executing agent can reference it without ambiguity. New ADRs append; existing ADRs are NEVER renumbered.

> **Two ADRs added in this v5 revision:**
> - **ADR-014** — Copilot is a Bridge (not a Provider) — original
> - **ADR-015** — FPT Cloud AI is a Provider (OpenAI-compat) — original
> - **ADR-016** — Claude Code is a Subagent Bridge — original
> - **ADR-017** — Dashboard data layer is local-only (same privacy model as Cowork Local proper) — original
> - **ADR-018** — Admin Board has NO network code — local-only team collaboration — original
> - **ADR-019** — Sandbox is ON by default — original
> - **ADR-020** — Secret-store is the single keyring gateway — original
> - **ADR-021** — Subprocess calls must go through CommandSpec — original

---

## ADR-001 — Per-Workspace state isolation

**Context.** Two concurrent turns in different Workspaces must not corrupt each other's checkpoint, audit log, or cost counter.

**Decision.** Every per-turn state lives in the turn's `ctx` dict, never a tab-level `self.` attribute.

**Consequences.** Two parallel conversations don't corrupt each other. NFR-9 verified by `tests/test_parallel_turns.py`.

---

## ADR-002 — Permission-gate parity across Cowork and Code

**Context.** E2's Trust & Control cluster must apply equally to both tabs.

**Decision.** Every mutating tool (`write_file`, `edit_file`, `run_command`, `install_package`, `remember`) routes through `PermissionGate.request` in BOTH Cowork and Code.

**Consequences.** Verified by `CoworkLocalallOS_3/tests/test_cowork_perm_gate.py` running 3 concurrent turns with mixed approve/reject.

---

## ADR-003 — PII redaction is always-on (opt-out at workspace level)

**Context.** PII leakage is the single highest-impact CoworkAthon-judging risk.

**Decision.** `core/pii.py::redact()` runs on every outbound user prompt, every tool-result append, and every disk write. Runs on by default; per-Workspace opt-out (never the other direction).

**Consequences.** Recall ≥ 99% on labelled 200-prompt set (NFR-7).

---

## ADR-004 — Audit log is JSONL append-only

**Context.** Audit data must be tamper-evident, atomic, and backpressure-aware.

**Decision.** `<AUDIT_DIR>/audit.jsonl` is append-only, one event per line, fsync per record, bounded in-memory deque + 5 s/100-event flush.

**Consequences.** NFR-6: `tests/test_audit_atomic.py`. No silent drops on crash.

---

## ADR-005 — Connector ABC generalizes notification channels

**Context.** FCI connector + Teams + Webhook all share a pattern (auth, query, format).

**Decision.** `core/connectors/base.py::Connector` ABC with `query()`, `send_notification()`, `configured()`. Implementations: `TeamsConnector`, `WebhookConnector`, `FciConnector`.

**Consequences.** New connectors ship as ~150 LoC of subclassing (vs ~600 LoC of new code).

---

## ADR-006 — `run_command` denylist is partial mitigation

**Context.** A real sandbox requires OS-level primitives not available in a portable Python app.

**Decision.** E2.4 ships the denylist (`rm -rf /`, `disk format`, `fork bomb`, ...) and documents it as a PARTIAL mitigation, NEVER as a sandbox. ADR-013 + ADR-019 (Sandbox ON default) supersede this for v5.

**Consequences.** ADR-013 + ADR-019 (Sandbox, E16.4) are the new source-of-truth.

---

## ADR-007 — FCI Service integration is Must-have (not "Won't this cycle")

**Context.** CoworkAthon judging criterion 3/5 requires FCI Service integration.

**Decision.** FCI cluster (E9.9–E9.15) is **never-cut** under any schedule-slip scenario.

**Consequences.** If FCI isn't wired by Day 11, the demo narrative becomes "Connector ABC + TeamsConnector + WebhookConnector shipped; FCI-specific connector is the next half-day integration". The connector ABC itself is the showcase.

---

## ADR-008 — Multi-tenant SaaS, mobile/web, voice/image/Design/Reflect/Chrome/Enterprise Search — Won't this cycle

**Context.** Total stories in scope can't exceed ~30 active + 3 stubs.

**Decision.** Out of scope for v3.0. Stable ADR-IDs reserved.

**Consequences.** No agent tries to add these mid-flight (audit grep for `# ADR-008` comments + `tests/test_banned_features.py`).

---

## ADR-013 — Sandbox is a partial mitigation only (v3.0); Sandbox v2 in E16.4 (v5)

**Context.** E16 (Security) raises the bar: ON-by-default Sandbox, `PathGuard`, `CommandSpec`.

**Decision.** E16.4 supersedes ADR-006. Sandbox ON by default; `COWORK_SANDBOX=off` is the explicit escape hatch (audited).

**Consequences.** Architectural constraint AC-12 added (see `architecture-constraints.md`).

---

## ADR-014 — GitHub Copilot is a Bridge (not a Provider) *(NEW v3)*

**Context.** GitHub Copilot has no public raw-inference API. It's a subagent framework.

**Decision.** E13 creates `integrations/github_copilot/bridge.py::CopilotBridge` (sibling of `Provider`, not subclass). Uses `@github/copilot-sdk` for the chat + `/models` catalog (JWT-auth).

**Consequences.** `provider-contract-tester` does NOT validate E13 (by design). Use `bmad-code-review` + `tests/test_copilot_integration.py` instead.

---

## ADR-015 — FPT Cloud AI is a Provider (OpenAI-compat) *(NEW v3)*

**Context.** FPT Cloud AI exposes an OpenAI-compatible inference endpoint at `https://mkp-api.fptcloud.com/v1`.

**Decision.** E12 adds `src/cowork_local/providers/fpt_cloud.py::FPTCloudProvider(Provider)` — OpenAI-compatible, Bearer-token auth, Vietnamese + Japanese data centres available.

**Consequences.** E12.1..E12.6 ship as concrete stories (not the single-line v3.1 stub from ADR-008).

---

## ADR-016 — Claude Code is a Subagent Bridge *(NEW v3)*

**Context.** `claude-code` is a subprocess CLI/SDK, not an inference endpoint.

**Decision.** E14 creates `integrations/claude_code/cli.py` + `permissions.py` bridging the `claude` subprocess into Cowork Local. NOT a Provider subclass.

**Consequences.** E14.5–E14.7 (Claude Code settings UI, test suite, docs) are PUSH-CUT-eligible under heavy Day-10 schedule slip.

---

## ADR-017 — Dashboard data layer is local-only *(NEW v4)*

**Context.** User requested "dashboard to monitoring all usage" — same privacy model as the rest of Cowork Local.

**Decision.** E15 Dashboard stores JSONL on local disk (`~/.cowork_local/dashboard/`). NO network egress. UI is PySide6 local.

**Consequences.** Architectural constraint AC-9 already prohibits external services in `core/`; E15 is fully consistent.

---

## ADR-018 — Admin Board has NO network code *(NEW v4)*

**Context.** "Team collaboration" can mean many things. We choose local-first.

**Decision.** E15.4 Admin Board: Team roster at `~/.cowork_local/team.json`. Workspace sharing = file-system-level export/import only. NO central server, NO cloud endpoint, NO third-party auth.

**Consequences.** Cross-team collaboration via shared filesystem or explicit workspace-export-zip. RBAC controls the export.

---

## ADR-019 — Sandbox is ON by default *(NEW v5 — Security epic)*

**Context.** Sandboxing was discussed as ADR-006 / ADR-013; E16 (Security) elevates it to a hard guarantee.

**Decision.** `core/sandbox.py::Sandbox.is_active = True` at app start. `Sandbox.disabled()` requires `COWORK_SANDBOX=off` + audit-event.

**Consequences.** AC-12 added; pre-commit hook `pre-commit-no-network` already covers the network-side of this. Filesystem-side covered by `pre-commit-command-spec-grep` (AC-11) and `core/security/path_guard.py` (AC-11, E16.3).

---

## ADR-020 — Secret-store is the single keyring gateway *(NEW v5 — Security epic)*

**Context.** Provider API keys must NEVER live in cleartext (env vars, dotfiles, audit log).

**Decision.** `core/secret_store.py::SecretStore` is the ONLY module allowed to import `keyring`. Any other `core/*` importing `keyring` is a violation (AC-10, pre-commit hook `pre-commit-secret-store-grep`).

**Consequences.** E16.2 migrates every provider integration (Anthropic, OpenAI-compat, FPT Cloud AI, GitHub Copilot, Claude Code) to `SecretStore.get(provider_id, "api_key" | "access_token" | "refresh_token")`.

---

## ADR-021 — Subprocess calls must go through CommandSpec *(NEW v5 — Security epic)*

**Context.** Bare `subprocess.Popen(...)` is unsafe; needs typed validation.

**Decision.** Every `core/*/subprocess` call must go through `core/security/command_spec.py::CommandSpec(...).validate()`. Bare subprocess calls are violations (AC-11, pre-commit hook `pre-commit-command-spec-grep`).

**Consequences.** Legacy `core/tools.py` is in the allowlist; re-validated in E16.6 by `tests/security/test_command_injection.py` with the 33 semgrep-derived vectors.

---

## ADR-022 — Security model (threat model + IR playbook) *(NEW v5 — E16.8)*

**Context.** E16.5 / E16.7 introduced supply-chain tooling and the loopback-only static server, but the unified threat model, the per-component STRIDE walk, and the incident-response playbook were undocumented.

**Decision.** Adopt the security model in [`docs/adr/0010-security-model.md`](../../../docs/adr/0010-security-model) (Context / Decision / Consequences; references OWASP Top-10 2021, MITRE ATT&CK Enterprise v15, NIST SP 800-53 r5). Wiki landing page lives at `wiki/security/index.md`; STRIDE walk at `wiki/security/threat-model.md`; 3-step playbook at `wiki/security/incident-response.md`.

**Consequences.** Any new architectural component must extend the STRIDE table; any new connector must be added to the OWASP coverage map. The file-path is `0010-security-model.md` per E16.8 AC-4; the internal ID is `ADR-022` to keep the existing sequence monotonic.

---

## Cross-reference

- `architecture-constraints.md` lists the 13 ACs referenced by these ADRs.
- `sprint-status.yaml` enumerates the 97 stories implementing these ADRs.
- `bmad-execution.plan.md` is the canonical execution guide.
