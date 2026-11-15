---
cairn: change
change: scoped-mail-sync
---

# Delta

## ADDED Requirements

### Requirement: A mail sync lists within a scope on the `Date`
An account MAY bound its mail by `item.filter.since`, and a run by `sync --since`, which overrides it: a duration back from today (`30d`, `12w`, `6mo`, `1y`), taken to the start of its UTC day so the runs of one day list one scope, a date at midnight UTC, or an RFC 3339 instant. A mail collection SHALL then sync under the scope from that floor (pimdir SYNC §5): a listed message whose `Date` lies below it SHALL be left out of the page, a message with no usable `Date` SHALL be in every scope, and an explicit removal SHALL apply whatever the date. What a scope leaves out SHALL NOT be deleted, neither in the store nor on the server. A scope SHALL be refused, by the key or flag it came from, when an endpoint the run syncs holds contacts or calendars.

#### Scenario: An old message received today
- **GIVEN** a mailbox holding, all appended today, a message dated 2020, one dated in the future and one with no `Date`
- **WHEN** it is synced under `item.filter.since = "30d"`
- **THEN** the store holds the future and the undated messages and not the one dated 2020

#### Scenario: A narrower scope deletes nothing
- **GIVEN** that mailbox synced with `--since 2019-01-01`, then under `30d` again
- **WHEN** the run ends
- **THEN** the message dated 2020 is still in the store and on the server, and once deleted on the server the next run removes it from the store

### Requirement: A provider narrows a scope with a margin
A connector MAY narrow a scoped listing by its provider's own filter to a superset of the scope: IMAP `SENTSINCE` a day below the floor (or no `Date` header) and `SENTBEFORE` two days above a ceiling, Graph `$filter=receivedDateTime ge` two days below the floor, Gmail `after:` and `before:` two days around the scope, in epoch seconds. The `Date` SHALL decide what is kept. A connector whose checkpoint is bound to no scope (IMAP, Gmail) SHALL widen a scope by listing only the band its coverage lacks; a Graph delta link made under a `$filter` is bound to it.

