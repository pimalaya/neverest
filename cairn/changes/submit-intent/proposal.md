---
cairn: change
id: submit-intent
status: landed
created: 2026-08-07
---

# The Outbox becomes a queue intent

## Why

Neverest reserved an `Outbox` collection: a constant, a case-insensitive match,
an `ensure_collection` on every run, a filter threading through every collection
listing, and an `OutboxMeta` envelope schema owned by the sync engine. That is
mail-specific machinery inside an engine that is now kind-neutral everywhere
above the backend seam, and it made the store hold a mail concept it should not
know about: pimdir itself is clean (zero occurrences of "outbox" in io-pimdir and
io-replica, source and spec).

It also said the wrong thing about what a collection *is*. A collection is a set
of items that exist; a queued send is not an item that exists, it is work to be
performed. Modelling it as a placement in a fake collection meant the store had
to be told never to sync that collection, and every listing had to remember the
exception.

The queue already carries "work a frontend asked for". The blob already survives
in the store pinned by its queue row ("queued bodies are pinned" is a schema v1
requirement). What was missing was only the statement that an owner may meet an
action kind it cannot perform.

## What

- A **`submit` action kind, defined by neverest, not by pimdir.** The format
  carries an action kind and a versioned JSON payload; who understands which
  kind is the owner's business. An owner that does not recognise a kind, or
  recognises it but lacks the capability, **skips** the row: left pending, never
  parked, never blocking later actions.
- The payload is today's `OutboxMeta` (`v`, `from`, `rcpts`, `subject`), the
  body is the row's pinned `object_hash`, and the intent anchors on whatever
  collection the producer chose (a client typically picks `Sent`). Neverest
  scans every collection's pending actions, so there is no anchor rule and no
  schema change.
- Performing it: the existing channel resolution (the first side offering one,
  its own `<side>.smtp` table before its native Graph `sendMail`). On success
  the row is acknowledged (`drop_action`), releasing the body's pin. A
  **transient** failure (an SMTP 4xx, a broken connection) leaves it pending; a
  **permanent** one (an SMTP 5xx, a malformed payload, a missing body) parks it
  with its error (`fail_action`).
- A build with no send channel (neither `smtp` nor `msgraph`) skips submit
  intents: pending, never parked, since another build performs them.
- `OUTBOX`, `is_outbox`, the `ensure_collection(OUTBOX, …)` calls and the
  listing filter are gone. `src/offline/outbox.rs` becomes
  `src/offline/submit.rs`: channel connect, send one, classify the failure.

## Known property

Submission is **at-least-once**. A crash between "the server accepted" and "the
row is acknowledged" resends on the next run. That was already true with the
collection (the drop happened after the send), so it is not a regression, but as
a queue intent it becomes a stated contract, and deduplication is somebody's
declared job: the receiving provider's, through `Message-ID`. No transaction can
span an SMTP dialogue, so neverest cannot close the window by itself.

The producer half (himalaya's enqueue) does not exist yet; this half lands alone,
which is why the payload contract is documented in the module header rather than
inferred from a caller.
