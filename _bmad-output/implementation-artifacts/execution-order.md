# Story Execution Order

**Stage:** Phase 4 (Implementation)

---

E0-E9 are already substantially shipped (see `sprint-status.yaml`), so there is no critical-path
schedule to walk — unlike the hackathon plan this folder used to describe, dodo has no deadline
and no two-developer stream split recorded anywhere in the repository.

## What's actually open, and in what order (if any)

The four forward stories are **mutually independent** — none blocks another, and none has an
unresolved dependency:

| Story | What it is | Any ordering constraint? |
|---|---|---|
| **E3.8** | API Explorer's OAuth2 auth type | None — self-contained inside `api_explorer/services/http/auth.rs` and `views/request_auth.rs` |
| **E4.6** | Docker's Exec/Terminal/Create/Pull/Build/Stats/Favorites | None — each control is independent; could be split into up to 4 separate stories if picked up |
| **E5.7** | Database Explorer's editing/CRUD/autocomplete/second-backend/column-sorting set | None between its sub-items, though a second backend (e.g. MySQL) should land after any schema changes to the `Driver` trait that a CRUD feature might need |
| **E9.4** | The GPL-3.0 distribution question | Not a coding story — a product/legal decision; blocks nothing else in the meantime |

There is no "pick E3.8 before E4.6" rule. Pick whichever is most valuable to whoever is asking for
it next.

## If working on a regression instead

Any defect against an already-`done` epic takes priority over a forward story — a broken existing
feature is worse than a missing placeholder becoming real one release later.