### Requirement: A mail round lands page by page, newest first
A mail round SHALL be answered in pages, newest first, each landing in its own write with its resume cursor: IMAP 500 UIDs by UID descending, its cursor `(UIDVALIDITY, lowest UID listed)`; Graph message delta 1,000 messages a page (`Prefer: odata.maxpagesize`, sent with every link), its cursor the next link; Gmail 100 ids, its cursor the page token. The checkpoint SHALL be the one taken when the round began where the provider gives one (IMAP `HIGHESTMODSEQ` at the select, Gmail's profile `historyId` read before the listing), Graph's delta link on its last page. An interrupted round SHALL resume from its cursor, and a cursor the provider refuses (an expired Graph link, a Gmail page token, a moved `UIDVALIDITY`) SHALL restart it.

#### Scenario: A round killed after its first page
- **GIVEN** a mailbox of 1,100 messages synced for the first time
- **WHEN** the run is killed once a page landed, and the account is synced again
- **THEN** the second run closes the round and the store holds every message once

### Requirement: Every listed member arrives named
Every member a listing hands the engine SHALL carry its meta (pimdir SYNC §4): IMAP reads `UID FLAGS RFC822.SIZE` and `BODY.PEEK[HEADER.FIELDS]` (`Date`, `From`, `To`, `Cc`, `Bcc`, `Subject`, `Message-ID`, `In-Reply-To`, `Content-Type`) in the listing's own `UID FETCH`, never `BODYSTRUCTURE`, deriving the meta from the header block as the body would derive it; Graph reads its summary `$select`; Gmail its metadata, `Content-Type` among the headers. A member the store already binds, a message being immutable, SHALL be named by its binding rather than read again. A kind whose meta is its body (CardDAV, CalDAV, Google Calendar and People, Graph contacts and events) SHALL have the bodies of a page fetched before it lands, 64 a request, each member carrying its body, unless the store holds it at the listed revision. The attachment mark SHALL be Graph's `hasAttachments`, else a top-level `multipart/mixed`, until the body is read.

### Requirement: The report states each collection's coverage
The report SHALL list under `coverage`, for every collection a run synced on an endpoint, the scope of that endpoint's last closed round (`since`, `until`) and when it closed (`at`), absent while none ever closed, and the round still open (`round`: `since`, `until`, `startedAt`). A round left open SHALL make the run incomplete.

### Requirement: The report counts what each source downloaded
The report SHALL list under `downloaded` the octets of bodies and listed meta each source received during the run.

### Requirement: A request that creates is sent again only when unprocessed
A request creating something (an uploaded or copied message, an event, a contact, a label, an imported message, a send, an invitation reply or cancel, a DAV create) SHALL be sent again on a 429 or a Google rate limit only, which a provider answers before acting, never on a 503 or another 5xx, after which it may have landed. A Graph body batch answered 500, 502 or 504 SHALL be sent again without making the source give up as throttled.

## MODIFIED Requirements

### Requirement: The report shows the one-source pull plan
A one-source sync SHALL report its pull plan, each non-tombstone item whose body it would download into the store, as `Fetch` hunks, in both a dry run (which stops there) and a real run (which then hydrates them). A dry run SHALL fetch no body to produce that report.

The plan SHALL select an item by the absence of a stored object, never by its detail level: a remote content change drops the stale object while the hub keeps the level the item had reached, so an item about to be re-fetched reads as complete to a level-keyed plan. A kind whose meta is its body arrives with it, its page fetching the bodies before it lands, so the plan SHALL also name every item the pull added or changed with its body. In a dry run such a member SHALL be named by its handle, its body not fetched: a preview that downloads an entire address book to print a plan is not a preview.

#### Scenario: A first dry run over a DAV account names its items
- GIVEN an initialized account with a CardDAV source and cards on the server
- WHEN `sync --dry-run` runs before any real sync
- THEN the report names each card, rather than reporting the account already in sync

#### Scenario: A dry run after a server-side edit names the re-fetch
- GIVEN a synced card edited on the server
- WHEN `sync --dry-run` runs
- THEN the report names the card whose body would be re-fetched

### Requirement: Meta and size fetches are targeted
A `Meta` fetch (link id + summary), which only revisits a claim a row does not hold, and the largest-first size probe SHALL fetch **only the handles being processed** (a `UID FETCH <handle-set>`), never the whole mailbox. A listing SHALL read the meta of the members the store does not bind and of no other, so an incremental sync's work scales with the number of changed messages, not the mailbox size.

### Requirement: IMAP enumeration is incremental (QRESYNC)
Enumeration SHALL carry a per-mailbox checkpoint `(UIDVALIDITY, HIGHESTMODSEQ)`. On a QRESYNC-capable server (ENABLEd on connect) with a checkpoint whose UIDVALIDITY still matches, a delta SHALL be a QRESYNC `SELECT (QRESYNC (uidvalidity highestmodseq))`: only the messages changed since the modseq, the meta read for the ones the store does not bind, plus the vanished UIDs, issuing **no FETCH when nothing changed**. Without a usable checkpoint (first sync, UIDVALIDITY change, malformed bytes) or on a non-QRESYNC server it SHALL answer a round, page by page.

### Requirement: DAV collections enumerate by sync token and resolve at Full
A CardDAV or CalDAV source SHALL enumerate through `REPORT sync-collection`, storing the returned sync token verbatim as the collection's opaque checkpoint, and SHALL fall back to a tokenless report (the whole member set, reported complete) when the server rejects the stored token. Because that report returns hrefs and ETags but no `UID`, the bodies of the members it lists SHALL be fetched through `addressbook-multiget` / `calendar-multiget`, in batches, before the page lands, each member named by its body, so a DAV item's link id has exactly one derivation. A member the store holds at the listed ETag SHALL be named by its binding, its body not fetched again.

### Requirement: A run reports the bodies it pulls, whatever the tier
A run SHALL report the bodies it fetches, and SHALL report the same ones whether or not it is a dry run. The report SHALL NOT depend on the tier a kind resolves its identity at.

A kind whose meta is its body is fetched while its page is listed, so its pull plan SHALL be read off the pull's events (the items it added or changed holding a body) as well as off the placements carrying no body yet; a plan read off the store alone is empty for exactly the items the run pulled, and the run calls itself quiescent having downloaded a collection.

#### Scenario: A first contacts sync says what it did
- GIVEN an empty store and an address book holding one card
- WHEN `sync` runs without `--dry-run`
- THEN it reports fetching that card, as `--dry-run` said it would, rather than reporting itself already in sync

### Requirement: An incomplete run has its own exit code
A run that could not do all its work SHALL exit with a code of its own, distinct from success, failure and the conflict code: a source it could not scan, a hunk it could not apply, a send or an intent that failed without parking, a source that gave up throttled, or a round left open on a collection it synced. It SHALL win over the conflict code, the report still counting what waits for a person.

An unreachable server is the common case. The run stops nothing else over it, so it is no failure, yet a run with nothing to do and a run that could not reach its server both exited 0, and a caller told offline from idle by parsing error strings. A rerun clears this state on its own, which is what sets it apart from the conflict code: that one waits for a person, this one for the network.

A send or an intent that parked is not this: it waits for a person, and its row says why.

#### Scenario: An unreachable source is not an idle run
- GIVEN an account whose only source refuses the connection
- WHEN it is synced
- THEN the scan error is reported and the run exits with the incomplete code

### Requirement: A throttled provider is waited out, then named
A request Microsoft Graph, Gmail, Google Calendar, Google People, CalDAV or CardDAV answers with 429, or with 503 for a request that creates nothing, or Google with a 403 rate limit, SHALL be sent again after the wait the provider states, else after a bounded exponential back-off with jitter. Past the bound, or on a stated wait longer than neverest waits, the source SHALL give up: no connection opened from it sends anything until the wait is over, what the run already wrote stays, the report lists the source under `throttled` with `until` (RFC 3339 UTC), and the run is incomplete (exit 3).

#### Scenario: A server answering 503
- **GIVEN** an account whose CalDAV source answers every request 503
- **WHEN** it is synced
- **THEN** the run exits 3 and `throttled` names `caldav` with an `until`

## REMOVED Requirements

### Requirement: A probed item is raised to the tier its kind resolves at
Probes are gone (pimdir draft-04): every listed member arrives named, a kind whose meta is its body having its bodies fetched with its page.
