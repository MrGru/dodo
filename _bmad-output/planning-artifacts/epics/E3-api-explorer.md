# Epic E3 — API Explorer

**Source:** `src/api_explorer/` (`models/`, `services/`, `state/`, `components/`, `views/`)
**Stage:** Phase 3 (Solutioning)
**Status:** Done, one placeholder remaining (OAuth2)

---

The largest module in dodo. Depends on E0 (shell) and E8 (persistence — `collections.json`,
`environments.json`, `script-consent.json`). `api_explorer::init` registers this module's key
binding from `main`, after `gpui_component::init`.

| Story | Title | Status | Depends on |
|---|---|---|---|
| **E3.1** | Send pipeline: pre-request script → resolve `{{name}}` → `prepare` → `Transport::execute` → post-response script, one blocking function over trait objects | Done | E0.1 |
| **E3.2** | QuickJS script sandbox: positive intrinsic allowlist, 2 s / 16 MiB / 256 KiB caps, one fresh runtime per run | Done | E3.1 |
| **E3.3** | Script consent gating keyed by a hash covering both hooks together; provenance (`script_origin`) survives editing, content hash does not | Done | E3.2 |
| **E3.4** | Code generation (`curl`/`fetch`/`axios`/XHR) via a single normalized form (`services/codegen/normalize.rs`), with secret variables withheld | Done | E3.1 |
| **E3.5** | `curl` parsing and paste-to-rebuild in the URL box; round-trip property test against E3.4's generator | Done | E3.4 |
| **E3.6** | Saved, importable (Postman/Insomnia) request collections | Done | E0.1 |
| **E3.7** | Tab title derived from the URL's path, not host; `min_w_0` on request/response columns | Done | E3.1 |
| **E3.8** | OAuth 2.0 as a fifth auth type (Basic/Bearer/API Key already ship) | **Planned (placeholder)** | E3.1 |

**AC (E3.2):** A script cannot exceed its deadline or memory caps; `pm.sendRequest` is denied by
not existing, with an error message that names it; the `Eval` intrinsic is present (required for
`ctx.eval` itself, not an extra capability).

**AC (E3.3):** Approving one hook's script never silently approves the other hook's script for the
same request.

**AC (E3.8, not yet done):** `AuthType::OAuth2` currently renders `later_step()`
(`components/later_step.rs`) — the one remaining caller of that shared placeholder in the
codebase. A real implementation is a new story, not a fix to existing code.
