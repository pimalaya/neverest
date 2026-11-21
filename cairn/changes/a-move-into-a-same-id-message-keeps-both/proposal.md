---
cairn: change
id: a-move-into-a-same-id-message-keeps-both
status: active
created: 2026-11-21
---

# A move into a same-ID message keeps both

## Why

Found while reviewing PR #36 (2026-11-21). Two different messages stating one `Message-ID` (a broken sender, a re-sent message with edited content) end up with the store holding one body twice and the other never, and an item bound to the server message whose body it does not hold. Nothing is lost on the server, but any later action on the mis-bound item (a delete, a flag) reaches the other message.

## Evidence (2026-11-21, Stalwart v0.16, neverest 5705f99, io-pimdir 0.19.1)

repro.patch adds `a_move_into_a_message_stating_its_id_keeps_both` to tests/moves.rs. Message A (body `MOVEDMARKER`) sits in Left, message B (body `OTHERMARKER`, same `Message-ID`) lands in Right, A is moved to Right. The server ends with both bodies, once each, in both variants. The store does not:

- **B listed before the move.** io-pimdir pulls B with its body "linked from store" by link id (io-pimdir src/upgrade.rs, around the "fetch {} bodies, {} linked from store" debug line): B's item holds A's body, never fetched. The move then mints `dup:` for A's item, as designed.
- **B unlisted when the move is pushed.** The move relocates A (UID 2; B is UID 1). The next listing lands the pending create `same-…` on UID 1, the first UID stating the `Message-ID` (io-pimdir src/sync.rs, "land the pending create {} under {}"), and pulls UID 2 as `dup:…#2`. The item holding A's body is bound to B.

`targets_holding` (src/sync/remote.rs) did not take part: the search found nothing in either run, so the suspected delete never happened.

## What

The fixes belong to io-pimdir, to propose there:

- A body is linked by link id only where the server message is proven the same bytes (a size or hash the listing states), else fetched: a `Message-ID` names a mail, not its bytes, as a link id names an item and not its bytes for revisioned content already.
- A pending create lands on the message its push named, not the first one stating its link id. Here neverest can help: `UID MOVE` and `UID COPY` answer `COPYUID` (RFC 4315), which `move_messages` drops today; a relocated remove could carry the target UID, if io-pimdir's push result takes one.

Then repro.patch lands in tests/moves.rs, passing.
