---
cairn: change
id: follows-storage-draft-17
status: active
created: 2026-11-23
---

# neverest follows storage-draft-17

The shared frame (target, who edits what, git, CPUs 0,1,16,17, locks, accounts, how a lane decides, what a plan holds) is /home/soywod/code/pimalaya/pimdir/cairn/changes/the-consumers-follow-draft-17/frame.md. This file keeps what is neverest's; the evidence is inventory.md, the steps tasks.md.

## Why

neverest owns the store the five CLIs share: it syncs it, drains the queue the others fill and builds the index they query. It moves to io-pimdir 7f16b27 (storage-draft-17, sync-draft-15, search-draft-17) with the other CLIs, or they refuse each other's store. The move itself is one test line; the rest is what the new engine lets neverest stop doing, a silent loss on its store surface, and spec text stale since HEAD's own pin.

## What

- A. The `[patch.crates-io]` git pin moves from b080e14 to 7f16b27 (requirement `0.19.1`), Cargo.lock on io-pimdir's source line alone (D1). Features unchanged (`client`; `search` and `indexer` under neverest's `search`): the domain features gate only the index extraction. The nix build already fetches git sources. MSRV 1.89. url 2.5.9 closes RUSTSEC-2026-0592, red at HEAD too (D3).
- B. No compile break; clippy clean on the 13 CI sets; one test names the crate's own draft (src/sync/driver/tests.rs:82).
- C. The refusal and its start over path stay as src/sync/driver.rs:322-338 words them: sync with the neverest that wrote the store until nothing is queued, move the store directory away, `neverest init`, `neverest sync`. No byte patch of `FORMAT_VERSION`.
- D. neverest's own SQL names no moved table.
- E. neverest refreshes the index and never queries it: the grammar, the sorts and the `pimdir` JSON contract do not reach it. Users meet the index rebuilt by the first refresh, a build cut by the 60 s deadline resumed with its merges held, 4,096-row batches (the 2 s refresh after a drain may overrun by one). The capped rekey park on any absence (io-pimdir since 2026-11-13) gains a neverest test and its spec sentence.
- F. Sync adoption. F1: a collection `PimdirSourceStore::settled_collections` names after its pull pass skips its push passes, every backend, pair and solo. F2: a settled collection whose source states no change is not spined at all: Gmail by its account history, already read once a run; DAV by the collection's `sync-token` in the home-set listing; IMAP only if io-imap can send `LIST-STATUS` (RFC 5819); Graph, Google Calendar and People state nothing account-wide. F3: Gmail states its message id as `server_object_id` (D4). COPYUID (`PimdirPushResult::assigned`, `LandPlacement`), `body_flush` and the 4,096-row write chunk are in HEAD's pin already.
- G. G1, silent loss, reproduced at HEAD and at 7f16b27: `sync --reset` without `--force` drops every local property (a `pimdir/label` among them) of the account's items; `pending_work` (src/cli/sync.rs:808) never counts them and the store drops a property with the last row of its seq. It now refuses, naming `--force` (D2). G2: `cargo deny` red. G3: spec drift (drafts, summary tables, capped rekey, unmet codes).
- H. Hot paths on the store surface: a no-op run reads each collection whole at least four times and lists a writable one twice (inventory H); first sync, incremental and the index refresh are measured too, HEAD against the move, then after F.
- I to L: spec fold, log, CHANGELOG, tests offline, on local servers and live, the cross-app check, the release order, report.md.

## Out of scope

- Gmail `remove` (delete or archive): a cross-app ruling not yet made.
- People and Graph contacts and calendars landing a round as one page (a stopped run starts over): still so at 99384ff, wire-side, no loss; a live check must show a page token outlives a run (D6).
- IMAP `EMAILID` for the listed variant of tests/moves.rs `a_move_into_a_message_stating_its_id_keeps_both`: imap-types and io-imap first.
- jmap-sources, the 409 sync token, the live checks of connector-listings-fail-rather-than-shrink and gpeople-batch-get, NEV-X2, the store-only flag reset: off this move's path, each waiting on a ruling, a live grant or another crate.

## Rekey and drain exposure (frame §3)

neverest rekeys IMAP alone, whose listings state known flags and no revision nor body: neither silent loss (R4, R1) is reached, nor R0, R2, R3, R7, R8 (inventory F). R5, R6, R6b and K4 are, each a duplicate rather than a loss; no mitigation keeps the rules, so their app tests wait for io-pimdir's fix (needs-io-pimdir.md 1).

## Decisions, defaults taken

- D1: Cargo.lock moves on one line; `cargo update -p io-pimdir` also flips five `windows-sys` edges, churn this move does not need.
- D2: the reset refusal over local properties is new to users; default refuse, `--force` dropping them, as the requirement's title promises.
- D3: url 2.5.9 lands here, the gate being red without it.
- D4: Gmail's message id is the server object id: account-wide and the API form of `X-GM-MSGID`.
- D5: the performance harness stays in the lane's scratch, its commands in journal.md; nothing committed.
- D6: People and Graph round paging parked until a live grant.

## Risks

- The five CLIs release on one io-pimdir revision or not at all: a draft-17 neverest and a draft-15 himalaya refuse each other's store.
- A wrong F1 or F2 skip delays a server change to a later run, never drops it; count gates guard both.
- A store moved away before its queue drained strands rows no server holds; the refusal and the CHANGELOG say so.
