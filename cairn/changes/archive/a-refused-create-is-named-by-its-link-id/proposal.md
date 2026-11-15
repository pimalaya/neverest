---
cairn: change
id: a-refused-create-is-named-by-its-link-id
status: landed
created: 2026-11-15
---

# A refused create is named by its link id

## Why

Live testing of 2026-11-15 (calendula part 2, P2-F3; himalaya part 2, F3 and F4; ledger lines N24 and N21 of cli-alignment-2026-11-15): every rejected create read `"id":""` (`{"side":"gcal","id":"","action":"append",...}`, the Graph append of N15 too), and its `add` hunk stayed in the patch with `error: null`, as if applied.

io-pimdir stages a create under its provisional handle, U+0001 followed by the link id (pimdir SYNC §2, `PimdirLinkId::provisional`). The report read it as the link id with a marker appended, an earlier shape, and kept what came before the U+0001: nothing. With an empty id, no hunk matched, so none was taken back.

## What

- src/offline/driver.rs `itemize_rejected` strips the leading U+0001: the report names the create by its link id, and its hunk, a copy or a frontend's add, is taken back.

## Decisions

- **The mailbox create stays a success (N21, first half).** A `collection-create` naming a mailbox the server already lists is a success: pimdir STORAGE Annex B.2 says so ("one already holding a collection of that name under that parent is a success, the intent being at least once"), as cairn/spec/sync.md "A collection is created through the queue" does. Parking it would park a create performed by a run whose answer was lost. Neverest lists before it creates, so it never sent iCloud the `CREATE` the report names. The refusal belongs to the producer, which can see the store lists the mailbox: himalaya's H29.
- **The row state is io-pimdir's (N21, second half).** A drained add is an applied queue row with a receipt (pimdir STORAGE §15.5); a push the server refuses later is reported under `rejected` and retried, the item staying in the store. Making the refusal the row's state (parked, coded, under the row's id) needs io-pimdir to park an applied row, or to tie a placement's rejected push to its receipt; io-pimdir 0.13 offers neither. Reported, no code here.
