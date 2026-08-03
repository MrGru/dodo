# Success Vision — What Success Looks Like

**Stage:** Phase 4 (Implementation)

---

dodo has no single ship date, so "success" is not a moment — it's an ongoing property:

1. **Any developer can `cargo run` and use whichever tool they came for** without hitting a
   surprise — a "coming soon" control stays honestly labeled rather than silently broken, and a
   shipped feature (API Explorer's scripting, Docker's four pages, the Database Explorer's query
   flow) keeps working the way `CLAUDE.md` says it does.
2. **Every commit stays green** — `cargo fmt`, `cargo clippy -D warnings`, `cargo test`, the i18n
   guards, and `src/database/`'s self-containment check all pass, so nobody has to "fix CI later."
3. **A forward story (E3.8 OAuth2, E4.6 Docker's remaining controls, E5.7 Database Explorer's
   remaining cuts) landing is a bonus, not a requirement** — each is independent and can wait
   indefinitely without degrading anything already shipped.
4. **The open question (E9.4, GPL-3.0 distribution) gets resolved deliberately, whenever someone
   with the authority to decide it does so** — not by silent omission and not by a story that
   happens to touch `deny.toml` in passing.

There is no "3.1 backlog" or "post-competition roadmap" in the sense the predecessor of this
document meant — see `v31-roadmap.md` (kept under its original filename per `bmad.config.yaml`, its
content is dodo's actual forward-looking roadmap) for what's genuinely next.
