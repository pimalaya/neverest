---
cairn: log
change: a-body-is-walked-once
date: 2026-10-14
---

# A body is walked once, by the store's write

Closes what the log of io-pimdir-linked-body-columns left under "Not done": neverest no longer walks a body's parts for the attachment mark. io-pimdir's `write::apply` walks each mail body entering the store once and states the mark and invitation, a body the account already holds taking those of a row holding it; neverest's own walk only computed a value `apply` overrode.

## What landed

- src/kind/mail.rs: `AttachmentMark` removed; `parse_body(raw, size)` leaves `attachment` unknown. src/kind/mod.rs, src/dav/client.rs: `Kind::parse_body` takes no mark.
- src/offline/remote.rs: `BodyScan` reduced to `HeaderScan` (the header block, capped by `HEADER_CAP`, no walk state); `HydrateSink::new` takes no kind; `stored_item` reads a stored blob up to its header block, 8 KiB a read, rather than to the end of its parts. `mark_tests` replaced by `scan_tests`: a streamed body, cut in 1, 7, 64 octets or whole, and a stored body are named, sized and sorted as the whole message, no mark stated.
- Effect: one walk of the parts per downloaded body instead of two (neverest's on the stream, io-pimdir's from the blob); a body served from the store to a second Gmail label is read for its header block alone instead of whole. Memory per body in flight: the header block alone.

## Cases checked

- A streamed body's blob is committed before the item reaches the write, and the collector cannot run meanwhile (io-pimdir `InFlight`, staging lock): the missing-blob branch of `apply` is moot for it.
- Every mail body reaches the store through `apply`; nothing in neverest reads the mark it derived.
- A body held only by rows from before neverest `0a4d7f8`, which state no mark: the new row now reads unknown like them, where neverest's walk gave the true mark. Left to io-pimdir if wanted.

## Capabilities moved

- sync: the streamed tier walks no part; the mark and invitation are the store's write's.
