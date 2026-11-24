---
cairn: tasks
change: the-live-night-findings
---

# Tasks

Each task lands with a test failing first, the gate, and a spec fold.

## Rulings (owner)

- [ ] R1: sent copy outside Sent on Gmail and Graph: label on Gmail and Partial on Graph (recommended), or import/copy on both.
- [ ] R2: a delete beaten by a server edit: a report line only (recommended), or a conflict.
- [ ] R3: a collection deleted on the server: retire and report (recommended), or report only.

## Data

- [ ] N1 (after R1): send.rs files natively only into `sent`; Gmail adds the label; Graph declares `mail.submit.copy` Partial. Unit test per provider; fold sync.md:1650 and pimdir STORAGE §843.
- [ ] N2 (after R2): report "delete overruled by a server edit" in text and JSON. Test: a queued delete over a server edit (driver test).
- [x] N3: Graph: base a pushed recurring event on the series read back after the create. Test: msgraph suite, a delete right after a create goes through. Landed 2026-11-24 as a create without an empty body (any body write is rewritten by Exchange, not only a series); an event with a description is left open (cairn/log/2026-11-24-a-graph-event-created-without-a-body.md).
- [x] N4: a submit's copy anchor survives a rename drained in the same run. Test: drain a submit and a rename of its copy folder together; the copy is filed in the renamed folder. Landed 2026-11-24 (cairn/log/2026-11-24-a-sent-copy-follows-a-rename.md); a rename from an earlier run waits on io-pimdir.
- [ ] N5 (after R3): retire a collection the server deleted, report it, park while a row anchors on it. Test on Stalwart: IMAP DELETE of a folder holding one message.

## Providers

- [x] N6: Google Meet on create through `events.insert` (or the follow-up patch). Test: tests/google.rs `a_google_meet_is_created_with_its_event` passes. Landed 2026-11-24 (cairn/log/2026-11-24-a-google-meet-on-an-event-with-no-attendee.md); the suite's harness stripped the request too.
- [ ] N7: Graph `isOnlineMeeting` on create and update. Test: msgraph suite asserts it. 2026-11-24: neverest sends `isOnlineMeeting: true` (the suite now really asks, see N6); the test tenant's calendars allow no provider (`allowedOnlineMeetingProviders: []`, default `unknown`), so Graph stores false, as the `partial` declaration says. `onlineMeetingProvider` waits on an io-msgraph field and a tenant with Teams.

## Report nits

- [x] N8: `sync --init -d` initializes nothing. Landed 2026-11-24 (cairn/log/2026-11-24-a-dry-run-initializes-nothing.md).
- [x] N9: a dry run says what it would apply, not "applied N queued action(s)". Landed 2026-11-24 (cairn/log/2026-11-24-a-dry-run-would-apply-the-queue.md).
- [ ] N10: dry-run item names use the UID, as a real run does. 2026-11-24: needs a ruling, sync.md "The report shows the one-source pull plan" says a dry run names such a member by its handle, its body not fetched (a preview downloading a whole address book is not one); the UID is only in the body.
- [x] N11: a collection new on the server shows in `collectionPatch`, not "already in sync". Landed 2026-11-24 (cairn/log/2026-11-24-a-new-server-collection-is-reported.md).
- [x] N12: `Errors (1):` lists the submission error it counts. Landed 2026-11-24 (cairn/log/2026-11-24-the-errors-counted-are-listed.md).
- [x] N13: a collection rename performed shows as a hunk, not "not synchronized" or "already in sync". Landed 2026-11-24 (cairn/log/2026-11-24-a-performed-rename-is-a-hunk.md); "not synchronized" not reproduced.
- [x] N14: a bare `conflict list` reports the uninitialized account and goes on with the others. Landed 2026-11-24 (cairn/log/2026-11-24-a-bare-conflict-list-skips-uninitialized.md).
- [ ] N15: `-m` also matches a collection's display name (iCloud and Google calendars are UUIDs). 2026-11-24: needs a ruling, sync.md "Bodies are content-addressed and deduped" builds the filter from the backend's id alone, a display name being optional, mutable and free to collide; a name matched in an exclude would drop a collection a server renamed into it. Recommendation: `-m`/`-x` on the command line only, a name matched only where no id matches, the configured filter unchanged.

## Housekeeping

- [ ] Drop the parked row #19 of the shared store (anchored on `stalwart/<U+FFFD>attachments` by the pre-fix himalaya).
- [ ] Next live run: add JMAP and Graph contacts/calendars sources to the live rc; run `--reset`, `--accept-mode` and conflicts on real accounts.
