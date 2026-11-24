---
cairn: change
id: the-live-night-findings
status: active
created: 2026-11-24
---

# The live night findings

The live test night of 2026-11-23 (workspace .lanes/live/report.md and journal.md) ran every neverest command on Stalwart, Google, Microsoft, Fastmail, iCloud and posteo, and drained the queues of himalaya, cardamum and calendula. Two bugs are fixed (34bc352, a move reported twice; 13c342c, the io-pimdir pin landing a relocation on the flags it carries). What is left is mostly what a run reports when an action did not fully happen: the row reads `applied`, the report says nothing, the user believes it happened.

## What

Data first, each with the ruling it needs and the recommendation:

1. **A sent copy dropped on Gmail and Graph.** src/sync/driver/send.rs:153 drops any `copy` when the provider files the copy itself (capability.rs:303-323), not only a copy into Sent; the row ends `applied`. Recommendation: file natively only when the copy names the `sent` role. Otherwise, Gmail: add the target label to the sent message (`messages.modify` on the id `send` returns: exact, no duplicate); Graph: declare `mail.submit.copy` Partial ("Sent Items only") so the producer refuses another copy target at queue time. Folds into pimdir STORAGE §843 and sync.md:1650.
2. **A delete beaten by a server edit reads `applied`.** pimdir SYNC §5 has the edit win, by design; the report must say so. Recommendation: a report line and a JSON hunk "delete overruled by a server edit" naming the item. On Graph the edit is neverest's own: Graph rewrites a recurring series after its create, so the base must take the series as read back after the push, else the user's first delete is always beaten.
3. **A submit queued beside a rename of its copy folder is parked "unconfirmed".** Recommendation: resolve the copy anchor by collection id when the submit is performed, or drain submits before collection operations.
4. **A collection deleted on the server stays in the store, unreported.** Recommendation: report a collection hunk and retire it from the store (the store mirrors, `collection.delete` gates pushing a delete to other sources, not forgetting one), parking it instead while a queued or parked row anchors on it.

Then provider failures:

5. **Google Meet on import.** An imported event's `createRequest` stays `Pending`; tests/google.rs `a_google_meet_is_created_with_its_event` fails. Recommendation: create through `events.insert` when the event asks for a meeting, or follow the import with a `conferenceDataVersion=1` patch.
6. **Graph online meeting.** `X-PIMDIR-ONLINE-MEETING:TRUE` leaves `isOnlineMeeting` false. Recommendation: set `isOnlineMeeting` and `onlineMeetingProvider` on create and update.

Then report nits (one task each, below).

## Not tested, for the next live run

JMAP sources (no account of the night's rc used one), Graph contacts and calendars on the shared store, `--reset` and `--accept-mode` on real accounts, conflicts on Google, Microsoft and iCloud.
