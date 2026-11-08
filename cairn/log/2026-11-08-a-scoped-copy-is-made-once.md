---
cairn: log
change: a-scoped-copy-is-made-once
date: 2026-11-08
---

# A copy is made once

A copy whose answer was lost (the server made it, the store never recorded it) stays staged. Without a scope the target's next listing lands it by its `Message-ID`; under one, a message dated below the floor is never listed, io-pimdir holds back only origin-less creates, and the IMAP upload searched nothing first, so the next run filed the message again. Reproduced on Stalwart on 8574814: a message dated 2020 copied from `A` into `B`, `B` holding it already, synced under `30d`, left `B` holding it twice.

## What landed

- `ImapClient::find_message` (src/imap/backend.rs) runs `UID SEARCH HEADER Message-ID "<id>"`, the brackets keeping a substring from matching a longer id; `GraphClient::find_message` (src/msgraph/client.rs) lists the target folder with `$filter=internetMessageId eq '<id>'`, checked live on the Microsoft test tenant (a hit answers the message, a miss an empty page). `Client::find_message` answers `None` for every other backend.
- The copy path of the push (src/offline/remote.rs) searches the target before `copy_item` or the upload, for every create carrying an origin and a `Message-ID` but a minted one, and binds a hit no item of the side binds yet. The proposal gated the search on a scope excluding the `Date`; it runs whatever the scope, the push knowing none and a garbled `Date` being in every scope for neverest yet below an IMAP `SENTSINCE`.
- A staged move has no gap on IMAP, its source half moving the member server side: `a_scoped_move_whose_answer_was_lost_is_made_once` (tests/scope.rs, Stalwart) locks it. On Graph a move's target half may deliver as a copy, so it searches too: the live `a_graph_move_is_delivered_once` passed on the test tenant (four bounces, each landed once).
- Stalwart 0.16.27 matches no `HEADER Message-ID` search, not even `""`, while `HEADER Subject` and `HEADER From` work: the copy test could not pass there, so the regression test runs over the fake IMAP, which now honours that criterion (`a_copy_the_target_holds_out_of_scope_is_bound_not_filed_again`, failing without the search).

## Left

- On a server matching no `HEADER Message-ID` search, Stalwart included, a lost copy under a scope is still filed twice.
- The Gmail API backend uploads a copy too (`copy_item` answers `None` for it) and was not examined.

Capability moved: sync (a new requirement, "A copy is made once").
