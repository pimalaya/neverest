---
cairn: change
id: a-drain-opens-no-endpoint
status: landed
created: 2026-10-06
---

# A drain opens no endpoint

## Why

A frontend (himalaya, calendula, cardamum) queues its writes and only the store's owner applies them (pimdir STORAGE §8, §15). Until then a created item has no `seq`, and no reader lists it: io-pimdir's overlay folds the actions on existing items and leaves a queued `add` out, having no id to show it under. So `calendula event create` followed by `calendula event list` did not show the event.

Neverest drained only inside a sync, and late: after every credential was read and, in the one-source sync, after the connections were opened. A run that could not reach its server (offline, an expired token) drained nothing, though applying the queue is a store write with no network in it. Reported by MOA, whose Agenda showed a created event only after a sync had succeeded.

## What

- `sync` drains once per run, at its start, after the declaration and before any credential is read or any endpoint opened, as the spec already said ("before any network work"). Not under `--declare-only`. A dry run drains its replica, as before, so it shows the creates it would push.
- The per-source drains go: the store applies each action as the source syncing its collection (STORAGE §15.2), so one store-wide drain answers for every source. The namespace rule of 2026-08-28 is retired with them.
- `neverest drain -a <account>`: the same drain, alone. It reads no credential and opens no endpoint, so it works offline. It never waits: a sync holding the store's `sync.lock` gets `busy: true` and exit 3 (the run, or the next, drains). It reports each applied row with its `seq` for an `add`, the parked rows, and how many wait for a sync (intents).

A frontend runs `neverest drain` right after queuing, then reads its write back; the push is the next sync's, which a caller debounces.

## Not done

No drain at the end of a sync: a row queued during a run waits for the next run, which a caller that queued it starts anyway.
