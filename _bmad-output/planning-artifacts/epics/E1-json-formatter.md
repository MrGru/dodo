# Epic E1 — JSON Formatter

**Source:** `src/json_formatter.rs`
**Stage:** Phase 3 (Solutioning)
**Status:** Done

---

A single-file tool; a leaf with no dependents.

| Story | Title | Status | Depends on |
|---|---|---|---|
| **E1.1** | Pretty-print pasted JSON at a chosen indent width | Done | E0.1 |
| **E1.2** | Parse error shown inline as a diagnostic when input is invalid | Done | E1.1 |

**AC (E1.2):** An invalid paste never produces a blank result or a modal dialog — the error
appears inline, at the point it was detected.
