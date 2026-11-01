---
cairn: tasks
change: a-listing-is-read-as-the-server-stated
---

# Tasks

- [x] `imap`: a `SELECT` without `EXISTS` is searched, never read as empty; test over a fake server, failing first (`a_select_without_exists_is_searched`, `left: []` on the old code).
- [x] `offline/remote`: a mail entry without meta. Closed as unreachable by construction (2026-10-30): IMAP derives a meta from any header block, an empty one included; Gmail (src/gmail/client.rs, the `bound` and `settle` paths) and Graph (src/msgraph/client/mail.rs, `received`, `pass` and `delta`) list a member bare only when `Held::holds` it, and `Held`'s handles are the keys of the very metas `PimRemote` names a bare member from (src/offline/remote.rs, `Bound::from_loaded`), so a bare entry always finds its name. No failing test can be written; no change made.
- [x] `dav`: an href answering 404 in every multiget stops blocking the collection: a lone read answered 404 or 410 is `Gone`, so the batch answers it gone instead of failing (src/dav/client.rs `get_item_stream`); test over a scripted server, failing first (`a_member_missing_from_every_read_is_gone`, `Stream item t1.ics in tasks error` on the old code).
- [x] `gcal`, `gpeople`: a lone read answered 404 or 410 is `Gone` (src/gcal/client.rs and src/gpeople/client.rs `get_item_stream`, `is_missing`); tests over a canned answer, failing first (`an_event_missing_from_its_read_is_gone`: `Get event e1 of c1 error: Calendar API returned HTTP 404: Not Found`; `a_person_missing_from_its_read_is_gone`: `Get person c1 error: People API returned HTTP 404: Not Found`, on the old code).
- [x] `msgraph`: band pages resume below the second the page before ended at (`Step::Below`), never at Graph's `$skip` next link, a page lying within one second following Graph's next link in the same request; a stored band next link is refused, which opens the round again. Tests, failing first: `a_band_round_resumed_after_a_delete_misses_nothing` (`2026-06-07` missing on the old code), `a_band_dense_in_one_second_still_goes_on` (hangs without the in-request follow).
- [x] CHANGELOG (IMAP `EXISTS`; DAV, Google Calendar and Google People in the body-gone entry; the Graph band paging is part of the unreleased scoped sync).
- [x] Spec folded, log written.
