---
cairn: tasks
change: connector-listings-fail-rather-than-shrink
---

# Tasks

Done on 2026-10-30 and landed as a-listing-is-read-as-the-server-stated: the IMAP `EXISTS` fix, the meta-less entry (closed, unreachable by construction), the DAV, Google Calendar and Google People gone members, the Graph band paging.

- [ ] `dav`: a 409 sync-token refusal restarts the listing; test, failing first. Needs evidence first: no server answering 409 is known (code reading of io-webdav only). Re-checked 2026-11-19 on io-webdav 0.11.0: still read as a failure; left for the evidence.
- [ ] io-webdav: a non-2xx member row other than 404 and 507 makes the listing incomplete (upstream change, then the pin). Re-checked 2026-11-19: io-webdav 0.11.0, the release neverest locks, still skips such a row (src/rfc6578/sync_collection.rs:293, `from_multistatus`), so neverest never sees it; an io-webdav change and release first.
- [ ] `msgraph`: series exceptions beyond five years read or the series fails; test, failing first. The scripted Graph session now fakes the instances listing (src/msgraph/client/calendar.rs tests, `instances`). Waits on the owner's ruling written in proposal.md, "Open question: an open series older than five years" (2026-11-19). Ruled 2026-11-19: the recommendation is taken (floor at the later of the start and `createdDateTime`); the code waits on the live check of `createdDateTime` on an imported series (Microsoft test account).
- [ ] `msgraph`: shared-calendar body read, after a live check on the Microsoft test account with the user's go.
- [ ] Spec folded, log written, CHANGELOG.
