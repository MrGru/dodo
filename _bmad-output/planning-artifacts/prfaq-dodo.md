# PRFAQ — dodo

**Source:** `product-brief.md`, `CLAUDE.md`
**Audience:** Anyone orienting to dodo for the first time

> The predecessor to this file (`prfaq-coworkathon.md`) was written for a hackathon submission
> ("Tích Hợp FCI Service" judging criteria, a Demo Day, top-5 finalists). None of that applies to
> dodo, which has no competition, no judging panel, and no deadline. This file keeps the PRFAQ
> shape (a narrative framing, useful for a first-time reader) without inventing a launch context
> dodo doesn't have.

---

## The "press release," if dodo needed one

dodo is a single native desktop app — no Electron, no web-view, no account — that puts a handful of
the small tools a developer reaches for constantly (JSON formatting, encode/decode, an HTTP
client, a Docker/Podman manager, a database explorer) behind one collapsible sidebar. Nothing
leaves the machine except what the tool itself is talking to: the API under test, the local Docker
socket, the target database.

## Frequently asked questions

**Is dodo trying to replace Postman / DBeaver / Docker Desktop?**
No — it covers the slice of each that a developer reaches for most, in one lightweight binary,
not every feature of any of them. See `product-brief.md` §1.6 for what's deliberately not built.

**Why GPUI instead of Electron/Tauri/a web stack?**
Native GPU rendering, no web-view runtime, and `gpui-component` supplies a ready widget set. The
tradeoff: both are pulled from git with no released version, pinned only by `Cargo.lock`
(`architecture-decisions.md` ADR-001).

**Is my data safe?** Yes in the sense that nothing is sent anywhere dodo doesn't already need to
talk to for the feature itself — but a stored database password or a secret request variable is
kept in **plain text** under `data_dir()`, not an OS keychain, and the UI says so every time
(`architecture-decisions.md` ADR-007). This is a stated tradeoff, not an oversight.

**Can I run scripts against my API requests?** Yes — pre-request and post-response scripts run in
a sandboxed QuickJS engine with a positive intrinsic allowlist, a 2 s deadline, and 16 MiB/256 KiB
caps. An imported script's hooks are gated by a consent dialog before they run automatically.

**What's the license?** dodo's own code is MIT. The binary also contains GPL-3.0-or-later code
reached through `gpui`; what that means for distributing a built binary is an **open question**,
recorded rather than silently decided (`THIRD-PARTY-NOTICES.md`).

**What's explicitly not coming soon?** Anything listed as a disabled "coming soon" control
(Docker's Exec/Terminal/Create/Pull/Build/deeper Stats/Favorites; the Database Explorer's
editing/CRUD/autocomplete/second-backend set; API Explorer's OAuth2 auth type) is a deliberate,
recorded cut, not a backlog item with a due date.

**Where do I go to actually use it?** `cargo run` from the repo root; see `README.md`'s "Build and
run" section.
