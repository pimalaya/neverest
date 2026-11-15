---
cairn: change
id: an-open-imap-round-lists-its-head
status: landed
created: 2026-10-13
---

# An open IMAP round lists its head

## Why

A round stays open for many runs (a first sync stopped by an arrival or a gesture, resumed run after run), and its later pages list older mail: a message arriving meanwhile showed only at the first delta after the round closed. pimdir `round-head` (SYNC §4, §5, io-pimdir `07bd316`) has a run resuming a round list the round's head first; the base (`a3deafd`) plumbed it with every backend listing none, and never told the engine which heads a source lists.

## What (design)

- **Heads stated.** `Client::heads()` answers `BOTH` for an IMAP connection whose server advertised QRESYNC, `BAND` otherwise: CONDSTORE alone has no `VANISHED`, so no delta head. The capabilities are those the connection read when it opened, the primary connection being open when the pool is built. The driver hands them to the sync coroutine (`PimdirSync::heads`).
- **Delta head.** The QRESYNC `SELECT` from the checkpoint asked, shared with the delta (`qresync_delta`): what changed, its meta where the store does not bind it under that `UIDVALIDITY`, the vanished UIDs, the checkpoint now. Never a round page. No QRESYNC, no `UIDVALIDITY` or a modseq of 0: `NoHead`. A moved `UIDVALIDITY`: `HeadRejected`. SYNC §4 asks a moved handle space to fail and name a rekey; neverest names a rekey by the stored checkpoint's epoch moving (the driver's rebuild guard), so the head is refused, the round restarts, takes the new `UIDVALIDITY` from its first page and rebuilds the collection when it closes, as a round cursor meeting a moved `UIDVALIDITY` already does. Failing the head instead would fail every later run, the round's checkpoint never moving.
- **Band head.** `UID SEARCH SINCE` the day of `round_started_at` less two days (the `INTERNALDATE`), each UID's flags or meta read as a round page reads them; no checkpoint, no removal.
- **Band obligation.** A band head stamps nothing, so the round's last page must list again what a band head would list then. The round's start instant never reaches a page request, but the connector needs none: the cursor now carries the highest UID the round found when it opened (`(UIDVALIDITY, lowest, top)`, 12 bytes), and the last page of a round resumed from a cursor lists again, after its `SELECT`, every message of the scope above it. That is every arrival since the round opened, a superset of what a band head of the round lists that no page of the round lists (moves in included, which a band head by arrival misses), at the price of one `UID SEARCH`. It holds on QRESYNC servers too (harmless, a delta head having stamped them). A cursor from before (8 bytes) reads its top as 0: its last page lists the whole scope again, once. No io-pimdir change is needed.
- **Pages after a head.** A page's members are read by `UID FETCH` when the page lists, from the round's cached UID list: a UID gone since is answered nothing and left out, so no page lists a member a head stated removed.
- **Test hook.** A debug build reads `NEVEREST_IMAP_NO_QRESYNC` to take the server for one without QRESYNC (Stalwart offers it), so the Stalwart suite runs band heads; a release build never reads it.
