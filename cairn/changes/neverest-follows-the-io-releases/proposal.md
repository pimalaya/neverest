---
cairn: change
id: neverest-follows-the-io-releases
status: landed
created: 2026-11-13
---

# neverest follows the io-* releases

## Why

Every family library neverest links was released on 2026-11-13, each breaking, after the family audit of the same day: io-sasl 0.2.0, io-http 0.7.0, io-imap 0.9.0, io-smtp 0.8.0, io-webdav 0.8.0, io-gcal 0.4.0, io-gmail 0.7.0, io-gpeople 0.7.0, io-msgraph 0.8.0, and io-oauth 0.6.0 for the live suites. neverest was two releases behind on io-gmail and io-smtp and three on io-oauth, and held io-http and io-msgraph by git pins whose commits these releases carry.

The audit also recorded defects on neverest's own side of four of them, which the releases give the means to close:

- IMAPB-S1 (high): after any error of a batched body fetch, neverest read the batch's messages one by one on the same connection. A batch that failed in the middle of its answer leaves the rest of that answer on the connection, which the next commands read as their own.
- The SMTP hand-over built its envelope by hand, unchecked, and sent its own `RSET` after a refusal, which io-smtp 0.8 sends itself.
- A Google contact update built from a vCard replaced each masked field with the vCard's projection of it, erasing what a vCard cannot show (GPEOPLE-02, fixed in io-gpeople by `merge_unprojected`, which a consumer has to call).
- A DAV body that is no whole multistatus parsed as a shorter listing (DAV-01, fixed in io-webdav 0.8, which fails it): neverest has to read that failure as a failed listing.

## What

- Every family requirement moves to today's release; the `[patch.crates-io]` pins of io-http and io-msgraph are removed, the pimalaya-cli pin kept as it is.
- A batched body fetch that errors is read item by item on the connection it failed on only when an IMAP server refused it with a tagged `NO` or `BAD`; after any other failure the IMAP connection is dropped and opened again first.
- An IMAP answer a listing is read from, holding a line that did not decode (io-imap 0.9 hands those over, where 0.8 dropped them without a word), fails the listing; the other answers warn. Server alerts are shown.
- The SMTP envelope goes through io-smtp's validating constructors; neverest sends no `RSET` of its own; a `421` reply loses the channel; the write of io-smtp's `RSET` after a refused `DATA` command is not counted as the data.
- A Google contact update goes through `merge_unprojected`; a property the stash cannot hold is named in a warning.
- The Google Calendar throttle reads the reasons io-gcal 0.4 keeps of an error.
- Tests pin that a DAV answer that is no whole multistatus fails a listing, and that a delete answered 207 with a failed member is not done.

## Not in this change

- What an undecodable line means in a `SELECT`, `LIST` or `STATUS` answer: warned of, the answer still read (see the log).
- The URLs neverest builds itself for Graph (the body, size and summary batches, `message_gone`, an event action) still splice the id as it is.
- The stream watcher of src/msgraph/client/wire.rs, which io-msgraph 0.8's `retry_after`, `keep_alive` and response headers could replace.
