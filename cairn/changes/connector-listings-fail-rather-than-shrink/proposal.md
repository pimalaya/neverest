---
cairn: change
id: connector-listings-fail-rather-than-shrink
status: active
created: 2026-10-30
---

# Connector listings fail rather than shrink

## Why

Pimdir stable-release review (2026-10-30), connector holes (report-neverest.md, "Connectors"). Each reads a listing as smaller or more final than the server stated, which a complete round turns into deletes, or stalls a sync for good. The holes fixed on 2026-10-30 (IMAP `EXISTS`, the gone DAV and Google members, Graph band paging, the meta-less entry closed) landed as a-listing-is-read-as-the-server-stated; what is left:

- io-webdav skips a member row with a non-2xx status other than 404 and 507 inside a complete listing (io-webdav src/rfc6578/sync_collection.rs:288-293): a 403 or a 5xx member reads as deleted.
- A DAV sync token refused with 409 instead of 403 is not read as invalid (io-webdav :222-227): the collection never syncs again. No server answering 409 is known yet.
- Graph series beyond five years lose their exceptions (src/msgraph/client/calendar.rs:192-195, :493-523), and the partial body propagates.
- Graph shared calendars read a body through `users/{me}/events/{id}` while listing through the calendar (may 404 and read as gone): needs a live check on the Microsoft test account.

## What

- A member row the server answered with an error inside a complete `sync-collection` listing makes it incomplete, fixed in io-webdav, then the pin.
- A 409 sync-token refusal restarts the listing, as a 403 `valid-sync-token` does, once a server is shown to answer one.
- A Graph series reads its exceptions past five years, or fails rather than propagating a partial body.
- The shared-calendar read is checked live first, with the user's go.
