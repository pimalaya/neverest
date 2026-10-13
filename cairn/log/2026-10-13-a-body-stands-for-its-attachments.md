---
cairn: log
change: a-body-stands-for-its-attachments
date: 2026-10-13
---

# A body stands for its attachments, and carries their mark

io-pimdir `14a8fca` records a stand-in per attachment as a message body enters the store; neverest takes it, and a streamed body now carries the attachment mark its parts give, where it was left unknown.

## What landed

- Cargo.toml, Cargo.lock: io-pimdir patched to `14a8fca`, no other package moved.
- src/kind/mail.rs: `AttachmentMark`, io-pimdir's `PimdirAttachmentWalk` fed a chunk at a time; `parse_body` takes the mark.
- src/kind/mod.rs, src/dav/client.rs: `Kind::parse_body` takes the mark, mail alone reading it.
- src/offline/remote.rs: `BodyScan`, the header capture and the mark read together; `HydrateSink` scans every fetched body as it streams (IMAP, Graph, Gmail, DAV, Google), `stored_item` reads a stored blob as far as the scan needs. Unit tests `mark_tests`.
- tests/attachments.rs (Stalwart): stand-ins and marks after a primed sync, a paged one over four connections and one after the listing over one; nothing new on a second sync or for a copy in another folder; a body left owed by a run stopped after its listing records its stand-ins in the run landing it, an arrival's under `--bodies listed` first.
- Every write path checked: neverest writes the store through io-pimdir alone, each sync, upgrade and mutation by the coroutine's `WantsWrite` (`PimdirSourceStore::service` then `write`), the conflict and paired paths by `write`; all reach `client::write::apply`.

## Not done

- A body io-pimdir links from the store without a fetch (a copy, a move) keeps the summary its listing read, its mark included: io-pimdir's `PimdirUpgrade` (`LookingUp`) carries the listed summary, not the held row's.
- The invitation of a streamed body stays unknown.

## Capabilities moved

- sync: a landed body stands for its attachments; the streamed tier carries the attachment mark.
