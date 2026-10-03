---
cairn: change
id: a-move-is-delivered-once
status: active
created: 2026-10-03
---

# A move is delivered once

Self-contained: a session with no prior context can take it from here.

## Why

A move staged through the store lands twice in its target on an IMAP source. Reproduced on 2026-10-03 with the released neverest 0.3.0 and himalaya 2.2.1, against the local Stalwart of `./tests/stalwart2.sh` (server A, IMAP on :143), on a fresh store:

```sh
neverest init -a c && neverest sync -a c
himalaya message move -a c --to imap/Drafts 1   # pimdir backend
neverest sync -a c
```

The run reports `delete item 1 in INBOX on imap` and `add item relay-1@pimalaya.org in Drafts on imap`, and the server's Drafts then holds the message under two UIDs, which the next sync stores as two items, the second minted `dup:<hint>#<handle>`.

pimdir SYNC §5 has a move deliver by either half: the target's create by copy or upload, the source's remove by relocating into its destination, and "Creates wait for the probes" holds the create back while the target holds a probe that may be the relocated member. Here both halves deliver. The likely order: every collection is enumerated first, then pushed collection by collection; INBOX's remove relocates the message with IMAP MOVE, and Drafts, enumerated before the relocation, holds no probe for it, so its create uploads a second copy.

Any frontend that archives by moving (MOA does, through himalaya) duplicates every message it archives.

## What

Make one of the two halves stand down when the other delivered in the same run. Leads, unverified:

- After a relocating `Remove` is accepted, re-enumerate the destination (or treat the relocation's new UID, `COPYUID` / `MOVE` response, as a probe there) before deriving the destination's `Add`.
- Or push a collection's removes with destinations before any create whose origin or identity they relocate, so the create meets the relocated member.

## Check

An ignored live test beside tests/relay.rs: seed one message, stage a `move` through a producer, sync once, and assert the target holds it once, on the server and in the store.
