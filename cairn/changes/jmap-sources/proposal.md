---
cairn: change
id: jmap-sources
status: draft
created: 2026-11-15
---

# Native JMAP sources

Self-contained: a session with no prior context can take it from here. Not started: the owner scheduled it after the next major release (2026-11-15). Every fact below was read in the tree on 2026-11-15 (neverest 59ca533, io-jmap 0.10.0 at 5953e96, android at its HEAD of that day); check each against the code before building on it.

## Why

A JMAP server (Fastmail, Stalwart) syncs today over IMAP, CardDAV and CalDAV. JMAP would bring four things those cannot:

- **One message, several mailboxes.** A JMAP `Email` is one object with a set of `mailboxIds`, which is pimdir's one item filed in several collections. IMAP hands the same message once per folder, and every run pays for each copy.
- **A change feed that states removals.** `Email/changes` answers `created`, `updated` and `destroyed` since a state string, for the whole account, on every server. IMAP states a removal with QRESYNC alone.
- **A send without a second channel.** `EmailSubmission/set` sends through the session the sync already holds, as Graph does through `sendMail`, so a JMAP account declares no `smtp` table.
- **One OAuth grant.** Fastmail's JMAP takes a bearer token for mail and contacts, where its CardDAV takes an app password only (its 401 advertises `Basic, Bearer`, only Basic works). A JMAP account needs no app password at all.

It also closes a gap inside the family: the Android app already syncs JMAP mail, contacts and calendars into a pimdir store, and himalaya, cardamum and carillon speak JMAP, while neverest refuses it.

It is not urgent: IMAP, CardDAV and CalDAV cover the same accounts today.

## What exists

- The configuration parses. `JmapConfig` (src/config.rs) reads `server`, `tls`, `alpn`, `auth` (`header`, `bearer.token`, `basic.username` and `basic.password`), `identity-id`, `drafts-mailbox-id`, and the shared `collection`, `flag`, `item` and `pool-size` keys. `jmap` is an account-level sugar key and a `sources.<name>.jmap` table, and `is_http()` already counts it.
- Opening it is refused: `SourceAccountBackend::resolve` bails with "A JMAP source parses but cannot be opened yet" (src/account.rs). There is no `jmap` cargo feature, no io-jmap dependency, no `Client::Jmap` variant (src/client.rs), `supports` returns no capability row for it (src/offline/capability.rs) and `sends_natively()` names Graph alone (src/config.rs).
- `jmap` means mail: there is no key for JMAP contacts or calendars, and the per-source keys sit directly in `jmap`, the shape Graph left in neverest abb2a95 (J3).
- One spec task is already waiting on the connector: cairn/changes/a-message-is-keyed-by-the-formats-reading/tasks.md, "A JMAP connector reads `header:…:asRaw:all`, first element".
- io-jmap 0.10.0 covers the calls: session, `Email/get`, `Email/query`, `Email/changes`, `Email/set`, `Email/import`, `Email/copy`, blob upload and download, `Mailbox/get`, `Mailbox/query`, `Mailbox/set`, `Mailbox/changes`, `Identity/get`, `EmailSubmission/set` (with `onSuccessUpdateEmail`), `cannotCalculateChanges` as a typed error, `ifInState` on every set; RFC 9610 `AddressBook` and `ContactCard` (get, query, changes, set, copy) with the vCard conversion behind its `vcard` feature; the calendars draft (draft-ietf-jmap-calendars-27) with `Calendar/get` and `Calendar/changes`, and `CalendarEvent` get, query, changes and set with the iCalendar conversion behind `ical`.
- tests/stalwart.sh already starts a Stalwart that serves JMAP on its HTTP port (8080, movable for rootless Docker through tests/pasta.sh).

## Prior art

Two places to read before writing a line:

