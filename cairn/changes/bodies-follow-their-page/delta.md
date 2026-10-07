---
cairn: change
change: bodies-follow-their-page
---

# Delta

## ADDED Requirements

### Requirement: A landed page's bodies download while the next pages list
In the one-source sync, each time a page of a collection's listing has landed, its members still without a body SHALL be queued for download, in batches of the hydration batch size ordered by the download order the run uses; before the first page of a round an earlier run left open, every member of the collection still without a body SHALL be queued. A connection with no collection left to spine SHALL take the queued batch that comes first in the download order across the account, fetch it on its own connection and raise it to `Full` at once, so the first bodies of a large collection are readable before its last page is listed.

A batch SHALL be applied to a collection only where its sync is at rest: while its listing waits on the server, or once its spine has ended, never between a page's load and its write, which would write the placement back without its body. An interrupted run SHALL keep every page and every body it applied. What phase 1 did not download (a run over one connection, a batch that failed to download or to apply, which is warned about) SHALL be downloaded by phase 2, which reads the bodies still owed once phase 1 has ended, fetching none twice. A dry run SHALL queue nothing.

#### Scenario: The first body before the last page
- **GIVEN** a mailbox of 1,600 messages synced for the first time over four connections
- **WHEN** its first page of 500 has landed
- **THEN** bodies of that page are readable while the round is still open

#### Scenario: Interrupted with bodies
- **GIVEN** that run killed once a body was readable and the round open
- **WHEN** the next run syncs the mailbox
- **THEN** the pages and bodies the first run landed are kept, and the run fetches exactly the bodies the store lacks

## MODIFIED Requirements

### Requirement: The one-source sync runs as two account-wide phases
The one-source (retain) sync SHALL run as two phases across the whole account, not a per-mailbox loop, so the connection pool never idles at a mailbox boundary:

- **Phase 1, spine (parallel over mailboxes).** A work-stealing pool of workers,
each on its own connection *and* its own store handle, reconciles each mailbox's spine (pull + meta + itemize + push). The network overlaps across mailboxes; store writes serialise on the store's single-writer lock (the seam sanctions process-level serialization), and every collection is pre-created serially first so no worker races lazy creation. A worker with no mailbox left downloads the bodies of the pages the others have landed (see "A landed page's bodies download while the next pages list"), the spine of a mailbox holding it at rest for each apply.
- **Phase 2, hydrate and apply (one global pool).** Once phase 1 has ended, the bodies still owed (`handle`, size and date from the local mail summary) are chunked into
per-mailbox batches queued in the download order the caller chose (largest first by default, see below), and work-stolen across the connections through **one** queue: a worker finishing one mailbox's last batch immediately steals the next mailbox's (`select_cached` re-SELECTs across the boundary), so no connection idles at a mailbox edge. Bodies stream into the blob store. The worker that fetched a batch SHALL raise its items to `Full` before taking the next one, over a cache-backed remote serving that batch alone (a miss falls back to a real fetch on the worker's connection). The applies SHALL be serialised on one store handle, the same body possibly belonging to two mailboxes.

A body SHALL be readable from the store as soon as its batch is applied, not once the account has downloaded. A run that stops during either phase SHALL keep every batch it applied, so the next run's pull plan holds only what is still without a body; nothing is held in memory beyond the batches in flight. In phase 2 a fetch error SHALL stop the pool and fail the run; a batch that fails to apply SHALL be warned about and left for the next run.

A dry run stops after Phase 1 (reporting the pull plan, downloading nothing). Progress is the two phases: `Scanning mailboxes (k/M)`, then one global `Downloading n% (done/total)` over every body phase 1 left.

### Requirement: The caller chooses the download order
`sync --download-order <largest|newest>` SHALL choose the order the one-source sync downloads bodies in, in both phases; `largest` is the default. Under `largest`, bodies are ordered as before: by size, largest first, within each collection (in phase 1, within each landed page) and, batch by batch, across the account. Under `newest`, bodies SHALL be ordered by their date (the mail summary's instant, from the store meta), newest first, then chunked into batches that stay within one collection, the batches taken across the account by their newest body among those queued. A body without a date SHALL come after every dated one, in plan order. Neither order adds a server request.

#### Scenario: Today's mail first
- **GIVEN** an Inbox holding a large mail from last year and a small one from today
- **WHEN** `sync --download-order newest` runs
- **THEN** today's mail is fetched and readable first

### Requirement: Hydration may run concurrently, largest-first
Full-tier hydration SHALL fetch bodies in **batches**: one `UID FETCH <set> (UID BODY.PEEK[])` streaming K bodies (`BATCH_SIZE`, default 64) in a single response, so N bodies cost ~N/K round trips per connection rather than one round trip per message. Each message is routed to its own streaming sink by the **UID on its own FETCH line**, so an out-of-order server response still lands correctly; a body line without a parseable UID SHALL fail the batch so the caller falls back to per-message fetches rather than misroute. In the one-source sync, hydration follows the listing page by page on the connections it leaves idle, then ends in one account-wide phase (see the two phases above), each batch applied as it lands: bodies are ordered **largest-first** globally by default (or newest first on request, see the download order) using each item's size from the store meta (no size probe), chunked into per-mailbox batches, biggest first, and work-stolen across the pool over one queue with no per-mailbox barrier; the cross-source copy path, lacking sizes, falls back to UID order. On any batch error the fetch SHALL fall back to per-message fetches; content-addressing makes the partial retry idempotent. The pool is **persistent**: connections are opened up front and kept for the run, so their auth is paid once, not per batch. The budget defaults to 4, is configurable per account (`connections`) and overridable by a `sync --connections` flag, and SHALL stay under the backend's per-account connection cap. Body bytes stream lock-free into the blob store; the engine serialises the index write on the single-writer store afterwards. The largest-first order takes its sizes from the store meta, never a server size probe.
