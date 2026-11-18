---
cairn: log
change: pimdir-draft-14
landed: 2026-11-18
---

# pimdir draft-14, through io-pimdir 0.17

io-pimdir 0.17.0, pimalaya-config 0.6.0 and pimalaya-cli 0.6.1 were released on 2026-11-18; neverest takes them from crates.io, no patch.

- The store and the engine follow storage-, sync- and search-draft-14 (io-pimdir 0.17, vendoring pimdir da7e01a); sync.md's opening line names them. A store io-pimdir 0.16 wrote is draft-13 and refused as an earlier neverest's. `written_earlier` in src/sync/driver.rs now asks io-pimdir's `PimdirFormatRefusal::order` (pimdir STORAGE §4.2) rather than parsing the draft number itself; src/sync/driver/tests.rs `a_store_version_tells_an_earlier_neverest_from_a_newer_one` pins draft-13 as earlier, draft-15 and an unreadable marker as not.
- `enqueue_with_body` now removes every `X-PIMDIR-` property from a calendar or card body (pimdir STORAGE §18). neverest's conflict settle (src/conflict.rs) and its run merge (src/sync/driver/reconcile.rs) stay on it, the owner having ruled on 2026-11-18 that neither body is the user asking a performer for an instruction. sync.md's "A settled body is a body of its item" states it, with the scenario "A settled body carrying an instruction"; guard src/conflict.rs `a_settled_body_lands_without_its_pimdir_instructions`, failing with the settle on `enqueue_user_body`.
- SYNC §4's draft-14 refusal exceptions (401, 407, 423, a quota exceeded): src/error.rs's `names_the_request` already excluded them, nothing moved.
- pimalaya-config 0.6's `SecretError::Output` and `Timeout` touch no neverest code; the CHANGELOG names the new secret failure text.

Capabilities moved: sync (store draft, conflict settle).
