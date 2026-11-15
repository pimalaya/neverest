---
cairn: change
id: a-body-stands-for-its-attachments
status: landed
created: 2026-10-13
---

# A body stands for its attachments, and carries their mark

Takes io-pimdir `14a8fca` (`stand-ins-as-the-body-enters`), and answers what its author noticed of neverest: a streamed body's summary left the attachment mark unknown.

## Why

- **Stand-ins.** pimdir STORAGE §14.3 has a message stand for each attachment by a file holding no body, referenced from the message, so a reader lists and searches attachments without opening a body. io-pimdir now records them in the write bringing the body in (`client::write::apply`), whatever wrote it. neverest has to take that revision, and every path landing a body has to reach that write.
- **The mark.** A body is streamed into the blob store and named by its header block (`HydrateSink`, then `kind::mail::parse_body`), which set `attachment` to `None`. The summary upsert writes the column as given, so the download overwrote the mark the listing had read (a top-level `multipart/mixed`, or Graph's `hasAttachments`) with `NULL`: a downloaded message read as of unknown attachment, where the walk of its parts decides it (Annex A.1). A body served from the store to a second Gmail label (`stored_item`) likewise.

## What

- io-pimdir patched to `14a8fca`; nothing breaks.
- Every body neverest lands reaches `write::apply`: neverest writes the store through io-pimdir alone (no SQL of its own), each sync, upgrade and mutation through the coroutine's `WantsWrite`, serviced by `PimdirSourceStore::write`, and the conflict and paired paths through `write` directly. Primed bodies, the bodies following a page, phase 2's, `Landing::land` (Gmail's shared landing) and the cached-fetch remote all go through it; a body found gone lands nothing.
- `kind::mail::AttachmentMark` wraps io-pimdir's `PimdirAttachmentWalk`; `BodyScan` feeds it beside the header capture, in the sink every fetched body streams through and in `stored_item`, which reads a stored blob as far as the scan needs. `Kind::parse_body` takes the mark, mail alone reading it.

## Not in scope

- A body io-pimdir links from the store without a fetch (an IMAP copy or move, `PimdirUpgrade`'s `LookingUp`) keeps the summary its listing read, mark and invitation alike: the engine's to fix, by carrying the held row's.
- The invitation of a streamed body stays unknown: io-pimdir has no streamed walk for it.
- Gmail and Graph bodies on a live account: the sink and `stored_item` are shared and tested, the stand-ins proven on Stalwart.
