---
cairn: change
change: every-collection-is-readable-early
---

# Delta

## ADDED Requirements

### Requirement: Collections are taken by role
The one-source sync SHALL take its collections in the order of the roles the run recorded for them (or, a role lookup failing, of those its listing states): the inbox or a default calendar or address book first, then sent, drafts, every collection without one of these roles by name (ASCII case aside, then by id), the all-mail archive, junk, and the trash last. Phase 1 SHALL hand its collections to the connections in that order, and a batch of bodies tied with another in the download order SHALL go first when its collection comes first.

#### Scenario: The inbox before the archive
- **GIVEN** an IMAP account holding `Archive`, `INBOX`, `Sent` and `Trash`
- **WHEN** it is synced over one connection
- **THEN** it lists `INBOX`, `Sent`, `Archive` and `Trash`, in that order

### Requirement: Every collection's newest mail comes first on request
`sync --prime <N>` SHALL, under a positive N, run phase 1 in two passes. The first SHALL take every collection in the run's order: a round SHALL list its first page and stop at rest before asking its second, the store left as a run killed there leaves it (the round open at its cursor); a round an earlier run left open SHALL stop before asking anything; a collection whose rounds are closed SHALL run its delta to the end. Right after, the collection's N newest bodies still owed (by the mail summary's date) SHALL download ahead of any other work, on whichever connection is free, the others it owes joining the feed of the idle connections. The second pass SHALL start once every first-pass spine and every primed body has ended, and SHALL list each stopped collection to its end, in the run's order, from its cursor; its pushes, its staged work and its coverage are done then, as for a collection listed whole. Phase 2 SHALL download each collection's N newest owed bodies ahead of every other, then the rest in the download order. The report SHALL name each fetch once, a stopped spine handing the next one what it owed. The default SHALL be 50 under `--download-order newest` and 0 under `largest`; `--prime 0` lists each collection whole in turn; a dry run never primes. Priming orders a run and SHALL NOT shorten it: a run that ends has listed every page and downloaded every owed body.

#### Scenario: One connection, three large folders
- **GIVEN** three folders of 1,100 mails each, synced for the first time over one connection under `--download-order newest`
- **WHEN** the third folder's first page lands
- **THEN** the first two hold their first page alone, and each folder's 50 newest mails are readable before any second page lists

#### Scenario: Killed once primed
- **GIVEN** that run killed once each folder's 50 newest mails are readable
- **WHEN** the next run syncs the account
- **THEN** every round closes and each folder holds each of its mails once, with its body

### Requirement: A run may download only what it listed
`sync --bodies <all|listed>` SHALL choose which owed bodies a one-source run downloads; `all` is the default. Under `listed`, the run SHALL download, in both phases, only the bodies of members its own pages landed, every other owed body staying owed for a later run. `--prime`, `--bodies` and `--download-order` SHALL be refused beside `--intents-only` and `--declare-only`, which list nothing.

#### Scenario: An arrival behind a backlog
- **GIVEN** an Inbox an interrupted first sync left owing thousands of bodies, and a new mail
- **WHEN** `sync --include-collection INBOX --bodies listed` runs
- **THEN** the new mail's body is downloaded and the backlog is not

## MODIFIED Requirements

### Requirement: The one-source sync runs as two account-wide phases
The one-source (retain) sync SHALL run as two phases across the whole account, not a per-mailbox loop, so the connection pool never idles at a mailbox boundary:

- **Phase 1, spine (parallel over mailboxes).** A work-stealing pool of workers,
each on its own connection *and* its own store handle, reconciles each mailbox's spine (pull + meta + itemize + push), the mailboxes taken in the run's order (see "Collections are taken by role"), in one pass or, primed, in two (see "Every collection's newest mail comes first on request"). The network overlaps across mailboxes; store writes serialise on the store's single-writer lock (the seam sanctions process-level serialization), and every collection is pre-created serially first so no worker races lazy creation. A worker with no mailbox left downloads the bodies of the pages the others have landed (see "A landed page's bodies download while the next pages list"), the spine of a mailbox holding it at rest for each apply.
- **Phase 2, hydrate and apply (one global pool).** Once phase 1 has ended, the bodies still owed (`handle`, size and date from the local mail summary) are chunked into
per-mailbox batches queued in the download order the caller chose (largest first by default, see below), each mailbox's primed bodies first when the run primes, and work-stolen across the connections through **one** queue: a worker finishing one mailbox's last batch immediately steals the next mailbox's (`select_cached` re-SELECTs across the boundary), so no connection idles at a mailbox edge. Bodies stream into the blob store. The worker that fetched a batch SHALL raise its items to `Full` before taking the next one, over a cache-backed remote serving that batch alone (a miss falls back to a real fetch on the worker's connection). The applies SHALL be serialised on one store handle, the same body possibly belonging to two mailboxes.

A body SHALL be readable from the store as soon as its batch is applied, not once the account has downloaded. A run that stops during either phase SHALL keep every batch it applied, so the next run's pull plan holds only what is still without a body; nothing is held in memory beyond the batches in flight, one a connection. In phase 2 a fetch error SHALL stop the pool and fail the run; a batch that fails to apply SHALL be warned about and left for the next run.

