---
cairn: log
change: the-store-tests-run-one-at-a-time
date: 2026-11-14
---

# The store tests run one at a time

The Tests run of f4874ec failed `a_check_names_an_account_no_configuration_names` (tests/accounts.rs): its in-process `PimdirStore::open` was refused as `Owned` right after `initialized` had opened and released the same store. It is the race of the 2026-11-08 log a-spawn-never-overlaps-a-reopen, left open there for any other test: a `neverest` another test thread spawns holds a copy of every descriptor of the test process until its exec, owner.lock's among them, and the flock lives on until the last copy closes.

Reproduced with the accounts tests of f4874ec run 200 times: 44 runs failed, each at that reopen.

## What landed

- tests/accounts.rs, tests/drain.rs and tests/intents.rs, the three integration binaries whose tests run without a server and hold a store in-process beside spawns: a `SPAWNS` mutex every test takes for its whole run (`serialized()`), as tests/endpoints.rs does for its server. A fine-grained lock would deadlock `a_drain_leaves_a_busy_store_alone`, which holds the store on purpose while it spawns.
- The guarded accounts tests passed 200 runs out of 200, the intents tests 50 out of 50. The drain binary still takes about 20 seconds, its throttling test dominating.
- No program change: one run of neverest never spawns while another thread reopens a store.

## Left

- The server suites mixing in-process stores and spawns (attachments, carddav, create, deletes, duplicates, heads, hydration, moves, multikind, roles, scope, submit) run with `--ignored` and are exposed the same way; they take the guard when one fails so.

No capability moved.
