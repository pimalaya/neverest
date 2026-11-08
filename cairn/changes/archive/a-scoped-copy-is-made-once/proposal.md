---
cairn: change
id: a-scoped-copy-is-made-once
status: landed
created: 2026-11-08
---

# A scoped copy is made once

## Why

A copy whose answer was lost (the server filed it, the client never recorded it: a session cut after the command, or a run stopped before its store write) stays staged and runs again. The Android app filed a second copy that way and now checks the target by `Message-ID` before copying (android cairn/log/2026-11-06-a-copy-is-not-made-twice.md).

Without a scope, neverest is safe: the target's next listing names the server's copy by its `Message-ID` and io-pimdir lands the pending create on it (pimdir SYNC §6). Under a date scope it is not:
- neverest drops every member dated below the scope from every page (`keep_in_scope`, src/offline/remote.rs), and an IMAP listing is narrowed by `SENTSINCE` before that, so the server's copy of an old message is never listed;
- io-pimdir holds back an out-of-scope create only when it has no origin (io-pimdir src/sync.rs at 5c18a02, `local.origin.is_none() && !self.opts.scope.contains(..)`), and a copy has one;
- the delivery searches nothing first: IMAP uploads the body (`copy_item` answers `None` for every backend but Graph mail, src/client.rs; `add_message_stream` in src/imap/backend.rs appends, then searches only to learn the UID), and Graph posts `message_copy`.

So the next scoped run pushes the copy again, and the target holds the message twice. Found by reading on 2026-11-08 (neverest d669915), then reproduced on Stalwart on 8574814: the target held the message twice.

## What

- A staged move under a scope has no gap on IMAP: its source half moves the member server side (`MOVE`), and the Stalwart test shows the target holding it once. On Graph its target half may deliver it as a copy (tests/msgraph.rs `a_graph_move_is_delivered_once`), which the search below covers.
- A mail create carrying an origin and a `Message-ID` is delivered only once its target is known not to hold it: IMAP searches the target by `UID SEARCH HEADER Message-ID "<id>"` and Graph lists the target folder filtered on `internetMessageId`; a message found there, bound by no item, is bound instead of filing another. A minted identity (a second copy of one `Message-ID`) does not search.
- Every such create searches, scope or not, rather than only one a scope excludes: the push knows no scope, and a garbled `Date` is in every scope for neverest yet below an IMAP `SENTSINCE`, so a gate on the date would miss it. Outside a scope the search finds nothing the listing has not landed, at one command a copy.
- Stalwart 0.16.27 matches no `HEADER Message-ID` search (not even `""`), so on it a lost copy is still filed twice; the regression test runs over the fake IMAP instead.

## Alternatives set aside

- io-pimdir holding a scoped copy as it holds an origin-less create: the copy would wait for a wider scope and never be made under this one.
- Keeping out-of-scope members that name a pending create in the listing: an IMAP listing narrowed by `SENTSINCE` never receives them.

## Compatibility

Not breaking. A scoped copy whose answer was lost no longer files a second one.
