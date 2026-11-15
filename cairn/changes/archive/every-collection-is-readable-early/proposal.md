---
cairn: change
id: every-collection-is-readable-early
status: landed
created: 2026-10-13
---

# Every collection is readable early

## Why

A first sync of a large account reads well in its Inbox and badly everywhere else. Reported by MOA (`docs/plan/first-sync.md`) and measured against the Android client's strategies (`short-first-sync`, `mail-download-window`, `docs/performance.md`):

- Collections are taken in the order of their provider ids, alphabetical: `Archive` before `INBOX`, the trash anywhere. The roles the run records are not read.
- Each connection lists one collection to its last page before taking the next, so a folder waits behind every page of a larger one; with one connection (Microsoft Graph, Gmail, DAV), no body downloads before the whole account is listed, phase 1 feeding bodies only to spare connections.
- `--download-order newest` orders the whole account at once: a folder whose mail is older than the Inbox's gets nothing until the Inbox's backlog is done.
- A Graph source opens one connection where Graph serves four requests at once a mailbox.
- A targeted run (`--include-collection`, what a caller runs when a watcher reports an arrival) downloads every body the collection still owes, the new mail waiting behind thousands of older ones an interrupted first sync left.
- The throttling bound (two minutes) is a constant, though a caller running syncs under its own deadline wants a shorter one.

Neverest's role stays to sync everything in a run: each change below orders or paces the work, none caps it. Memory stays bounded: no buffer grows with the connection budget beyond one batch a connection.

## What (design)

- **Collections by role** (a1). The one-source sync takes its collections in the order of the roles recorded this run: the inbox (or a default calendar or address book), sent, drafts, every other collection by name, the all-mail archive (which repeats the others), junk, the trash last. Batches tied in the download order go to the collection taken first.
- **Per-backend connection budget** (a3). IMAP keeps `connections` (account, `--connections`, default 4). Microsoft Graph opens its `pool-size`, else 3, never more than `connections`. Gmail keeps one until its source sends one batch at a time across the account (another change; the budget's Gmail arm is the one line to widen then). Google Calendar, People, CalDAV and CardDAV keep one. Every connection is its own client with its own caches (Graph's folder ids, listed rows, Drafts id); what they share (the throttle, the downloaded-bytes counter) is already shared across threads; pushes stay serialised; and the page feed now works over Graph connections as over IMAP ones.
- **Priming** (a2), `sync --prime <N>`. Under a positive N, phase 1 runs two passes. The first takes every collection, in the run's order: a round lists its first page (the newest) and pauses at rest before its second, which leaves the store as a run killed there would (the round open at its cursor, already specified and tested); a round an earlier run left open pauses before asking anything; a collection whole runs its delta to the end. Right after, the collection's N newest owed bodies download ahead of any other work, on any connection, so one connection reads the first page and its newest mail of every collection before any second page; the rest of what it owes joins the feed when connections are spare. Once every first page and every primed body is done, the second pass fills each paused collection, in the run's order: its round resumes from the cursor, the pages feed the idle connections as before, and its pushes, staged work and coverage are done as for any collection (a listing pushes nothing until its round closes, so nothing is deferred that was not before). Phase 2 then downloads each collection's N newest owed bodies ahead of the rest, then the rest in the download order. The fetches a paused spine owes are carried to the spine that fills it, so the report names each fetch once. **Default**: 50 under `--download-order newest` (what a reader sees of a mailbox before scrolling; the Android client's first chunk), 0 under `largest`, the fastest whole run, which priming would only slow by a store read a collection. `--prime 0` turns it off. A dry run never primes.
- **Bodies of what was listed** (a4), `sync --bodies <all|listed>`, `all` by default. Under `listed`, the run downloads only the bodies of members its pages landed (arrivals, changed members); the rest stay owed for a later run. A round an earlier run left open resumes as before; only its bodies wait.
- **Throttling bound** (a5), `sync --max-wait <SECONDS>`, 120 by default: the longest single wait on a throttling provider (a stated `Retry-After` or the back-off), past which the source gives up for the run. Resolved once into each source's throttle (`Account::resolve_waiting`), shared by its connections; Graph's body batches read it for the wait each throttled get states, and Gmail's batch pauses for theirs, instead of their own two minutes.
- **Where nothing is listed.** `--prime`, `--bodies` and `--download-order` are refused beside `--intents-only` and `--declare-only`, which list nothing; `--max-wait` still bounds the sends an intents run makes.

## Out of scope

- An io-pimdir change: a new message arriving while a round is open is still listed only once the round closes (a delta), whatever the order.
- A configuration key for `prime`, `bodies` and `max-wait`: flags are enough for the caller that asked.
- Gmail beyond one connection, waiting for its account-wide batch gate.
