---
cairn: log
change: a-body-is-downloaded-once
date: 2026-10-13
---

# A body is downloaded once, or found gone once

The three gaps `gmail-reads-in-paced-batches` left to the driver: a body gone on the server failed the run, a Gmail message under two labels downloaded twice, and a resumed hydration asked Gmail for sizes the store had. Ported from the Android client's `body-run-loop` (gone ids apart from failed ones).

## What landed

- src/client.rs: `Gone`, the marker a lone read of an item gone fails with (`Gone::is`); `StoredSize`; `Client::fetch_bodies` takes the sizes the store records and answers the ids the server said are gone; `Client::global_handles` (Gmail).
- src/gmail/client.rs: gone ids from the `minimal` and raw passes, a lone 404 as `Gone`; the run's size, else the store's, `minimal` for the rest. The scripted sessions move to a `scripted` test module the driver's tests share.
- src/msgraph/client.rs, contacts.rs, calendar.rs: gone ids from the size batch, the body batch and lone reads; `get_item_stream` marks a 404 `Gone`; the store's sizes before the size batch.
- src/imap/backend.rs: a UID a completed `UID FETCH` answered nothing for is gone (RFC 9051 §6.4.9, checked on Stalwart: an expunged UID is left out, the command `OK`); a lone fetch streaming nothing is `Gone`.
- src/offline/remote.rs: `hydrate_batch` answers `Hydrated { items, gone }`, a lone `Gone` joining `gone` rather than failing the batch; `stored_item` reports a stored blob as a fetch would, from its header.
- src/offline/driver.rs: `HydrateBatch { collection, bodies: [(handle, size)] }`; `GoneBodies` per run, read by phase 2's plans and by every batch; `Landing::land`, one landing for both phases (phase 2 now over each connection's store handle, writes serialised as in phase 1); `Placed` reads where an account-wide handle is owed across the source's collections, serves it from a body one holds, downloads the rest once and applies to each owing collection under its gate, one at a time; `hydrate_batches` plans a shared handle once, at its best tier; `hydrate_pool` takes one landing closure.
- Tests: Gmail and Graph scripted sessions (gone ids, a lone 404, store sizes sparing the size reads, a large stored size read alone); the driver over a scripted Gmail (a message under two labels downloaded once and landed under both, a gone body asked once in the run, no `minimal` read; a body held under one label serving another with no download); tests/hydration.rs on Stalwart (three messages expunged while the first batch downloads: the run completes, the next run's listing removes them, fetching nothing; it fails before this change).

## Not done

- DAV, Google Calendar and Google People bodies gone still fail their read.
- Not run on a real Gmail account.

## Capabilities moved

- sync: a body gone from the server is owed nothing more; a message under several labels is downloaded once; a body fetch counts a gone handle as answered; Gmail and Graph plan body reads by the store's sizes; phase 2 lands through each connection's store handle.
