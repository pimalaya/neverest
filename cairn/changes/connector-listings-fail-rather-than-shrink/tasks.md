---
cairn: tasks
change: connector-listings-fail-rather-than-shrink
---

# Tasks

Done on 2026-10-30 and landed as a-listing-is-read-as-the-server-stated: the IMAP `EXISTS` fix, the meta-less entry (closed, unreachable by construction), the DAV, Google Calendar and Google People gone members, the Graph band paging.

- [ ] `dav`: a 409 sync-token refusal restarts the listing; test, failing first. Needs evidence first: no server answering 409 is known (code reading of io-webdav only).
- [ ] io-webdav: a non-2xx member row other than 404 and 507 makes the listing incomplete (upstream change, then the pin).
- [ ] `msgraph`: series exceptions beyond five years read or the series fails; test, failing first. Needs a fake of the instances listing first.
- [ ] `msgraph`: shared-calendar body read, after a live check on the Microsoft test account with the user's go.
- [ ] Spec folded, log written, CHANGELOG.
