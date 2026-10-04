---
cairn: log
change: collections-created-and-occurrences
date: 2026-10-04
---

# Collections created through the queue, and one occurrence answered

pimdir draft-04 (io-pimdir `3a9ffb8`) adds the `collection-create` intent and an optional `recurrence_id` in `calendar-reply` and `calendar-cancel`, with their capabilities. MOA creates the folder a box lacks (an Archives) through the store rather than by its own code, and lets you answer or cancel one meeting of a series.

## What landed

- Cargo.toml: io-pimdir by git rev `3a9ffb8`, io-msgraph by git rev `240af15`, until their next releases; jiff for the `gcal` feature too.
- src/offline/create.rs: the pending `collection-create` rows, read with io-pimdir's `PimdirCollectionCreate`.
- src/offline/driver.rs: `drain_collection_creates`, at the start of a sync before the submissions, the calendar intents and the listing, in both modes; the parent mapped out of the performer's namespace, the row settled as the calendar intents are (`settle_intent`, shared).
- src/client.rs: `Client::create_named_collection`: IMAP under the parent with its `LIST` delimiter (`ImapClient::child_mailbox`, refusing `\Noinferiors` and a flat namespace), Gmail `parent/name` (`create_child_label`, `child_label` refusing system labels and `/`), DAV keyed by a free segment of the name (`free_segment`) and displayed as it (`create_collection_named`), refusing a parent; an existing collection is success; other backends refuse.
- src/offline/invitation.rs: `InvitationIntent::occurrence` reads and validates `recurrence_id`; `occurrence_window`, `utc_stamp`, `wall_stamp`. src/msgraph/client/calendar.rs `occurrence_event` matches the instances by io-msgraph's `MsgraphEvent::recurrence_id_of` or their UTC stamp; src/gcal/client.rs `occurrence` by the wall time, date or UTC stamp of `originalStartTime`. The verbs then act on the instance.
- src/offline/capability.rs: `supports` (a row per capability, what a check reports) beside `declaration`, which splits the intents performed on what a source holds (`held_only`: reply, cancel, both occurrences, collection.create) into a `none` source-wide row and one per collection. `collection.create`: IMAP, Gmail, DAV full; Graph, Google Calendar and People `none`; `collection.create = false` makes it `none`. Occurrences as their intents; `none` on CalDAV.
- Receipts: io-pimdir's `drain`, which neverest already runs, records them for the rows it applies, an `add`'s `seq` included. The intents neverest performs (`submit`, the calendar intents, `collection-create`) are acknowledged by `drop_action`, which records none: a producer reads such a row as `Unknown` once it is gone.
- Tests: tests/create.rs on Stalwart (an IMAP folder, a child under it, the same intent again; a CalDAV calendar, and a parent parking); unit tests on the Gmail label (`child_label`), the DAV segment, the payload, the occurrence window and stamps, Google's instance match, the declarations. Graph and Google occurrences by unit tests only: no test credentials here.

## Capabilities moved

- sync: a collection is created through the queue.
- sync: an intent limited to one occurrence acts on that instance.
- sync: a check reports what each source does where it holds.
