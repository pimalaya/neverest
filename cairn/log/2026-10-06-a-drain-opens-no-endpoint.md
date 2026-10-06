---
cairn: log
change: a-drain-opens-no-endpoint
date: 2026-10-06
---

# A drain opens no endpoint

A queued create has no `seq` until the store's owner applies it, and io-pimdir's overlay leaves it out, so a frontend could not read back an item it had just created. Neverest drained only inside a sync, after reading every credential and, in the one-source sync, after opening the connections: a run that could not reach its server applied nothing. Reported by MOA, whose Agenda showed a created event only after a successful sync.

## What landed

- src/offline/driver.rs: `run` drains once, after `declare` and before `Account::resolve`, not under `--declare-only`, a dry run on its replica; the drains of `run_local` and `run_pair` removed. `open_store`, `declare` and `drain_queues` are `pub(crate)` for the new command.
- src/cli/drain.rs: `neverest drain`, declaring the sources then draining, reading no credential; `try_store_lock` (src/cli/sync.rs) never waits, a held `sync.lock` answering `busy: true` and exit 3. The output lists each applied row (`id`, `collection`, `kind`, `seq` for an `add`), the parked rows and the count left for a sync. Schema `neverest-drain`.
- tests/drain.rs: with the CalDAV endpoint on a closed port, a drain and a sync (exit 3) both apply a queued create under the reported `seq`, and a drain leaves a busy store alone; on Stalwart (:8080, ignored by default) the drained event keeps its `seq` after the push and the fetch back, and a second store of the server lists it. Run: unit tests all green; create, caldav, carddav, moves and drain on a Stalwart at :143/:8080 and the Radicale at :5232. submit.rs not run: its SMTP server (tests/stalwart2.sh) was not up.

## Capabilities moved

- sync: the drain runs once per run before any credential or endpoint (modified); `neverest drain` (new requirement); "A source drains the collections of its own namespace" retired, the drain being the store's.
