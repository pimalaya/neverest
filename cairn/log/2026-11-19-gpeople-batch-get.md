---
cairn: log
change: gpeople-batch-get
date: 2026-11-19
---

# Google People bodies read in batches

io-gpeople 0.8.1, the release neverest locks, has `people:batchGet` (`GpeoplePersonsBatchGet`).

## What landed

- src/gpeople/client.rs: `fetch_bodies` reads its ids 200 a request through `people:batchGet` with the vCard fields, committing each person with its etag as its batch lands; a person answered 404 or 410 is answered gone, one refused otherwise left unanswered for the per-item read. `person` stays for `get_item_stream`. src/client.rs passes the gone ids on.
- Tests: `a_batch_reads_its_persons_and_a_missing_one_is_gone` and `four_hundred_fifty_persons_are_read_in_three_requests` over a scripted session; the first fails to build on the code before (`fetch_bodies` answered nothing), the second sent 450 requests there.

Left: tests/google.rs's cleanup by resource name and the live run, both in the change's tasks.

## Spec

- [sync](../spec/sync.md): new *Google People bodies are read in batches*.
