---
cairn: log
change: a-card-is-named-from-every-octet
date: 2026-11-17
---

# A card is named from every octet

Finding NEV-4 of the CLI family analysis of 2026-11-15.

## What landed

- Every streamed body went through one header scan written for mail: it stopped at the first blank line or at 256 KiB, and the link id and summary were derived from that prefix. A card whose `UID` followed an inline `PHOTO` past 256 KiB, or any card with a blank line before its `UID`, was keyed `hash:` of the prefix, and the same contact on the other endpoint of a pair, serialised otherwise, read as another one and was copied each way.
- The scan now reads a card or a calendar resource whole (`HeaderScan::new(kind)`), on a streamed fetch, a batched one and a body served from the store; mail keeps its header block.
- No rekey: no released neverest holds such a store (0.x users start over at v1, whose stores begin empty), so a store built from v1.0.0 keys every card from its whole body from its first run. A store a development build of 2026-11 filled keeps a prefix key until the card changes; rebuild it to rekey.

## Spec

sync.md, the streaming paragraph of mail (now naming cards and calendar resources).
