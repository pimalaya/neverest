---
cairn: log
change: a-listing-is-read-as-the-server-stated
date: 2026-10-30
---

# A listing is read as the server stated

The done half of connector-listings-fail-rather-than-shrink, split out on 2026-10-30; that change stays active for what is left.

## What landed

- src/imap/backend.rs: a `SELECT` without `EXISTS` is searched, never read as an empty mailbox.
- src/dav/client.rs, src/gcal/client.rs, src/gpeople/client.rs: a lone read answered 404 or 410 fails with `Gone`, so `hydrate_batch` records the member gone instead of failing the collection's fetch in every run.
- src/msgraph/client/mail.rs: a band page resumes below the second the page before ended at (`Step::Below`), a page within one second following Graph's next link in the same request; a band next link a round stored before is refused, which opens the round again.
- The meta-less mail entry (src/offline/remote.rs) was closed as unreachable by construction: every connector lists a member bare only when the store's metas name it (`Held::holds`, `Bound::from_loaded`).
- Tests: `a_select_without_exists_is_searched`, `a_member_missing_from_every_read_is_gone`, `an_event_missing_from_its_read_is_gone`, `a_person_missing_from_its_read_is_gone`, `a_band_round_resumed_after_a_delete_misses_nothing`, `a_band_dense_in_one_second_still_goes_on`.

## Evidence

On the old code: the IMAP search answered `left: []`; the DAV batch failed with `Stream item t1.ics in tasks error`; the Google reads failed with `Get event e1 of c1 error: Calendar API returned HTTP 404: Not Found` and `Get person c1 error: People API returned HTTP 404: Not Found`; the resumed Graph band missed `2026-06-07`; the dense band hung without the in-request follow.

## Capabilities moved

- sync: an IMAP mailbox is never read as smaller than it stated (added); a Graph band page resumes below a date (added); a mail round lands page by page, newest first (modified: the Graph band cursor is a date, a moved `UIDVALIDITY` a rekey); a body gone from the server is owed nothing more (modified: DAV, Google Calendar and Google People covered).
