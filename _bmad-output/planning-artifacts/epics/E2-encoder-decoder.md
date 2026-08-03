# Epic E2 — Encoder / Decoder

**Source:** `src/encoder_decoder.rs`
**Stage:** Phase 3 (Solutioning)
**Status:** Done

---

A single-file tool; a leaf with no dependents.

| Story | Title | Status | Depends on |
|---|---|---|---|
| **E2.1** | Base64 (standard + URL-safe), URL percent-encoding, Hex — both directions | Done | E0.1 |
| **E2.2** | JWT inspector: splits a token into header/payload/signature | Done | E2.1 |

**AC (E2.2):** JWT inspection is decode-only — no signature verification is performed or implied
by the UI.
