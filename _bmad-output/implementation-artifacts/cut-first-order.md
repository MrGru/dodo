# Prioritization Order — What to Defer if Time Is Limited

**Stage:** Phase 4 (Implementation)

---

dodo has no submission deadline and no schedule-slip scenario, so there is no "cut order" in the
sense this document's predecessor meant (a hackathon dropping scope to hit a fixed date). What's
useful to carry forward is a priority order for anyone with limited time choosing what to work on
next, since dodo's forward stories are all independent and none is required:

| Priority | Work | Why it ranks here |
|---|---|---|
| **1 (never defer)** | A regression in already-shipped E0-E9 behavior | A broken existing feature actively harms users; nothing else here does |
| **2** | An `i18n`, `fmt`, or `clippy` failure introduced by any change | Blocking CI gates by design — never left red |
| **3** | E3.8 (OAuth2 auth type) | The single remaining placeholder in a heavily-used tool (API Explorer); highest user-visible value among the forward stories |
| **4** | E5.7 sub-items (Database Explorer cuts) | Real day-to-day friction (no autocomplete, no persisted history) for a tool used for real queries |
| **5** | E4.6 sub-items (Docker's remaining controls) | Lower frequency of need (Exec/Terminal, Create/Pull/Build) than E3.8/E5.7 |
| **6** | E9.4 (GPL-3.0 distribution decision) | Not a coding task — needs someone with the authority to actually decide it, not effort |

**Never silently drop:** the i18n guard tests, `cargo clippy -D warnings`, or the
`src/database/` self-containment check — these are structural invariants, not scope that can be
traded off under pressure.
