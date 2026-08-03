# Open Questions — Need a Human Decision

**Stage:** Phase 4 (Implementation)

---

1. **GPL-3.0-or-later distribution posture.** `gpui -> sum_tree -> ztracing -> zlog` pulls
   GPL-3.0-or-later into every build; dodo's own source is MIT. `THIRD-PARTY-NOTICES.md` records
   the chain and leaves the question of what that means for distributing a built binary explicitly
   open. Resolving this needs someone with the authority to make a licensing call, not an
   engineering fix — see E9.4 in `planning-artifacts/epics/E9-build-release-licensing.md`.
2. **Whether the Windows and `macos-x64` release paths have ever actually run on real hardware.**
   `docs/release.md`'s "What 'verified' means" states plainly that they haven't been confirmed on
   a real host; someone needs to either run the verification or accept the risk explicitly rather
   than let CI-green stand in for it indefinitely.
3. **Whether `README.md` should be updated to match current functionality.** A direct source audit
   (recorded in `sprint-status.yaml`) found it describes API Explorer's Auth/Scripts/Cookies/
   Tests/Console/collections and Docker's Images/Volumes/Networks as "arriving later" when all of
   these have already shipped. Fixing this is a small, low-risk task but is a documentation/product
   decision (how much detail a first-time reader should see), not something this regeneration
   silently corrected on the repository's behalf.
4. **Priority among the four forward stories (E3.8, E4.6, E5.7, E9.4).** They're mutually
   independent (see `execution-order.md`); nobody has stated which, if any, is actually wanted
   next.
