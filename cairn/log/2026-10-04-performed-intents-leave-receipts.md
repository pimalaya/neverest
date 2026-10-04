---
cairn: log
change: performed-intents-leave-receipts
date: 2026-10-04
---

# A performed intent leaves a receipt

The intents neverest performs were acknowledged by `drop_action`, which records no receipt (2026-10-04, collections-created-and-occurrences): once the row was gone, a producer read a message sent or a folder created as `Unknown`, like a row cancelled by request. pimdir draft-04 now has a performer record the receipt when it acknowledges an intent, and io-pimdir `8c83c04` offers `acknowledge_action(id, seq)`.

## What landed

- Cargo.toml, Cargo.lock: io-pimdir by git rev `8c83c04`.
- src/offline/driver.rs: `settle_intent` (calendar intents and `collection-create`) and `drain_submits` (a `submit` without a copy to file) acknowledge with `acknowledge_action(id, None)`; a `submit` whose copy neverest files keeps `replace_action`, which now records the intent's receipt itself. No `seq`: a created collection and a provider-filed copy reach the store with a later listing, the copy neverest files is an `add` of its own, an invitation leaves its event as found. Parked and pending rows are untouched.
- Tests: unit `a_performed_intent_reads_as_applied_by_its_producer`; tests/create.rs and tests/submit.rs (Stalwart) follow the row with `action_status` to `Applied`. Run: unit tests all green; create on a Stalwart at :143/:8080, submit on a tests/stalwart2.sh server on other ports (the default ones held by another fixture), ports edited for the run only.

## Capabilities moved

- sync: a performed intent leaves a receipt (new requirement); the submit requirement acknowledges with the receipt.
