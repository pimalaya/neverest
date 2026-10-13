---
cairn: change
change: a-body-is-downloaded-once
---

# Delta

## ADDED Requirements

### Requirement: A body gone from the server is owed nothing more
A body read the server answers gone SHALL NOT fail the run: a Gmail or Graph read answered 404 (in a batch, a size read, or alone), and an IMAP UID a completed `UID FETCH` answers nothing for (RFC 9051 §6.4.9), or a lone fetch of it that streams nothing. The run SHALL record the body gone for each collection owing it and SHALL NOT ask for it again: no later batch of phase 1 and none of phase 2. The store SHALL keep the member as it was listed, a body read being no listing (pimdir SYNC §5), so the next run's listing takes it away before that run plans its body. DAV, Google Calendar and Google People bodies are not covered.

#### Scenario: Expunged while the run downloads
- **GIVEN** an IMAP folder whose three smallest messages are expunged after a `-j 1` run listed them, while it downloads the largest bodies
- **WHEN** the run reaches their batch
- **THEN** the run completes, the three members stay without a body, and the next run's listing removes them, fetching nothing

#### Scenario: A Gmail body gone
- **GIVEN** a Gmail message deleted after it was listed
- **WHEN** its raw read answers 404
- **THEN** the run completes and no later batch of the run asks for it

### Requirement: A message under several labels is downloaded once
On a source whose handle names one item across its collections (a Gmail message id, the same under each label), a run SHALL download each body once: phase 2 SHALL plan a handle once, under the first collection in the run's order listing it, among the primed bodies when any collection primes it; a batch SHALL read where its handles are owed across the source's collections, serve a handle some collection already holds a body for from that body with no download, download the rest once, and raise each body to `Full` in every collection holding its handle without one, one collection at a time and only where its sync is at rest. The store holds one object per hash, which every placement references.

#### Scenario: `INBOX` and a user label
- **GIVEN** a Gmail message under `INBOX` and `Work`, its body owed in both
- **WHEN** the run downloads bodies
- **THEN** one raw read fetches it, and both placements hold the same body

#### Scenario: A label added later
- **GIVEN** a message whose body the store holds under `INBOX`, listed under `Work` by a later run
- **WHEN** that run downloads bodies
- **THEN** `Work` holds the same body and nothing is downloaded

## MODIFIED Requirements

### Requirement: A body fetch answers for every handle it was asked about
A batched body fetch SHALL be treated as complete only when it answers for every handle it carried, a handle the server answered gone being answered (see "A body gone from the server is owed nothing more"). A batch answering for fewer SHALL fall back to a per-item fetch for the remainder, exactly as a batch that errors already does, and SHALL report the shortfall; a per-item fetch the server answers gone SHALL be answered gone, never fail the batch.

A backend that answers for a subset is not a backend that failed, so nothing surfaces as an error; but the engine cannot tell an unanswered handle from an unasked one, so an unanswered handle is recorded nowhere and re-requested on every later run. That is a run that fetches a whole collection, stores nothing and reports itself in sync.

#### Scenario: A server answers for two cards out of sixty-four
- GIVEN a batched fetch of 64 handles that returns 2 bodies
- WHEN the run continues
- THEN the other 62 are fetched one by one and the shortfall is reported

### Requirement: An empty body is refused, never stored
A body of zero bytes SHALL fail the fetch, naming the item and its collection. No kind neverest syncs has an empty body: a message carries headers and a card carries at least its `BEGIN` and `END` lines. An IMAP UID a lone fetch answers with no message at all is no empty body: the message is gone.

An empty body stored is worse than a fetch that fails. Its link id is the digest of nothing, so every empty body a server returns resolves to the same identity; the second one collides with the first, the duplicate-link-id floor freezes it, and the collection stays frozen for every later run.

#### Scenario: A server returns zero-length cards
- GIVEN a server answering card bodies with zero bytes
- WHEN the run fetches them
- THEN it fails naming the first such card, rather than storing an item whose identity is the digest of nothing

