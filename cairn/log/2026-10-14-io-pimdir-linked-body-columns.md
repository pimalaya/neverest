---
cairn: log
change: io-pimdir-linked-body-columns
date: 2026-10-14
---

# A linked body takes its held row's columns, a streamed one its invitation

io-pimdir `ca4c589` closes the two gaps the log of 2026-10-13 (a-body-stands-for-its-attachments) left under "Not done": `client::write::apply` now gives a placement linked to a body the account already holds (an IMAP COPY or MOVE, a second Gmail label, an upgrade's `LookingUp`) the body's size and the attachment mark and invitation of a row holding it (`held_mail_summary`), and walks an entering body once for its stand-ins, mark and invitation (`PimdirParts`), stated over what the writer read.

## What landed

- Cargo.toml, Cargo.lock: io-pimdir patched to `ca4c589`, no other package moved.
- No neverest code: every write reaches `apply`. neverest's own streamed walk (`AttachmentMark`, `BodyScan` in src/offline/remote.rs) still sets the mark, which `apply` now overrides with the same value (both are io-pimdir's `PimdirAttachmentWalk`); it could take `finish_parts` and drop its reading, left for later.
- tests/attachments.rs: the NOTE on a copy's mark now says it is the held row's.

## Not done

- neverest's own walk (`AttachmentMark`) duplicates io-pimdir's; taking `finish_parts` would drop it.

## Capabilities moved

- sync: a body linked from the store takes the held row's size, mark and invitation; a streamed body carries its invitation.