A dry run stops after Phase 1 (reporting the pull plan, downloading nothing). Progress is the two phases: `Scanning mailboxes (k/M)`, then one global `Downloading n% (done/total)` over every body phase 1 left.

### Requirement: A landed page's bodies download while the next pages list
In the one-source sync, each time a page of a collection's listing has landed, its members still without a body SHALL be queued for download, in batches of the hydration batch size ordered by the download order the run uses; before the first page of a round an earlier run left open, every member of the collection still without a body SHALL be queued (when the run primes, the first pass queues them instead). A connection with no collection left to spine SHALL take the queued batch that comes first in the download order across the account, ties going to the collection the run takes first, fetch it on its own connection and raise it to `Full` at once, so the first bodies of a large collection are readable before its last page is listed. This holds over every backend's connections, Microsoft Graph's as IMAP's.

A batch SHALL be applied to a collection only where its sync is at rest: while its listing waits on the server, or once its spine has ended, never between a page's load and its write, which would write the placement back without its body. An interrupted run SHALL keep every page and every body it applied. What phase 1 did not download (a run over one connection that does not prime, a batch that failed to download or to apply, which is warned about) SHALL be downloaded by phase 2, which reads the bodies still owed once phase 1 has ended, fetching none twice. A dry run SHALL queue nothing.

#### Scenario: The first body before the last page
- **GIVEN** a mailbox of 1,600 messages synced for the first time over four connections
- **WHEN** its first page of 500 has landed
- **THEN** bodies of that page are readable while the round is still open

#### Scenario: Interrupted with bodies
- **GIVEN** that run killed once a body was readable and the round open
- **WHEN** the next run syncs the mailbox
- **THEN** the pages and bodies the first run landed are kept, and the run fetches exactly the bodies the store lacks

### Requirement: Hydration may run concurrently, largest-first
Full-tier hydration SHALL fetch bodies in **batches**: one `UID FETCH <set> (UID BODY.PEEK[])` streaming K bodies (`BATCH_SIZE`, default 64) in a single response, so N bodies cost ~N/K round trips per connection rather than one round trip per message. Each message is routed to its own streaming sink by the **UID on its own FETCH line**, so an out-of-order server response still lands correctly; a body line without a parseable UID SHALL fail the batch so the caller falls back to per-message fetches rather than misroute. In the one-source sync, hydration follows the listing page by page on the connections it leaves idle, then ends in one account-wide phase (see the two phases above), each batch applied as it lands: bodies are ordered **largest-first** globally by default (or newest first on request, see the download order) using each item's size from the store meta (no size probe), chunked into per-mailbox batches, biggest first, and work-stolen across the pool over one queue with no per-mailbox barrier; the cross-source copy path, lacking sizes, falls back to UID order. On any batch error the fetch SHALL fall back to per-message fetches; content-addressing makes the partial retry idempotent. The pool is **persistent**: connections are opened up front and kept for the run, so their auth is paid once, not per batch. The budget is per backend and SHALL stay under the backend's per-account cap: IMAP takes `connections`, 4 by default, configurable per account and overridable by a `sync --connections` flag; Microsoft Graph takes its `pool-size`, else 3, never more than `connections` (Graph serves four requests at once a mailbox, a `$batch`'s inner requests among them); Gmail, Google Calendar, Google People, CalDAV and CardDAV keep one. Each connection is its own client: what one caches (Graph's folder ids, listed rows, Drafts id) serves it alone, and what they share (the throttle, the downloaded-bytes count) is shared across threads. Body bytes stream lock-free into the blob store; the engine serialises the index write on the single-writer store afterwards. The largest-first order takes its sizes from the store meta, never a server size probe.

### Requirement: A throttled provider is waited out, then named
A request Microsoft Graph, Gmail, Google Calendar, Google People, CalDAV or CardDAV answers with 429, or with 503 for a request that creates nothing, or Google with a 403 rate limit, SHALL be sent again after the wait the provider states, else after a bounded exponential back-off with jitter. Google's refusals SHALL be read by their reasons and wording: a burst (a concurrency 429, a per-second rate, a 503) SHALL wait one to two seconds; a per-minute quota, whose answer names the minute (`Units per minute per user`) whatever its reason, two, three, four then five seconds, then the next minute; a daily quota SHALL give the source up until the next midnight in America/Los_Angeles. Past the bound, or on a single wait longer than `sync --max-wait <SECONDS>` (120 by default), the source SHALL give up: no connection opened from it sends anything until the wait is over, what the run already wrote stays, the report lists the source under `throttled` with `until` (RFC 3339 UTC), and the run is incomplete (exit 3). Graph's body batches and Gmail's batch pauses SHALL wait no longer than that bound.

#### Scenario: A server answering 503
- **GIVEN** an account whose CalDAV source answers every request 503
- **WHEN** it is synced
- **THEN** the run exits 3 and `throttled` names `caldav` with an `until`

#### Scenario: Gmail's per-minute quota
- **GIVEN** a Gmail read answered 403 `Quota exceeded for quota metric 'Total Query Cost' and limit 'Units per minute per user'`, reason `rateLimitExceeded`
- **WHEN** it is synced
- **THEN** the read is sent again two seconds later, rather than failing the run


## REMOVED Requirements

None.