- **The Gmail connector** (src/gmail/client.rs, src/gmail/run.rs) is the model inside neverest. Gmail holds one message under several labels and one history for the mailbox, which is JMAP's shape exactly: handles stable across collections (`global_handles`), an account-wide feed read once a run and projected on each collection (`GmailRun`), an expired position restarting a full round (`Listed::CursorRejected`, `Listed::HeadRejected`), a delta head listed while a round is open. The seam needs no new notion for JMAP.
- **The Android app** (android/rust/src/mail/jmap.rs, mail/jmap_step.rs, contact/jmap.rs, calendar/jmap.rs, client/jmap.rs) syncs JMAP into a pimdir store today, on Fastmail and on Stalwart. Its paging (anchors, the reception margin), its blob reads, its `Email/import` refusals, its card revision (a hash of the card's JSON) and its contact pushes are worked out and tested. Two things do not carry over: it steps one query over the whole account where neverest's engine enumerates a collection at a time, and it keys a message by the server's parsed `messageId` and `sentAt`, which neverest's rule forbids (`A message is keyed by the format's reading`: the key is derived by io-pimdir from the raw header fields).

## The plan

Three sources, in this order. Each is a release on its own.

One `jmap` table holds the session and names the domains it syncs, as `msgraph` does (J3): `jmap.mail`, then `jmap.contacts`, then `jmap.calendar`, each a source of its own (`jmap`, `jmap-contacts`, `jmap-calendar`) with its own rights and filter.

### 1. Mail: `jmap.mail`

A `jmap` cargo feature in the default set, `Client::Jmap`, src/jmap/client.rs and src/jmap/run.rs. The source is `jmap`; it syncs `message/rfc822`.

**Collections.** Every `Mailbox` is a collection, keyed and named by its name path (`Parent/Child`, built from `parentId`), as Graph and Gmail key theirs, so a JMAP endpoint and an IMAP endpoint of one account pair their folders. `role` gives the collection's role. Create, rename and delete go through `Mailbox/set`; `myRights` and the configured rights give the capabilities a sync declares.

**Handles.** The `Email` id, immutable and the same in every mailbox.

**Enumeration.**

- A full round is `Email/query` with `inMailbox`, newest first, a page at a time, its `Email/get` back-referenced: `id`, `mailboxIds`, `keywords` and `size` for every member, the `headers` for those the store does not bind (`Held`). The checkpoint is the `Email` state read before the round's first page, so nothing that lands during the round is missed.
- A delta is `Email/changes` since that state, read once a run for the account and projected on each mailbox: a changed email whose `mailboxIds` names the mailbox is an entry, one the store binds there that left it is stated vanished, a destroyed one is vanished everywhere. `cannotCalculateChanges` restarts a full round.
- While a round is open, a resuming run lists its delta head first, from the round's state.
- Under a scope (`item.filter.since`), the query narrows with `after`, which reads the reception and not the `Date`, so it takes a margin as Gmail's `after:` does, states `narrowed` on every page, and its last page hands every id of the mailbox from a query no date narrowed.

**Summaries and bodies.** A summary is derived by io-pimdir from the raw `headers` list, never from `messageId`, `subject` or `sentAt`. A body is the octets of `blobId`, downloaded through the session's `downloadUrl` template, kept as they arrive.

**Flags.** Keywords and flags map one to one (`$seen`, `$flagged`, `$answered`, `$draft`, `$forwarded` and the registered rest, src/item/flag.rs), pushed as `keywords/<keyword>` patches.

**Pushes.** An add uploads the blob then calls `Email/import` with the mailbox and the keywords. A move is one `Email/set` patch dropping one mailbox and adding the other; a copy adds a mailbox. A delete drops the mailbox, and destroys the email when it was its last one. An update refuses, a message being immutable. A write on an email gone counts as done.

**Send.** A `submit` intent imports the message as a draft (`drafts-mailbox-id`, else the mailbox whose role is `drafts`) and creates an `EmailSubmission` under `identity-id`, else the identity matching the message's `From`, with `onSuccessUpdateEmail` filing it in the sent mailbox and clearing `$draft`. `sends_natively()` answers true, an `smtp` table beside a `jmap` mail source is refused as it is beside Graph's, and the rule that a send possibly delivered is never sent again holds.

**Session.** One `session_get` on the configured URL, no redirect followed. The core capability's `maxObjectsInGet`, `maxObjectsInSet`, `maxCallsInRequest`, `maxSizeUpload` and `maxConcurrentRequests` bound the pages, the batches and the pool. A rate refusal is waited out through src/throttle.rs.

### 2. Contacts: `jmap.contacts`

A second domain of the same table and feature, as `msgraph.contacts` is Graph's: the source `jmap-contacts`, syncing `text/vcard` over the session the mail domain uses.

- Every `AddressBook` is a collection, keyed by its id and named by its `name`. A `ContactCard` lists the books it is in (`addressBookIds`), one item filed in several collections.
- The body is the vCard io-jmap converts the card to and from (RFC 9555, its `vcard` feature); the projection stays in the protocol crate. The item's identity is the card's `uid`.
- JMAP has no revision for one card and no `If-Match`. The revision is a hash of the card's JSON, as the Android app pins it, and it only tells an unchanged card from a changed one.
- A delta is `ContactCard/changes` since a state. A write is `ContactCard/set` carrying `ifInState`, the state the run last read: a card edited on the server since is then refused (`stateMismatch`), and the next run merges it, as a Graph contact edited since the last sync is refused today.

### 3. Calendars: `jmap.calendar`

Left for later, on three facts: the specification is still a draft, io-jmap reads calendars and cannot create, rename or delete one (no `Calendar/set`), and Fastmail's JMAP grant covers mail and contacts only (android/docs/providers.md, finding of 2026-11-04), so Stalwart would be the only server to test against. It opens when the draft is published or a provider serves it. The Android app's calendar/jmap.rs is the starting point.

## Library work

None blocks the mail source. Each is worth doing in io-jmap first:

- `JmapEmailProperty` cannot ask for one raw header (`header:Message-ID:asRaw:all`), only for the whole `headers` list. The list is enough to key a message, at the cost of every header field of every new message. A property variant carrying a header name and form would cut that to the fields a summary reads.
- `blob_download` answers a `Vec<u8>`. The seam writes bodies to a stream (`fetch_bodies<S: Write>`); a streamed download would keep a large message out of memory.
- No `Calendar/set`, which the calendar source waits for.

## Decisions asked

To settle when the work opens, each with the answer this proposal assumes:

- **J1, what keys a mailbox.** The name path, so JMAP pairs with IMAP on one account, against the mailbox id, which never changes on a rename. Assumed: the name path, a rename reading as a remove and an add, as on IMAP and Gmail. To check first: a name holding a `/`.
- **J2, a delete from an email's last mailbox.** Destroy it, which is what an IMAP expunge from its last folder does, against moving it to the trash mailbox. Assumed: destroy, under the source's `item.delete` right.
- **J3, the table shape.** Graph's, which moved to one `msgraph` table in neverest abb2a95 (2026-11-15, cairn/changes/graph-domains-share-one-table): one `jmap` table holding the session (`server`, `tls`, `alpn`, `auth`), naming the domains it syncs under `jmap.mail`, `jmap.contacts` and `jmap.calendar`, each `= true` or a table of its own `collection`, `item`, `flag` and `pool-size`; the source ids `jmap`, `jmap-contacts` and `jmap-calendar`; under `sources.<name>`, a `jmap` table names one domain and the source keeps `<name>`. Against: the three tables Graph left. Assumed: Graph's shape, so one session serves the account's domains and a JMAP and a Graph account read alike. Since `jmap` parses today with `collection`, `item`, `flag` and `pool-size` directly in it, those keys move under `jmap.mail` and the old spellings are refused by name, as Graph's were; no store holds a JMAP source yet, so nothing migrates.
- **J4, the guard on a contact write.** `ifInState` refuses a write whenever any card of the account changed, which starves on a busy shared book. Assumed: `ifInState`, with one delta and one retry before the item waits for the next run.
- **J5, raw headers.** The whole `headers` list now, the one-header property once io-jmap has it. Assumed: ship on `headers`.

## Not in scope

Push (EventSource and push subscriptions belong to carillon), threads as a unit, `Email/queryChanges`, vacation responses, identities beyond the one a send names, shared accounts other than the session's primary one for each capability, and JMAP as a target of a migration before it has run as a source.
