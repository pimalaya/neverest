---
cairn: log
change: submit-intent
landed: 2026-08-07
---

# The Outbox became a queue intent

Neverest reserved an `Outbox` collection: a constant, a case-insensitive match,
an `ensure_collection` on every run of both paths, a filter in every collection
listing, and an `OutboxMeta` schema owned by the sync engine. Mail-specific
machinery inside an engine that speaks collections and items everywhere else,
and a mail concept pushed into a store that had none (pimdir and io-replica
carry zero occurrences of "outbox", source and spec). It also mismodelled the
thing: a collection holds items that exist, and a queued send is not an item, it
is work to be performed.

**The intent** (`offline/outbox.rs` → `offline/submit.rs`): a queue action whose
kind (`submit`) neverest defines, not pimdir. The format carries a kind and a
versioned JSON payload; which kinds an owner can perform is the owner's
business, so an owner meeting one it cannot perform **skips** the row (pending,
never parked, never blocking later actions). io-pimdir reads such a row back as
`PimdirAction::Unknown { kind, payload, object_hash }`, which is exactly what
`submit::pending` filters on.

The payload is the former `OutboxMeta` (`v`, `from`, `rcpts`, `subject`) plus
`object`, the body hash, by the convention every pimdir action kind follows: it
is what puts the hash in `queue.object_hash`, and therefore what pins the body.
The body is written durably before the enqueue and belongs to no collection. The
whole contract is documented in the module header, because the producer half
(himalaya's enqueue) does not exist yet and this half lands alone. The intent
anchors on whatever collection the producer chose: the drain already iterates
`queued_collections()`, so there is no anchor rule and no schema change.

**Performing it** (`driver.rs::drain_submits`, replacing `flush_outbox`): the
same channel resolution as before (`open_send_channel`, the first side offering
one, its own `smtp` table before its native Graph `sendMail`). Success
acknowledges the row (`drop_action`), releasing the body's pin so GC reclaims it.
Failures are now **classified**, which is the point of moving into the queue:

- transient (an SMTP 4xx, a dropped connection, a Graph 5xx / 408 / 429):
  `fail_action(id, None)` bumps the attempt counter and leaves the row pending,
  so the next run retries;
- permanent (an SMTP 5xx, an undecodable payload, an unsupported payload
  version, a missing body, a Graph 4xx): `fail_action(id, Some(error))` parks it
  with its error, queryable, never resent forever and never silently dropped.

`GraphClient::send_mime` returns its client error unwrapped so the HTTP status
is readable at the classification point. A build with neither `smtp` nor
`msgraph` skips submit intents entirely: they stay pending, never parked, since
another build performs them.

**Removed**: `OUTBOX`, `is_outbox`, both `ensure_collection(OUTBOX, …)` calls
and the listing filter. A remote folder named `Outbox` now syncs like any other
one, which the driver test pins.

**Report**: `outbox` → `submitted`, entries carrying the queue row id, the
anchoring collection, the subject, the error and whether it parked.

**Known property, now stated**: submission is at-least-once. A crash between the
server's acceptance and the acknowledgement resends next run. True with the
collection too (the drop followed the send), so not a regression, but as a queue
intent it is a visible contract and dedup is the receiving provider's declared
job (`Message-ID`). No transaction spans an SMTP dialogue.

Verified against io-pimdir's working tree: 56 unit tests green, including an
intent that survives the store's drain (skipped, not parked, no item created)
and is read back with its envelope and its pinned body, an intent sending that
body through the envelope it carries, a 5xx parking where a 4xx retries, and an
undecodable or bodyless intent parking instead of looping. Clippy clean across
every feature subset.

Breaking: anything writing into the `Outbox` collection to have it sent must
enqueue a `submit` action instead. This lands before 1.0.0.

Spec updated: `sync` (MODIFIED: "The Outbox is local-only and flushes through
the send channel" became "A queued submission is a `submit` queue intent"; the
sole-owner requirement now states that a capability-bound intent is left pending
by the drain, never parked).
