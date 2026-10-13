---
cairn: log
change: every-collection-is-readable-early
date: 2026-10-13
---

# Every collection is readable early

A first sync read well in its Inbox and badly elsewhere: collections came in provider-id order, each connection listed one collection to its last page before the next, one connection (Graph, Gmail, DAV) downloaded no body before the whole account was listed, and a targeted run's arrival waited behind the backlog. Each change orders or paces the run; none caps it.

## What landed

- src/offline/mod.rs: a `Pager` answers `Rest::Go` or `Rest::Pause` at rest, told the listing about to be asked; `run_verb_paged` returns `Paged::Done` or `Paged::Paused` with the coroutine, whose partial report the driver reads.
- src/offline/driver.rs:
  - `run_order`/`role_rank`: collections by role (inbox or default, sent, drafts, others by name, all, junk, trash), roles read before they are declared.
  - `connection_budget`: IMAP `connections`, Graph `pool-size` or `GRAPH_CONNECTIONS` (3) within `connections`, Gmail and the rest 1 (the Gmail arm is the hook for its account-wide batch gate).
  - `Work` replaces `HydrateFeed`: one scheduler for phase 1, handing out primed batches, first-pass spines, fills (once the first pass and its primed bodies are over) and other batches, ties by collection rank. `PageFeed` pauses a round before its second page under priming; `prime_bodies` queues a collection's N newest owed bodies after its first pass, the rest beside the pages' bodies.
  - `collection_spine` returns `Spined`: a paused spine itemizes what it pulled and carries its owed set to the fill, which itemizes the fetches, the staged work, pushes and reports coverage.
  - `hydrate_batches` puts each collection's N newest owed bodies first in phase 2.
  - `Pacing` (`order`, `prime`, `bodies`, `max_wait`), `Bodies`, `Listed`, `DEFAULT_PRIME` (50).
- src/throttle.rs, src/account.rs, src/msgraph/client.rs: `Throttle::waiting`/`max_wait`, `Account::resolve_waiting`; Graph's batch rounds read the source's bound instead of `BATCH_RETRY_MAX_WAIT`.
- src/cli/sync.rs: `--prime <N>`, `--bodies <all|listed>`, `--max-wait <SECONDS>`. They are refused beside `--intents-only` and `--declare-only`.
- Rebased on the Graph wire rework, Gmail batches and `--intents-only`: `--max-wait` bounds the wait a throttled get inside a Graph batch states and Gmail's batch pauses; Gmail stays on one connection (its batches pass one at a time across the source, but a listing page or a lone read on a second connection would run beside a batch, unmeasured).
- A fake Graph in the driver's tests, over the session connector: a source's three connections serve side by side and never four, each body asked once; a ten-minute throttle on one stops every connection; `--max-wait` bounds a batch's wait.
- Unit tests in the driver, the throttle and the account; tests/hydration.rs (priming killed and resumed, each fetch reported once with timings, `--bodies listed`, the order test pinned to `--prime 0`), tests/moves.rs (a move synced by runs naming one collection each, both ways).

## Measured

Local Stalwart, three folders of 1,100 small mails, `--download-order newest`, first sync (time to the first readable body / to every folder's first page / whole run):

- `-j 1 --prime 0`: 3.9 s / 3.0 s / 7.3 s
- `-j 1 --prime 50`: 0.6 s / 1.7 s / 7.4 s
- `-j 4 --prime 0`: 1.1 s / 1.5 s / 6.5 s
- `-j 4 --prime 50`: 1.1 s / 1.7 s / 6.4 s

## Capabilities moved

- sync: collections are taken by role; every collection's newest mail comes first on request; a run may download only what it listed; per-backend connection budgets; the throttling bound is the run's.
