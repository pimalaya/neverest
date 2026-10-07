---
cairn: change
id: scoped-mail-sync
status: landed
created: 2026-10-07
---

# A mail sync lists newest first, page by page, within a scope on the `Date`

## Why

Block 4 of the joint plan (pimdir `cairn/changes/scoped-mail-sync`): io-pimdir `35a1c3f` implements pimdir draft-04's paged, scoped rounds, and neverest is the connector MOA runs it through.

A 1 GB Microsoft 365 box showed nothing in MOA for about two minutes: neverest listed a whole folder before writing anything, Graph's message delta answered ten mails a page, and every listed handle was then probed and raised to `Meta` one request at a time. There was no way to bound a mailbox by date, nor to say what part of it the store holds.

## What

- **The seam.** io-pimdir's remote seam answers a `PimdirEnumerate` (a delta from the checkpoint, or a round over a scope from its start or its cursor) with one page, or `CursorRejected`. Every listed member carries its meta (hint, summary, sort key); probes and the `Probed` level are gone. `Client::enumerate` takes the request and what the store binds (`Held`), and answers `Listed`; `Client::scope_bound` is false for IMAP and Gmail.
- **IMAP.** Rounds of 500 UIDs, highest first, the UID list read once by `UID SEARCH` (`SENTSINCE` a day below the floor or no `Date`, `SENTBEFORE` two days above a ceiling); each page one `UID FETCH (UID FLAGS RFC822.SIZE BODY.PEEK[HEADER.FIELDS (…)])`, `Content-Type` among the fields, no `BODYSTRUCTURE`, the meta derived by io-pimdir from the header block as the body would be; a UID the store binds under the same `UIDVALIDITY` reads its flags only. Checkpoint `(UIDVALIDITY, HIGHESTMODSEQ)` on the first page; cursor `(UIDVALIDITY, lowest UID)`, refused when `UIDVALIDITY` moved. QRESYNC deltas fetch the meta of the changed UIDs the store does not bind.
- **Graph mail.** A round is a fresh message delta with the summary `$select`, `$filter=receivedDateTime ge since − 2 days` under a scope, `Prefer: odata.maxpagesize=1000` on every request; each response one page, its next link the cursor, the delta link on the last page the checkpoint; an expired link (410) is `CursorRejected`. Meta from the row: `sentDateTime` as the date, `hasAttachments` as the mark, `ccRecipients`.
- **Gmail.** Rounds of 100 ids (`messages.list`, `q=after:<since − 2 days> before:<until + 2 days>` in epoch seconds), the profile's `historyId` read before the first page as the checkpoint, the page token as the cursor (a 400 is `CursorRejected`), metadata read for the ids the store does not bind (`Content-Type` added to the headers; io-gmail has no HTTP batch, so each is its own paced read), flags of bound ids from three id-only listings read once a run. History deltas read the metadata of the unbound.
- **DAV, Google Calendar and People, Graph contacts and events.** Listings stay whole; `PimRemote` fetches the bodies of the members the store does not hold at the listed revision (64 a request, across the pool) before the page lands, each member carrying its body. Graph contacts delta asks 1,000 a page. A dry run names them by handle, fetching nothing.
- **Scope.** `item.filter.since` (account) and `sync --since`: a duration back from today, taken to the start of its UTC day, a date or an instant. Refused, naming the key it came from, when a synced endpoint holds contacts or calendars. `PimRemote` drops every listed member whose `Date` lies outside the scope (no usable date is in every scope).
- **Report.** `coverage: [{ source, collection, since?, until?, at?, round?: { since?, until?, startedAt } }]` for each collection synced; a round left open makes the run incomplete. `downloaded: [{ source, bytes }]`: bodies and listed meta.
- **Throttling, corrected.** A request creating something (an uploaded or copied message, an event, a contact, a label, an import, a send, an invitation reply or cancel, a DAV `PUT` create or `MKCOL`) is sent again on 429 or a Google rate limit only, never on 503. Graph body batches answered 500, 502 or 504 are retried but give the source up only when Graph throttled them (429, 503).

## Not done

- Gmail metadata in HTTP batches of 50: io-gmail 0.4 sends no batch request; the reads go one by one, paced.
- IMAP `ENVELOPE` and `INTERNALDATE` are not fetched: the header fields carry everything `ENVELOPE` does, read the way the body is, and a received date is never stored.
- `Retry-After` outside a Graph batch, and the Google error `reason`: unchanged from `sync-safety-fixes`.
- Live Graph and Gmail runs of the new listings: the live suites compile, they need the test tenants' secrets.
