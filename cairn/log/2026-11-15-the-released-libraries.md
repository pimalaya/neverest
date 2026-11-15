---
cairn: log
change: the-released-libraries
date: 2026-11-15
---

# The released libraries

Owner order of 2026-11-15: every app builds on released libraries, no `[patch.crates-io]` left.

## What landed

- pimalaya-cli and pimalaya-config 0.5, io-gcal 0.5, io-gmail 0.9, io-gpeople 0.8, io-imap 0.11, io-webdav 0.9, all from crates.io; the patch section is gone. The lock moved nothing else (mime-rs 0.2 arrives with io-imap).
- io-imap 0.11 keeps the items of a FETCH line that decode and names the line in `ImapOutput::partial` rather than `skipped`. `rows_of` and `data_of` (src/imap/backend.rs) count those lines with the skipped ones, so a listing page holding one still fails rather than state its message removed; `a_listing_row_that_does_not_decode_fails_the_page` caught it.

## Left

- io-pimdir 0.12 still depends on mime-rs 0.1, so the tree holds mime-rs 0.1 and 0.2 until io-pimdir moves.
- `data_of` callers now read the decoded items of a partly decoded line, where the line was absent; none was checked one by one.

No capability moved.
