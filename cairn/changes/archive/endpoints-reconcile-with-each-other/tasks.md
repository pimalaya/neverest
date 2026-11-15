---
cairn: tasks
change: endpoints-reconcile-with-each-other
---

- [x] Read each item's shared body before a reconcile round, as the merge's ancestor
- [x] Detect the items both endpoints rewrote in one round, from the store alone
- [x] Hydrate the source's body as the shared one and record the target's as the divergence against it
- [x] Route it through the existing conflict resolution, so a settling merge and a parked collision take the same path
- [x] Skip it under `one-way`, where the source decides, and on a dry run
- [x] Report a parked cross-endpoint divergence once, and count it among the outstanding
- [x] Narrow the store an upgrade reads to what its source holds, so a copy on offer claims no identity
- [x] Test: a card changed on both endpoints merges or parks, and neither body is overwritten
- [x] Test: two endpoints already holding one card bind it to a single item
- [x] Test: a copy on offer is not read as a holding of the side it is offered to
