---
cairn: change
id: a-body-is-downloaded-once
status: landed
created: 2026-10-13
---

# A body is downloaded once, or found gone once

Self-contained. Closes the three gaps `gmail-reads-in-paced-batches` left to the driver ("Not in scope"), and ports what the Android client learned in `body-run-loop` (android `cairn/changes/body-run-loop`, `e711f25`). Memory stays bounded: nothing below grows with the account beyond what phase 2 already plans.

## Why

- **A body gone fails the run.** A message deleted between its listing and its body read is left out of a batch: Gmail and Graph answer 404 inside it, IMAP's `UID FETCH` ignores a UID the mailbox no longer holds (RFC 9051 §6.4.9). The driver read a short batch as a server that did not answer, fetched the rest one by one, and the single read failed the run: Gmail and Graph on their 404, IMAP on an empty body (`fetch_body_stream` completes with nothing). A phase-1 batch that failed so was downloaded again by phase 2 of the same run, to fail there. Measured on Stalwart: three messages expunged while the first batch of a `-j 1` run downloads failed the run before this change.
- **A message under two labels downloads twice.** A Gmail id names one message under all its labels, but hydration plans each collection apart: a message in `INBOX` and a user label was planned, fetched and stored twice (one blob, two downloads). io-pimdir's own link dedup (`PimdirUpgrade`, by `Message-ID`) does not help: the driver downloads a batch before the upgrade runs, and the upgrade links by link id only where the summary's size agrees with the stored body's, which Gmail's `sizeEstimate` need not.
- **A resumed hydration asks Gmail for sizes it has.** The store records each owed body's size (the summary's), but `fetch_bodies` took ids alone: a body the run did not list (an interrupted run's backlog) cost a `minimal` read first, 5 units each. Graph's sizes likewise (`stated_sizes`).

## What

- **Gone is told apart** (`src/client.rs`). `Client::fetch_bodies` answers the ids the server said are gone; a lone read of one fails with the `Gone` marker. Gmail: a 404 in the size pass or the raw batches, or alone. Graph: a 404 in the size batch, the body batch or alone, mail, contacts and events. IMAP: a UID a completed `UID FETCH` answered nothing for, or a lone fetch that streamed nothing. DAV, Google Calendar and Google People answer none (unchanged). `hydrate_batch` answers `Hydrated { items, gone }`: a gone handle is answered, so it is not read alone, and a lone read failing with `Gone` joins it rather than failing the batch.
- **A gone body is owed nothing more** (`src/offline/driver.rs`, `GoneBodies`). The run records it per collection; no later batch of the run asks for it (a phase-1 batch, phase 2's plans), and the run is complete. The store keeps the member as listed: an upgrade cannot drop it, and pimdir SYNC §5 drops a member on a listing alone (a round's absence or a vanished handle), so the next run's listing takes it away before its hydration plans it. A member a listing keeps naming but whose body keeps answering 404 is asked once a run, never in a loop.
- **One download for every label** (`Landing`, `Placed`, `Client::global_handles`). On a source whose handles are account-wide (Gmail), a batch first reads every collection of the source for its handles: a handle some collection holds a body for is served from that blob (`stored_item`: the header read back from the blob, the identity, summary and sort key derived as a fetch derives them); the others are downloaded once; each body lands in every collection holding its handle without one, one collection at a time under its gate. Phase 2 plans a handle once, under the first collection listing it, primed when any collection primes it. io-pimdir stores an object once per hash (`store_object` upserts), so each placement references the one blob.
- **Sizes from the store.** A batch carries each body's size (`HydrateBatch { collection, bodies: [(handle, size)] }`), handed to `fetch_bodies` as `StoredSize`. Gmail plans its raw batches by the run's own size, else the store's, a `minimal` read asking only for what neither knows; Graph likewise before its size batch.
- **Phase 2 lands through each connection's store handle**, writes serialised as in phase 1, so phase 1 and phase 2 share one landing (`Landing::land`).

## Not in scope

- DAV, Google Calendar and Google People bodies gone (their reads still fail on a 404): named by their listing, they are rarely owed a body later.
- A gone member dropped from the store at once: a body read alone is no listing, and pimdir offers no verb for it.
- Two connections of a source with account-wide handles fetching the same message at the same moment: Gmail keeps one connection.
- Measuring on a real Gmail account: the live suite (`tests/google.rs`) needs the test account's secret.
