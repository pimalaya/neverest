---
cairn: change
change: follows-storage-draft-17
---

# What neverest needs from io-pimdir

For the io-pimdir lane (io-pimdir cairn/changes/what-the-consumers-need). Evidence in inventory.md.

1. **R5, R6, R6b and K4 fixed** (pimdir cairn/changes/a-smaller-sync/report.md, candidates/k3_rekey.rs, k4_drain.rs). Why: neverest rekeys IMAP on `Renumbered` and drains every account's queue, pair accounts included, so each case duplicates a message in the store and then on a server. Not reached through neverest: R0, R1, R2, R3, R4, R7, R8 (inventory F). App tests once released (tasks.md 19), each failing on 7f16b27 first: a renumbered `FakeImap` INBOX holding a message whose `Message-ID` a queued, unpushed `add` states syncs to one item and no second `APPEND`; the same with the item another endpoint offers; a no-UIDPLUS append delivered then renumbered is reported landed; a pair's queued `move` into a collection the other endpoint binds the identity in leaves one item and one message there. Workaround: none that keeps the drain-first rule.
2. **An owner write registering a source with no checkpoint** (pimdir the-freeze-plan/tasks.md:69). Why: src/sync/storage.rs:143-182 opens its own connection and runs `sql::LOAD_CHECKPOINT` and `sql::UPSERT_CHECKPOINT` itself. Workaround: the code as it is, which passes at 7f16b27.
3. **Optional, performance**: a read of one source's bound placements of a collection (handle, link id, sort key, revision) without the hub, which `Bound` (src/sync/remote.rs:142-173) builds from `load(All)` before every pass; and an indexed read of a collection's placements still owed a body, which phase 2 finds by projecting every hub (src/sync/driver/spine.rs:594-614). Workaround: the whole loads, most of which F1 and F2 remove; measured by tasks.md 14.
4. **The release**: io-pimdir 0.20.0 published by its tag workflow, for tasks.md 20.