### Requirement: Gmail reads in paced batches
A `gmail` source SHALL read message metadata, sizes and raw bodies through Gmail's HTTP batch endpoint, `gmail.batch-size` reads a batch (10 by default, 1 to 100) and at most `gmail.batches-in-flight` batches at once across the source's connections (1 by default), each batch paced by the units of all its reads. A read refused inside a batch for concurrency, rate or a missing answer SHALL be sent again at the head of the next batch after one to two seconds, one refused a per-minute quota likewise after two to five seconds and then the next minute; a read answered 404 SHALL be a message gone, never an error, and a body read so SHALL be answered gone to the driver; a daily quota, or a read refused six times, SHALL give the source up. A raw batch SHALL hold about 8 MiB of messages by their sizes, a message over 5 MiB being read alone, and each body SHALL be stored as its batch lands. The sizes SHALL be the run's own reads, else the ones the store records for the bodies it owes, a minimal read asking Gmail only for the sizes neither knows.

A run SHALL read a message once: a message carrying several synced labels SHALL be named, under the second label listing it, from the metadata the first read, its flags from that label's own flag listings. The run SHALL read the mailbox history once, with no label, and project it on each label.

#### Scenario: Ten reads a batch, the refused one put back
- **GIVEN** a Gmail label of 45 unbound messages, one refused for concurrency once
- **WHEN** its round page is read
- **THEN** five batches are sent one after another, 10, 10, 10, 10 and 6 reads, the refused message first in the second

#### Scenario: A message under two labels
- **GIVEN** a message filed under `INBOX` and a user label
- **WHEN** both are listed in one run
- **THEN** its metadata is read once

#### Scenario: A resumed hydration
- **GIVEN** bodies an interrupted run left owed, their sizes in the store
- **WHEN** a run downloads them
- **THEN** raw batches read them with no minimal read first

### Requirement: Graph mail bodies are fetched in JSON batches
A Graph mail source SHALL fetch the `Full` tier of an id set through JSON batches (`POST /$batch`) of raw MIME gets (`/messages/{id}/$value`), each body answered base64-encoded, matched to its message by request id whatever order Graph answers in, and written to the blob store as soon as it is decoded, before the next is read. A batch SHALL hold at most 20 gets and 4 MiB of the sizes stated for them (the MAPI message size, `$expand=singleValueExtendedProperties($filter=id eq 'Integer 0x0E08')`, which the band listing, the summaries read by id and the whole-folder delta ask for, the delta asking without it once Graph refused it, else the size the store records for the body it owes); a message stated over 1 MiB, or stating no size, SHALL be read alone by a plain get. When more than one message of a fetch states no size to the session nor to the store, their sizes SHALL be read first, 20 a batch, by id alone. A get Graph throttles (429 or a common 5xx), alone or with its whole batch, SHALL be sent again in a later batch once its own `Retry-After` is over (seconds or an HTTP date), else after a back-off doubling from a second, at most four times; one still throttled, or stating a wait over `sync --max-wait` (two minutes by default), SHALL be left unanswered, a throttle (429, 503) giving the source up until its wait is over. A get answered 404, for the size or the body, alone or in a batch, SHALL be answered gone. A get refused otherwise SHALL be left unanswered, so the per-item fallback fetches it and surfaces its error.

#### Scenario: Twenty bodies, one round trip
- **GIVEN** a Graph mail folder holding twenty messages of 50 KiB each whose bodies the store lacks
- **WHEN** a sync hydrates them
- **THEN** one batch request fetches all twenty, stored byte for byte as a direct get would

#### Scenario: Large mail is read alone
- **GIVEN** that folder with three messages of 2 MiB more
- **WHEN** a sync hydrates them
- **THEN** each of the three is read by its own get, and no batch asks for more than 4 MiB

### Requirement: The one-source sync runs as two account-wide phases
(The phase 2 bullet.) Once phase 1 has ended, the bodies still owed (`handle`, size and date from the local mail summary), but those the run found gone, SHALL be chunked into per-mailbox batches queued in the download order the caller chose, each mailbox's primed bodies first when the run primes, a handle of a source with account-wide handles planned once, and work-stolen across the connections through one queue. Each batch SHALL carry the sizes the store records, which the backend plans its reads with. The worker that fetched a batch SHALL raise its items to `Full` before taking the next one, through its own store handle, over a cache-backed remote serving that batch alone (a miss falls back to a real fetch on the worker's connection); the writes of the workers SHALL be serialised, the same body possibly belonging to two mailboxes.

## REMOVED Requirements

None.
