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

## Open question: an open series older than five years (2026-11-19)

src/msgraph/client/calendar.rs `series_windows` reads a series with an end date whole, in five-year windows, and an open-ended one only within five years of today on either side (`OPEN_SERIES_YEARS`), so a birthday Outlook stores from 1604 costs two or three requests and not 85. An exception of an open series older than that floor, or further than five years ahead, is left out, and the series is written to the other endpoint without it: the delta's "read whole or not at all" is not met for that case alone.

Neither reading of the delta is free. Reading every open series from its start costs one request per five years of its age per series per body read (85 for a 1604 birthday, every time its body is read). Failing the read whenever the series starts before the floor fails every yearless birthday, in every run, for good.

Recommendation: read an open series from the later of its start and the date its master states as `createdDateTime`, five years at a time, and up to five years past today as now. An occurrence is edited once the series exists in the mailbox, so a floor at its creation keeps every exception a person made there and costs a 1604 birthday created in 2019 two requests. A series imported with its exceptions may predate its own creation: whether Graph keeps the original `createdDateTime` on an import is the live check to make before the code, on the Microsoft test account with the owner's go.

Ruling (owner, 2026-11-19): the recommendation is taken. The code lands after the live check above.
