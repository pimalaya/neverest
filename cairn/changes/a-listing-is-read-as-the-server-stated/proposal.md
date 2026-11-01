---
cairn: change
id: a-listing-is-read-as-the-server-stated
status: landed
created: 2026-10-30
---

# A listing is read as the server stated

## Why

The done half of connector-listings-fail-rather-than-shrink (pimdir stable-release review, 2026-10-30, report-neverest.md "Connectors"), split out so the log says what landed: each hole read a listing as smaller or more final than the server stated, which a complete round turns into deletes, or stalled a sync for good.

- IMAP read a `SELECT` answer without `EXISTS` as an empty mailbox (src/imap/backend.rs, `exists.unwrap_or(0)`): a complete round of nothing deleted the mailbox locally.
- An href answering 404 in every multiget blocked the sync of its collection for good (src/offline/remote.rs, `hydrate_batch`), and Google Calendar and Google People reads did the same on a 404.
- Graph mail band rounds paged by an offset persisted across runs, with no closing pass (src/msgraph/client/mail.rs): deletes in between shifted the offset, a message was missed and a bound one inferred deleted.
- A mail listed without its meta was to fail its page (src/offline/remote.rs): found unreachable by construction, closed.

## What

- IMAP: a `SELECT` without `EXISTS` searches anyway, never reads as empty.
- A lone read answered 404 or 410 by a CardDAV, CalDAV, Google Calendar or Google People server is gone, so the batch answers it gone instead of failing, the next listing settling the member.
- A Graph band page resumes below the date the page before ended at, never at an offset.
- The meta-less entry stays as it is: no connector lists one.

## Not done

What connector-listings-fail-rather-than-shrink keeps: a 409 sync-token refusal, io-webdav's non-2xx member rows, Graph series beyond five years, the Graph shared-calendar read.
