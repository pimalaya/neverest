---
cairn: log
change: a-refused-create-is-named-by-its-link-id
date: 2026-11-15
---

# A refused create is named by its link id

Ledger lines N24 and N21 of the CLI alignment round of 2026-11-15.

## What landed

- src/offline/driver.rs `itemize_rejected`: the provisional handle (U+0001, then the link id) read as io-pimdir stages it, so a refused create is named by its link id, never `""`, and its hunk taken back.

## Left

- A refused push as the state of the queue row that staged it (parked, coded, under the row's id): io-pimdir's.
- A `collection-create` of a mailbox the server lists stays a success (pimdir STORAGE Annex B.2): the producer refuses it (himalaya H29).

## Capabilities moved

- [sync](../spec/sync.md): *A refused write names the item as a person knows it* amended.
