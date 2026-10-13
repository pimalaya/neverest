---
cairn: log
change: a-graph-round-lists-its-head
date: 2026-10-13
---

# A Graph mail round lists its head

A large Microsoft 365 folder keeps its first round open for many runs, and a message arriving meanwhile showed only once the round closed. A run resuming the round now lists its band head first (pimdir `round-head`, io-pimdir `07bd316`): the messages Graph received since two days before the round opened, so the arrival lands before the round's next page; the round's last page lists them again after its own rows, so what is still there is kept when the round closes and what went is found absent.

## What landed

- src/client.rs: `Client::heads` answers `PimdirHeads::BAND` for Graph mail.
- src/msgraph/client/mail.rs: `head`, the band head by `receivedDateTime` from the round's start less two days (`received_floor` reads RFC 3339 and SQLite's forms), members bound by their flags and others by their summary, a delta head and an unreadable instant answered none; `relist_head`, the last page of a whole-folder round and of a band round listing the head again after its own rows; the pass reading a member the head named by its summary; the instant kept per folder for the run (`Folder::head`); tests over the fake folder, each checked by a mutation.
- src/msgraph/client.rs: `received_first`, the listing by arrival with the size `$expand` and its 400 fallback; `GraphClient::scripted` and `scripted::{Server, Step}` for driver tests.
- src/offline/driver.rs: driver tests over a scripted Graph (vectors 65 and 66's cases); the throttle test of `graph_budget` deterministic (a request reaching the fake after the throttled answer is answered once the source gave up, and no connection sends a second one; the base failed one run in 160 under load).

## Left

- A live run on a Microsoft 365 test tenant.
- A delta head from the stored link on a widening band round.

## Capabilities moved

**sync**: the Graph mail folder requirement (band heads, the last page's obligation).
