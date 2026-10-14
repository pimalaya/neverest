# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Added attachment stand-ins (pimdir STORAGE §14.3, io-pimdir `14a8fca`): a message body a sync lands records, in the write storing it, one file holding no body per part disposed as an attachment, in the account's attachments collection (`<account>/\u0001attachments`, role `attachments`), with its name, media type, decoded size and IMAP section, and an `attachment` reference from the message, whichever path landed the body (primed, by page while the listing runs, or in phase 2, Gmail's shared landing included). A body the account already held (a copy, a move, a second Gmail label) adds none, and bodies held before are not walked again.
- Added Graph mail round heads (pimdir `round-head`, io-pimdir `07bd316`): while a folder's first round is open, a run resuming it first lists the messages Graph received since two days before the round began (`receivedDateTime`, newest first), so a message arriving meanwhile lands before the round's next page instead of after the round closes. The round's last page lists them again after its own rows, so what is still there is kept when the round closes and what was deleted meanwhile is dropped. Graph lists no delta head, its delta link arriving with a round's last page only.
- Added Gmail round heads (pimdir `round-head`, io-pimdir `07bd316`): while a label's first round is open, which on a large Inbox paced by the quota lasts many runs, a run resuming it first lists the history since the round began, so a message arriving meanwhile lands before the round's next page instead of after the round closes, and the round closes on the history id that head read. An expired history id restarts the round. A later page reads again what the head touched and leaves out a message that left the label since it was listed. The driver now hands the sync the heads its source lists; under `--prime`, a resumed round lists its head before it stops.
- Added heads to IMAP rounds (pimdir SYNC §4, io-pimdir `07bd316`): a run resuming a round an earlier run left open lists first what changed since the round opened, so a message arriving or leaving during a long first sync shows at once rather than after the round's last page. With QRESYNC, a delta head from the round's checkpoint (`SELECT … QRESYNC`: changes, vanished UIDs and the new checkpoint); without, a band head (`UID SEARCH SINCE` the round's opening less two days). The round cursor now carries the highest UID found when the round opened, and the round's last page lists again every message that arrived above it, so nothing a band head named is found absent when the round closes; a round left open by an earlier version lists its whole scope again on its last page, once.
- Added `sync --prime <N>`: every collection first lists its newest page and downloads its N newest bodies, before any collection lists its second page, so each one is readable early whatever its size, over one connection too; the paused collections are then listed to their end, and phase 2 downloads each collection's N newest owed bodies ahead of the rest. It orders the run and never shortens it. 50 by default under `--download-order newest`, 0 under `largest`; `--prime 0` turns it off. On a local Stalwart account of three folders of 1,100 mails over one connection, the first body is readable after 0.6 s instead of 3.9 s, and every folder's first page after 1.7 s instead of 3.0 s, the whole run unchanged (7.3 s).
- Added `sync --bodies <all|listed>`: under `listed`, a run downloads only the bodies of what its own pages listed, every other owed body left for a later run, so a mail a targeted run brings is readable at once rather than behind the backlog an interrupted first sync left.
- Added `sync --max-wait <SECONDS>`, 120 by default: the longest single wait on a throttling provider, past which the source gives up for the run, Graph's body batches and Gmail's batch pauses included. `--prime`, `--bodies` and `--download-order` are refused beside `--intents-only` and `--declare-only`, which list nothing.
- Added scoped mail syncs (pimdir draft-04 SYNC §5, io-pimdir `35a1c3f`): `item.filter.since` in the account (`30d`, `12w`, `6mo`, `1y`, a date or an RFC 3339 instant) and `sync --since <WHEN>` for one run bound a mail collection by its `Date` header. A message older than the floor is neither listed nor deleted, a message with no usable `Date` is in every scope, and an explicit removal applies whatever the date. The listing is narrowed to a superset (IMAP `SENTSINCE` a day below the floor, or no `Date`; Gmail `after:` two days below), the `Date` deciding; Graph lists the scope exactly by `sentDateTime` (`/messages`, `$filter=sentDateTime ge A and sentDateTime lt B`, newest first). A wider scope lists only the band it lacks on IMAP, Gmail and Graph, whose checkpoint is bound to no scope: Graph keeps one message delta link per folder, made over the whole folder with no date filter (with summaries for a whole-folder round, else by one pass of ids, dates and flags that closes the first round under a scope and corrects what changed while its band was listed), which serves every later scope, a change outside the scope dropped and a new message in it named by its summary read in JSON batches. A Graph link stored under a scope before this, made under a `$filter`, is refused once, which opens a round. Refused, by the key it came from, on an account syncing contacts or calendars.
- Added paged rounds (SYNC §4): a mail listing lands page by page, newest first, each page committed with its resume cursor, so the first page of a large mailbox is readable within seconds and an interrupted run resumes where it stopped. IMAP pages 500 UIDs (UID descending), Graph 1,000 messages (the message delta with `Prefer: odata.maxpagesize=1000`, io-msgraph `00ef069`, or `/messages` with `$top=1000` under a scope), Gmail 100 ids. A refused cursor (an expired Graph link, a Gmail page token, a moved `UIDVALIDITY`) restarts the round.
- Added `coverage` to the sync report: per collection synced, the scope of the source's last closed round (`since`, `until`) and when it closed (`at`), and the `round` still open (`since`, `until`, `startedAt`); a round left open makes the run incomplete (exit 3). Added `downloaded: [{ source, bytes }]`, the octets of bodies and listed meta each source received.
- Added a back-off on throttling for Microsoft Graph, Gmail, Google Calendar, Google People, CalDAV and CardDAV: a request answered 429 or 503, or by Google with a 403 rate limit, is sent again after the wait the provider states (a Graph batch's `Retry-After`, Google's `Retry after <instant>`), else after a bounded exponential back-off with jitter (1 to 16 s, five times). Google's refusals are read by their reason and wording: a burst (Gmail's `Too many concurrent requests for user`, a per-second rate, a 503) waits a second or two, a per-minute quota (`Quota exceeded for quota metric 'Total Query Cost' and limit 'Units per minute per user'`, which carries the same `rateLimitExceeded` reason) waits 2, 3, 4 then 5 s and then the next minute, and a daily quota (`dailyLimitExceeded`, `per day`) gives the source up until Google's midnight (America/Los_Angeles). Past that, or on a stated wait over two minutes, the source gives up: every connection it opened stops sending until the wait is over, what landed stays, and the report lists it under `throttled: [{ source, until }]`, the run exiting 3.
- Added Gmail pacing: the Gmail source holds itself to 200 quota units a second across its connections (about 40 reads a second, Gmail metering 250 per user) and to `gmail.units-per-minute` over any sixty seconds (6,000 by default, the per-user quota a Google project gets, a read counting the 5 units Gmail bills), each call or batch taking the cost of all its reads, rather than running into refusals.
- Added Gmail batched reads: message metadata and raw bodies are read through Gmail's HTTP batch endpoint (`www.googleapis.com/batch/gmail/v1`), `gmail.batch-size` reads a batch (10 by default, at most 100) and `gmail.batches-in-flight` batches at once across the source (1 by default), Gmail running a batch's reads side by side and refusing past its per-user concurrency (measured: no refusal in 2,250 reads by batches of 10 one after another, 19 % at 50). A read refused inside a batch is sent again at the head of the next one; a 404 is a message gone, never an error. A raw batch holds about 8 MiB of messages by their sizes, a message over 5 MiB being read alone, and each body is stored as its batch lands.

- Added `unreached` to the sync report: every endpoint whose connections the run could not open (a server it could not reach, or one refusing the connection or its credentials), with the error, so a program tells an unreachable provider from an unreadable folder without reading the collection patch. The failed scan and exit 3 are unchanged.
- Added `drain`: applies what frontends queued in the store (himalaya, calendula, cardamum), reading no credential and opening no endpoint, so a created item gets its id and readers list it at once, offline included; the next sync pushes it. It never waits for a sync holding the store: it answers `busy` with exit 3. It reports each applied row with the `seq` an `add` created, the parked rows, and how many rows wait for a sync (intents).
- Added collection roles in the store (pimdir draft-04, io-pimdir `ef8eae0`): every sync records the role each server states for a collection it syncs (`collections.role`: `inbox`, `sent`, `drafts`, `trash`, `junk`, `archive`, `all`, `flagged`, `important` for mail, `default` for a calendar or an address book), and clears one the server no longer states. IMAP and Gmail state theirs in the listing the sync already reads, as do Graph calendars and contacts, Google Calendar and People; Graph mail (one well-known folder lookup per role) and CalDAV (`schedule-default-calendar-URL`) are looked up again only when the source's collection set moved, the answer kept in `neverest.json`. Never guessed from a name. A paired sync (with targets) records none yet.
- Added `sync --declare-only`: lists every collection the sources hold and records its kind, name and role in the store, whatever the collection filter, syncing no item; only queued collection creations run. What a frontend reads to offer the collections it could sync and to learn the inbox or the default calendar, without a throwaway store. Its collections count as held by their source, so the intents a source performs on what it holds (`collection.create`) are declared on them before any sync.
- Added receipts for the intents neverest performs (pimdir draft-04, io-pimdir `8c83c04`): a `submit` sent, a `calendar-reply` or `calendar-cancel` performed and a `collection-create` done are acknowledged with a receipt, so the frontend that queued them reads them applied (`himalaya pimdir queue show`) rather than unknown, as a cancelled row reads.
- Added the `collection-create` intent (pimdir draft-04): performed at the start of a sync, before the listing, so the same run lists the new collection. IMAP creates the mailbox under its parent with the server's delimiter, Gmail the label `parent/name`, CalDAV and CardDAV a top-level collection displayed as the name; one already listed is success. Declared as `collection.create` on each collection an IMAP, Gmail or DAV source syncs, `none` on Graph and Google Calendar and People.
- Added occurrence replies and cancels: a `calendar-reply` or `calendar-cancel` naming a `recurrence_id` acts on that instance of the series on Graph and Google, found among the instances two days either side of its date; one not found parks. Declared as `calendar.reply.occurrence` and `calendar.cancel.occurrence`, `none` on CalDAV.
- Added `sync --download-order <largest|newest>`: bodies download largest first by default, the fastest whole run, or newest first, so recent mail is readable early in the first sync of a large account. Items without a date come last.
- Added Microsoft Graph mail writes through the store: a move pushes through Graph's own move, a copy through its copy, and an added message lands in Drafts, the one folder where Graph can create it, Graph filing every uploaded message as a draft. An add into another folder is rejected rather than filed there as a draft. Graph gives a moved or copied message a new id, and the store follows it.
- Added the `wizard` cargo feature, on by default, gating the interactive configuration: the `configure` command and the offer a first run makes. A build without it drops the prompts and the dependencies only they use, and a missing configuration points at the documented sample instead.
- Added capability declarations (pimdir draft-03, STORAGE §15.6): every run, before the queue drains, each mail, contacts and calendar source declares in the store what it can push or perform, from its backend and its configured rights alone, so himalaya, cardamum and calendula refuse an unsupported action before it is queued. JMAP sources stay undeclared.
- Added the sent copy: a `submit` asking for `copy` files the message there once sent, by an `add` queued in the same run, unless the provider files sent mail itself (Gmail, Graph). A failed send files no copy.
- Added the join link of a Graph online meeting (Teams) to the iCalendar of its event, as a standard `CONFERENCE` (RFC 7986 5.11; io-msgraph `d6cb958`), and the ways of joining a Google conference likewise, one `CONFERENCE` per entry point (io-gcal `1268ef6`), where `X-MICROSOFT-SKYPETEAMSMEETINGURL` and `X-GOOGLE-CONFERENCE` were written before.
- Added online meetings on push (pimdir STORAGE Annex B.1 `calendar.online-meeting`): an event added or updated with `X-PIMDIR-ONLINE-MEETING:TRUE` gets a Google Meet (`conferenceData.createRequest`, every Google event write now declaring `conferenceDataVersion=1`) or a Graph online meeting (`isOnlineMeeting`, the calendar's default provider). Its write reports no revision, so the next sync reads the event back with its `CONFERENCE` and without the request property. Declared `partial` on Google Calendar (a Workspace domain can turn Meet off) and Graph (an account without a default provider gets no link), `none` on CalDAV.
- Added the calendar invitation intents (pimdir Annex B.2): a queued `calendar-reply` answers an invitation and a `calendar-cancel` cancels a meeting the account organises, through Graph's event actions (the comment carried) or Google's own notifications (`sendUpdates=all`, a cancellation without the comment). Each sync performs them first, one-way included, and reports them under `intents`; a refusal or a gone item parks, a transient failure retries. They are declared on each calendar the source syncs; CalDAV declares them `none`.
- Added `sync --intents-only`: applies the queue, then performs what frontends queued (collection creations, sends, invitation replies and cancellations) and stops, listing no collection, pushing no other write and fetching nothing. An endpoint opens only when an intent needs it, so a message leaving through an `smtp` table opens that channel alone, even with the source's own server down. It refuses `--dry-run`, `--reset`, `--declare-only`, `--since` and the collection filters. What a frontend runs right after queueing a send; the copy neverest files in Sent is pushed by the next sync.
- Added `uncertain` to each `submitted` entry of the sync report: true for a send that may have been delivered despite its failure, false on success and on a failure that certainly delivered nothing.

### Changed

- Changed Microsoft Graph body batches to be bounded in octets: the listings ask Graph for the MAPI message size (`$expand` of `Integer 0x0E08`, io-msgraph `fa73f04`; the whole-folder delta asks without it once Graph refuses it), which the summary now carries, and a batch holds at most 20 gets and 4 MiB of stated size, a message over 1 MiB or of no stated size being read alone (the sizes of several unsized messages read first, 20 a batch). Each body is written to the store as soon as it is decoded rather than once its batch and every retry round were in, so a connection holds one answer of about 4 MiB and one body at a time instead of up to twenty messages of any size.
- Changed the Graph mail listing to hold a few octets a message: a delta and the pass are read page by page into ids and flags rather than collected whole, and the run no longer keeps the summary of every message it listed: a band round followed by its pass keeps three bits a message, and the pass lists again only what changed since the band. The `Meta` tier reads each summary by id.
- Changed a throttled get inside a Graph batch to wait its own `Retry-After` rather than the longest of the batch, and to give the source up at once on a stated wait over `--max-wait` (two minutes by default), as a single request does, rather than waiting two minutes and asking again.
- Changed submission to at-most-once. A send failing after the message was handed over whole with no verdict (SMTP: no reply after the data's final `.`; Graph `sendMail`: the request written whole and no answer, or 500, 502, 503 or 504) is uncertain: parked with an error beginning `uncertain: `, never sent again by any run, the run that parks it exiting 2. Each send's attempt is counted (`attempts`) and its queue id written to `submitting` beside the store before the first byte leaves, so a run killed mid-send leaves the row attempted and the next run parks it as uncertain rather than sending it again. A Graph 401 is now transient (an expired token), where every 4xx but 408 and 429 parked. A refused SMTP message resets the transaction before the next one, and a session lost mid-send leaves the rest of the run's sends pending, unattempted.
- Changed an SMTP channel that does not open from a warning to a reported failure: a server that cannot be reached is named `<source>.smtp` under `unreached`, the sends pending and unattempted; refused credentials fail each queued send with the reason, not parked. Either makes the run exit 3, as any `unreached` endpoint now does.
- Changed the one-source sync to take its collections by role, as recorded this run: the inbox (or a default calendar or address book), sent, drafts, every other collection by name, the all-mail archive, junk, then the trash, where it took them by provider id. A batch of bodies tied with another in the download order goes to the collection taken first.
- Changed the Microsoft Graph sources to open three connections, or their `pool-size`, never more than `connections`, where they opened one; Gmail, Google Calendar, Google People, CalDAV and CardDAV keep one. The bodies of a landed page now download on Graph's spare connections as on IMAP's.
- Changed io-pimdir to 0.8, patched to the pushed revision `189db40` (pimdir draft-04): opening a store creates the store-wide `items_by_sort_global` index, so a reader paging mail across collections walks it instead of sorting every row, and brings an older store to the canonical schema (the invitation column, references, files, receipts).
- Changed the other Pimalaya libraries to their latest versions, unreleased fixes taken from pushed revisions through `[patch.crates-io]`, as the Android client pins them: ical-rs 0.6 (`caa1422`), io-msgraph 0.6 (`2ae2637`), io-gcal 0.2, io-gmail 0.5, io-gpeople 0.5, io-smtp 0.6, io-http (`e181530`), io-imap (`b1ab38e`) and io-webdav (`62fad89`). What they change for a sync:
  - An HTTP response whose head passes 64 KiB or whose body passes 256 MiB fails its exchange, where it was buffered whatever its size; a Graph body batch that large falls back to one get a message. A connection closed mid-body fails the exchange rather than hanging the run.
  - An IMAP batched body fetch fails on a message announced over 100 MiB, the batch then falling back to one fetch a message, which takes it.
  - A Graph series no longer keeps its `EXDATE` lines in its stash: Graph states its cancelled occurrences itself, which a stashed copy doubled on read. An `EXDATE` added in the store is still not pushed, Graph cancelling an occurrence only by the deletion of its instance.
  - A Google Calendar series is updated from its master whatever the order of its components, where the first `VEVENT` of the document was read.
  - Lines of an iCalendar written from Graph or Google are folded at 75 octets, and a merge settles `DTSTAMP`, `LAST-MODIFIED` and `SEQUENCE` without a conflict.
- Changed every listed member to arrive named (pimdir draft-04, probes removed): IMAP reads `UID FLAGS RFC822.SIZE` and the header fields Annex A needs, `Content-Type` among them, in the listing's own `UID FETCH`, no `BODYSTRUCTURE`; Graph its summary `$select` (`hasAttachments`, `ccRecipients` added); Gmail its metadata read (`Content-Type` added), in HTTP batches. A message the store already binds is not read again. DAV, Google Calendar and People, Graph contacts and events fetch each page's bodies, 64 a request, before it lands, a member at the revision the store holds being named by its binding. The attachment mark comes from Graph's `hasAttachments`, else from a top-level `multipart/mixed`, until the body is read.
- Changed body downloads to follow the listing page by page: in the one-source sync, as soon as a mail page has landed its bodiless members are queued, and every connection with no collection left to list downloads them (in the download order, batches of 64) while the next pages list, so the first mail of a large mailbox is readable before its last page is listed rather than after. A round an earlier run left open first queues the bodies its landed pages still owe. What is left (a run over one connection, a failed download) is downloaded after the listing as before. On a local Stalwart folder of 1,600 mails over four connections, the first body is readable after about 0.45 s instead of 1.6 s, and the whole folder with its bodies after 2.4 s instead of 2.7 s.
- Changed the throttling back-off to send a request creating something (an uploaded or copied message, an event, a contact, a label, an imported message, a send, an invitation reply) again on a 429 or a Google rate limit only, never on a 503, after which it may have landed already. A Graph body batch answered 500, 502 or 504 is still sent again but no longer gives the source up as throttled.
- Changed `sync` to drain the queue once, at its start, before reading any credential or opening any endpoint, rather than per source after connecting: a run that cannot reach its server (offline, an expired token) still applies what frontends queued. `--declare-only` still drains nothing.
- Changed the Gmail source to read each message once a run and the mailbox history once a run: a message listed under several labels is named from the read of the first one listing it (its flags from each label's own flag listings), and a delta reads `history.list` once for the whole mailbox, with no `labelId`, and projects it on each label, where it read the history once a label. A message's labels are read again for a delta only when no label read them since the history was.
- Changed Microsoft Graph mail body fetches to JSON batches: twenty raw MIME gets per request instead of one, the first sync of a large Microsoft 365 box no longer paying a round trip per message. A request Graph throttles (429, common 5xx) is sent again after its `Retry-After`; any other refusal still falls back to a single get.

- Changed `check` to be documented as a doctor: what it prints about collections, roles and capabilities is for a person, and a program reads the store instead (`sync --declare-only`). IMAP `\Flagged` and `\Important` now state the `flagged` and `important` roles.
- Changed `check` to mark the default calendar and address book with `default: true` (Google Calendar `primary`, the Google People address book, Graph `isDefaultCalendar` and the default Contacts folder, CalDAV `schedule-default-calendar-URL` per RFC 6638 when the server gives it; none on CardDAV and mail), and to list each source's `capabilities: [{ name, support, detail? }]`, the source-wide rows a sync declares in the store.
- Changed the UID of a Microsoft Graph event that came in by mail: Exchange files it under its global object id, which wraps the organizer's UID; the event is now stored under that UID (io-msgraph `ical::original_uid`). Stored UIDs of such events change at the next sync.
- Changed `check` to list each source's collections, `collections: [{ id, name, role? }]`, instead of counting them, `role` being what the server states the collection is for: `inbox`, `sent`, `drafts`, `trash`, `junk`, `all` or `archive`, from IMAP SPECIAL-USE and `INBOX`, Graph's well-known mail folders, or Gmail's system labels; never guessed from a name. A caller adding an account can propose its folders from it. Nothing is written to the store.
- Changed the one-source sync to keep each body as it arrives: every downloaded batch is written to the store at once, instead of the whole account once everything had downloaded. A mail is readable minutes after a first sync starts, and a run stopped halfway (quit, sleep, a lost connection, a caller's time limit) keeps what it downloaded, the next run fetching only the rest.
- Changed `sync` to exit 3 when the run could not do all its work, a rerun picking it up: a source it could not reach, or a hunk, a send or an intent that failed without parking. An unreachable server used to exit 0, like a run with nothing to do. Exit 3 wins over the conflict code 2.
- Changed `check` to also open and authenticate the SMTP channel a source declares, without sending anything, so a wrong submission server or password fails the check rather than the first send. Each endpoint now reports `smtp`, whether its channel was checked.
- Changed a `submit` naming its sending source to be sent by that source alone.
- Changed Google Calendar updates and deletes to notify the attendees (`sendUpdates`): an update when the event leaves them to the server (`SCHEDULE-AGENT`), a delete when the event has attendees.
- Changed a new Google Calendar event the account organises, scheduled on the server, to be inserted with its UID and invite its attendees, rather than imported, which notified nobody.

### Fixed

- Fixed a message filed again from a body the account holds (an IMAP COPY or MOVE, a second Gmail label, a body an upgrade links from the store) keeping the size and attachment mark its listing read, and a downloaded message reading no invitation: io-pimdir `ca4c589` states, as the write lands, a linked body's size and the mark and invitation of a row holding it, and walks an entering body once for its stand-ins, mark and invitation.
- Fixed the attachment mark of a downloaded message reading unknown: the body was streamed into the store and summarised from its header block, `attachment` left `NULL` over the mark the listing had read. The walk of the parts (io-pimdir's `PimdirAttachmentWalk`) now reads the body as it streams, keeping a header block, a line head and a boundary per level, never the body, so the mark is the one the whole message gives: set for an attachment under `multipart/signed`, cleared for a `multipart/mixed` holding only an inline image. A body served from the store to a second Gmail label is walked from its blob alike.
- Fixed a body deleted on the server between its listing and its download failing the run: a Gmail or Graph read answered 404 and an IMAP UID a `UID FETCH` answers nothing for are now answered gone, the run completes, and no later batch of the run asks for the body again (phase 2 included); the member stays as listed until the next run's listing takes it away. Graph contacts and events likewise.
- Fixed a Gmail message filed under several labels downloading its body once a label: a batch reads where its messages are owed across the labels, downloads each body once and lands it under every label holding the message, and a body a label already holds serves a label added later with no download.
- Fixed a resumed Gmail or Graph hydration asking the server for sizes the store records: the sizes of the owed bodies plan the batches, a `minimal` read (Gmail) or a size batch (Graph) asking only for those neither the run nor the store knows.
- Fixed Microsoft Graph requests failing on a connection Graph closed while it idled (about two minutes): a connection idle over a minute is reopened first, and a read (a `GET`, a `$batch` of gets) that met a reused connection already closed before any octet of its answer is sent again once on a new one. A connection is no longer reused after a read that timed out or an answer cut short, whose late bytes could have been read as the next request's answer; a timed-out read, a create, a send or a move is never sent again.
- Fixed a flag change, a move or a delete pushed to Graph on a message gone from the server failing every run: it counts as done (a move is checked by reading the message, so a destination folder removed meanwhile still fails it), as does the delete of a Graph contact or event gone. A copy of a message gone still fails.
- Fixed Gmail's per-minute quota refusal failing the run: its answer (`Quota exceeded for quota metric 'Total Query Cost' and limit 'Units per minute per user'`) names its reason only as `rateLimitExceeded` in `errors[]`, which the throttle, reading the message for `Rate Limit Exceeded`, missed; it is now waited out.
- Fixed a Gmail write on a message gone already failing: removing a label, starring, moving or deleting from `TRASH` a message deleted since (404) now counts as done, a `batchModify` answered 404 being done again one message at a time.
- Fixed two endpoints holding one `UID` under two bodies before their first sync ending with the source's body on both, the target's own lost: the target's listing landed the create the source's item staged and read the source's body as a local edit (io-pimdir `189db40`). The divergence parks again, as one with no common ancestor must, and either `conflict resolve` direction reaches both endpoints at the next run, `--prefer-local` included (#31).
- Fixed a one-way account parking a divergence on its target and exiting 2: the target's pull recorded it as a conflict, its pass being unable to push, and the pushing pass never revisited it. The store's body is now kept over it and pushed in the same run, as `one-way` declares.
- Fixed a widening of the scope deleting the mail of no usable `Date` it did not list: a band round lists by the provider's date filter (IMAP `SENTSINCE`, which skips a garbled `Date`, Gmail's `after:` on the arrival, Graph's `sentDateTime`), which misses such mail, yet its last page inferred it gone. io-pimdir `ff28408` makes a band round infer no delete of an undated member, so the Graph listing no longer relists the bound undated messages on its last band page.
- Fixed an undated mail a scoped round no longer lists being kept when the round ran in one go but deleted when it was resumed: io-pimdir `f9b13f8` stores an empty mail date as no date, as STORAGE Annex A.1 says, so both paths read it in every scope.
- Fixed the date of a Microsoft Graph mail: the summary took `receivedDateTime`, the server's arrival time, where pimdir's mail `date` (and its sort key) is the `Date` header, which Graph gives as `sentDateTime`. A message without one has no date rather than its arrival time. A date already stored is corrected when Graph next reports the message.
- Fixed an IMAP delete expunging more than its message: it marked the UID `\Deleted` then sent a plain `EXPUNGE`, which also removed every message another client had marked `\Deleted` in that mailbox. It now sends `UID EXPUNGE` on that UID (RFC 4315 UIDPLUS, built into IMAP4rev2); a server offering neither gets no delete: the push is rejected, kept in the store, and no `\Deleted` flag is set.
- Fixed a Google People sync failing when People answers an expired sync token with HTTP 400 rather than 410: the token is dropped and a full listing restarts, as for a 410.
- Fixed a collection deleted on one endpoint of a two-endpoint account being created again from the other on the next run: the store tells a collection both endpoints held, now deleted on the other (`collection.delete` permitting), from one new on one side, created on the other as before. After a crossed delete the store drops the collection, so one made again later reads as new.
- Fixed a Graph series over more than five years failing to read, open-ended ones (the Birthdays and holidays calendars) among them: Graph refuses an instances listing over more than five years. A series is now read in windows of at most five years, an open-ended one within five years either side of today.
- Fixed a CardDAV or CalDAV item whose `UID` holds a `:` (a `urn:uuid:`) read as two resources and copied again on every run: Stalwart lists `urn:uuid:x.vcf` as `urn%3Auuid%3Ax.vcf`. A member now has one spelling, a `:` encoded as `%3A`, and a new resource is named in it. An item an older version created with a literal `:` is read under the new spelling.
- Fixed a calendar conflict parking over stamps alone: a side rewriting `DTSTAMP`, `LAST-MODIFIED`, `CREATED` or `SEQUENCE` is no longer a collision, so a Graph meeting edited right after its creation, while Graph restamped it on sending the invitations, merges instead of parking, and a later cancellation no longer recreates it. The merged event keeps the higher `SEQUENCE`.
- Fixed a Graph calendar series with a `numbered` or `noEnd` range failing to read, the Birthdays calendar among them: Graph fills its `endDate` with `0001-01-01`, which ended the instances window before its start, and Graph refused it (400) on every run. The window now follows the range type (io-msgraph 0.4.5).
- Fixed a Graph calendar enumeration listing an event twice when Graph repeated it across a page boundary; one id at two revisions now fails the enumeration.
- Fixed a move staged through the store landing twice in its target on IMAP: the run scans collections over several connections, and the target uploaded the message while the source relocated it. A source's collections now push one at a time, scans and fetches staying parallel.

## [0.3.0] - 2026-10-02

### Added

- Added Microsoft Graph contacts, the `msgraph-contacts` backend: the user's contact folders sync as address books, the default Contacts folder as `contacts`.

  Creates, updates and deletes push. A card keeps its vCard UID on Graph, in the extended property the projection stashes, so it matches its copy on another source. An edit made on Graph since the last sync is refused and picked up by the next run. The token needs the `Contacts.ReadWrite` scope.

- Added Microsoft Graph calendars, the `msgraph-calendar` backend: the user's calendars sync as collections, a recurring series and its exceptions as one item.

  Every run lists each calendar in full rather than through Graph's calendar view delta, whose time window would read an event leaving it as deleted.

  An event keeps its UID on Graph in a stash extended property, and Windows zone names read as their IANA counterpart. An exception edited locally does not push yet. The token needs the `Calendars.ReadWrite` scope.

- Added Google contacts and calendars through their native APIs, the `gpeople` and `gcal` backends, both in the default feature set.

  People syncs every connection as one address book, `contacts`. Calendar syncs each calendar of the user's list, a recurring series and its modified instances being one item.

  Both resume from a sync token and keep a card's or event's UID, so it matches its copy on another source. An instance of a series modified locally does not push yet. The token needs the `contacts` or `calendar` scope.

- Added Gmail through its native API, the `gmail` backend, in the default feature set.

  User labels and `INBOX`, `SENT`, `DRAFT`, `SPAM`, `TRASH` sync as collections named like on IMAP, `UNREAD`, `STARRED` and `IMPORTANT` as flags. Runs resume from the mailbox history id.

  A delete removes the collection's label, archiving from `INBOX`; only a delete from `TRASH` is permanent. The token needs the `https://mail.google.com/` scope.

### Changed

- Put `msgraph` back in the default feature set, now that Graph carries contacts and calendars as well as mail.

- Turned `vendored` on by default, so `cargo install` builds SQLite from source and needs none on the machine. Drop it to link the system SQLite and save about 1 MB; it also vendors OpenSSL when `native-tls` is on. The Nix builds still link the store's SQLite.

### Fixed

- Fixed a server edit made while an item sat in a cross-source conflict being dropped silently, and a remove settling a conflict leaving a base that claimed the old body, which hid the server's state once the item came back (io-pimdir 0.5.1).

- Fixed a queued send transmitting its `Bcc` field to every recipient over SMTP (RFC 5322 3.6.3). The Bcc recipients still receive it through the envelope.

## [0.2.0] - 2026-09-07

Neverest 0.2 is a full rewrite on top of the I/O-free `io-*` ecosystem. The CLI, the configuration schema and the sync engine all changed shape.

[MIGRATION.md](./MIGRATION.md) carries the upgrade path from v0.1.0. Nothing of an old setup is read: the configuration is rewritten and the first run starts from an empty store.

### Added

- Added the local **pimdir store**, the single local copy an app reads.

  One store per account at `neverest/<account>/` under the platform's state location, overridable with `store.root`: a SQLite index beside a content-addressed blob directory. Its presence is what says the account is initialized.

  The state location is `$XDG_STATE_HOME` on Linux and the BSDs, `~/Library/Application Support` on macOS and `%LOCALAPPDATA%` on Windows.

  Every collection is grouped under the account that syncs it and carries a display name, so a frontend reads `Work` where the id says `caldav/ED99C7C8`. Every item carries a sort key and a typed summary row, so a collection lists in its natural order without reading a body.

- Added the `init` command, run once per account before the first sync.

  It opens every source first, so credential and network errors surface up front. `sync` refuses to run without it, and `init` refuses to run over it.

- Added **named endpoints and a declared mode**: a `sources` table, optionally a `targets` one, and the flags `one-way` and `retain`.

  A map key is the pimdir source id, so a name is what every binding it owns is recorded under; a positional list would reassign them all on a reorder.

  A backend written directly under the account (`imap.server = "…"`) is sugar for a source named after its protocol, and the only shape the wizard writes.

  One source and one target sync both ways, several targets are one-way only, and several sources with no target is the offline replica. Every other combination is refused at load, naming the nearest legal one.

  An account may hold sources of several kinds: mail, contacts and calendar under one store.

- Added `one-way`, which declares authority rather than leaving both sides to merge.

  The `sources` side wins and the other's change is discarded, so nothing is reported as a conflict. The other side is still enumerated every run, or every item would be re-pushed.

- Added `retain`, which declares whether the store is a replica or only the ledger.

  The store is the ledger in every mode, holding the spine and the checkpoints. `retain` says whether it also holds bodies, defaulting from the destination: true with no target, false with one.

  `retain = true` alongside targets is honoured rather than refused, since migrating while keeping a copy is a thing to want. It makes the store a backup, so `sync --reset` then destroys data.

- Added the **mode guard**: the mode is stamped beside the store and compared on every run.

  Turning `one-way` on over an account that synced both ways is refused, that run being the one that discards what the previous mode merged. `sync --accept-mode` records the answer.

- Added **CardDAV** and **CalDAV** support via [io-webdav](https://github.com/pimalaya/io-webdav), behind the `dav` cargo feature.

  Address books and calendars are collections, keyed by their path segment and named by their `DAV:displayname`. Enumeration is RFC 6578 `sync-collection`, its token the engine's checkpoint; a server implementing none is listed with a `PROPFIND` instead.

  These are the first mutable-content backends, so they are the first to exercise revisions, conditional writes and conflicts, which mail leaves inert. Writes are `PUT` and `DELETE` on the last-synced ETag, so a remote that moved is rejected rather than overwritten.

  A link id is the vCard or iCalendar `UID`, falling back to a body digest. A calendar item is the object **resource**, not the component, so a recurring series and its overrides are one item (RFC 4791 §4.1).

- Added **Microsoft Graph** support via `io-msgraph`, behind the `msgraph` cargo feature, which is **not** a default.

  Delta-query enumeration, bodies through the raw MIME endpoint, flag and delete pushes. Appends and moves are pull-only, and auth is a bearer token from an external broker.

  Graph carries three domains behind one protocol and this backend syncs only mail, so a released binary is built without it. Build with `--features msgraph` meanwhile.

- Added the queued **`submit` intent** and its send channel.

  A frontend enqueues a submission through the store's action queue, and the row pins the body until the send. Every run performs the pending ones through the one source offering a channel.

  A transient failure leaves the row pending, a permanent one parks it. Submission is at-least-once, so deduplication is the receiving provider's job.

  The `smtp` table mirrors the `imap` one field for field. Omitting `sasl` is the unauthenticated relay, which stops after `EHLO`.

- Added `store.purge-after`, the retention sweep.

  The store retains an item rather than deleting it when its last binding vanishes. Each sync then purges every retained item older than this delay (`"90d"`, `"12h"`, `"0"`), collects garbage, and reports what it reclaimed.

  Unset never purges, `"0"` reproduces a terminal delete, and `sync --no-purge` skips one run. Beside a read-only source it makes a backup a remote expunge cannot lose.

- Added the **three-way merge** a run resolves a content conflict with.

  Most divergence is not disagreement: one side changed a phone number and the other a note, and the base the last sync agreed on says which side touched which field.

  Two endpoints of one account are merged against the body they both came from, which no endpoint's own reconcile can see. Both sides setting one field differently parks for a person.

  The merge is built in rather than configured: a pure function over stored bodies, with no taste in it. Mail is immutable-content and reaches none of it.

- Added the `conflict` command, the only place a collision is decided.

  `conflict list` names what is waiting, `conflict show <id>` prints the three bodies, and `conflict resolve <id>` settles one. `--prefer-local` and `--prefer-remote` discard a side, which a person may ask for by name and a run must never do on its own.

  Neverest raises no notification of its own: `--json` carries `conflicts` and `outstandingConflicts`, so a caller notifies on entry with no state to keep.

- Added `conflict.merger` and `conflict resolve --interactive`, handing a collision to a program of your own.

  The four paths are appended git-mergetool style. A command naming `{base}`, `{local}`, `{remote}` or `{output}` is substituted instead, which is the form `tcard merge` and `tcal merge` take.

  The result is taken only on a zero exit with the output written, since an editor exits zero on a bare quit, and only when it is a body of that item. No lock is held across the merger.

- Added exit code 2 for a run that reconciled its collections and still left something waiting.

  A parked conflict, a refused duplicate `UID` and a rejected write are one class: unresolved, re-reported every run, unchanged by a rerun. Failing instead would stop ten thousand items over one duplicated phone number.

  The outstanding count sits beside it in both output modes, read from the store rather than the run's tally.

- Added a warnings section for what a run could not deliver, under `refused` and `rejected` in `--json`.

  A refused create names the side, the collection and the `UID`; a rejected write names its reason and takes back the hunk, so a run that wrote nothing never reads as having written. Neverest repairs neither: which copy to keep is the user's call.

- Added the per-account sync.lock advisory file lock, so two concurrent runs cannot corrupt the store.

  It honours `store.root`, and a second run waits up to 60 seconds before exiting with a clear error.

- Added the store's action queue, neverest being its sole owner: every run drains it before it syncs.

  The queue drains store-wide in append order, the store applying each action as the source syncing its collection, so a queued action never waits for the side that owns its namespace. Each is applied exactly once.

- Added the engine's answer to a delete a source may not push: held beside another source of the collection, reverted for a source alone.

  A local edit whose member the server deleted is re-staged as a pending create rather than dropped.

- Added the **handle-space rebuild**: an IMAP `UIDVALIDITY` change drives io-pimdir's rekey.

  Bodies, summaries and pending state carry over by link id, and the collection's `generation` bumps atomically, so a frontend derives its epoch from the store alone.

- Added `--log-level` (alias `--log`) and `--log-file <PATH>`, where only `RUST_LOG` used to work.

- Added `sync --source <name>`, narrowing a run to the named sources.

- Added the `<protocol>.item.update` permission, gating in-place body edits.

  It defaults to `true` and only bites on a mutable-content backend.

- Added the `json-schema` command, describing what each data command prints under `--json`.

  One schema per command path, to stdout or one file per command with `--dir`. A consumer reading `conflicts` out of the sync payload has the shape written down rather than inferred.

### Changed

- **BREAKING**: moved the sync onto the [io-pimdir](https://github.com/pimalaya/io-pimdir) engine, replacing a hand-rolled three-way diff.

  An account's sources are the sources of one shared collection, so cross-source propagation of items, flags and deletions falls out of the shared hub. The store services the engine's storage seam; neverest's driver answers the remote.

- **BREAKING**: replaced `left` and `right` with the `sources` and `targets` tables and the `one-way` flag.

  They are refused at load in any form, naming what declares the direction they never could.

- **BREAKING**: made the sync vocabulary kind-neutral, turning a mail sync into a generic PIM sync.

  Everything above the backend seam speaks collections and items rather than folders and messages. The `folder` and `message` tables became `collection` and `item`, and so did the per-source permission tables.

  `-f` / `--include-folder`, `--exclude-folder` and `--all-folders` became `-m` / `--include-collection`, `--exclude-collection` and `--all-collections`.

- **BREAKING**: switched every `--json` key to camelCase, and replaced `-o json` with `--json`.

  camelCase matches the wire formats the endpoints speak and keeps every key reachable by dot access in jq, which neither `outstanding_conflicts` nor `message-id` was. TOML keys stay kebab-case.

- **BREAKING**: moved `collection.filter` from the account to the source it filters.

  An `include = ["INBOX"]` means nothing to a contacts source, so filters are asymmetric: a collection may be synced on one source and skipped on another.

- **BREAKING**: moved the SMTP submission channel onto the source it completes.

  Written `sources.<name>.smtp.*`, or `smtp.*` under an account whose mail backend is the sugar. Two channels are refused at load rather than resolved by configuration order.

- **BREAKING**: enforced per-source permissions per operation.

  They map onto the engine's per-kind push rights one to one, and a forbidden kind stays pending while the others propagate. A tightened block takes effect now where it previously did not.

- **BREAKING**: renamed `synchronize` to `sync`, `check-up` to `check`, and `completions` and `manuals` to `completion` and `manual`, the plurals staying as hidden aliases.

  The positional `<account>` became an optional `-a` / `--account <NAME>`, falling back to the entry marked `default = true`.

- **BREAKING**: made `check` and `init` print one payload rather than a run of prose lines.

  Separate messages meant several JSON documents on stdout and nothing a parser could read.

- Made every `server` accept a bare authority, port included, as well as a full URL.

  An authority takes the backend's default scheme, so `posteo.de:8843` resolves rather than reaching a backend hostless.

- Resolved every configured secret once per run instead of once per opened connection.

  A `password.command` used to be spawned inside the connection layer, so one source at `-j 4` ran it four times before its first request. A run now resolves them up front, memoizing identical commands.

  A credential that fails resolves against its own endpoint rather than the account, so a stale calendar entry no longer leaves mail unsynced. Nothing is logged but the command and its duration.

- Reduced the configuration wizard to a single input, an email address, deriving the account name from its domain.

  Discovery runs every mechanism in parallel (provider rules, PACC, Thunderbird Autoconfiguration, RFC 6186 SRV, RFC 6764 DAV) under a deadline, and every reachable service is proposed.

  Only backends compiled into the running build are offered, and only the SASL mechanisms the server advertises.

- Made `neverest configure` generate an account and never edit one.

  It appends the generated `[accounts.<name>]` table as plain text, so comments, ordering and hand-written formatting survive. The name is suffixed until free, and the account claims `default` only when no other does.

  `--json` or a redirected stdout prints the account and touches no file, so `neverest configure > config.toml` works. Editing one is a job for your editor, against [config.sample.toml](./config.sample.toml).

- Made the IMAP and SMTP `alpn` fields optional rather than defaulted in place, so io-imap and io-smtp own their default.

  The SMTP channel therefore offers the `smtp` ALPN token (RFC 7595) where it offered none. Set `smtp.alpn = []` to restore the old behaviour.

- Put every remote behind a cargo feature: `imap`, `msgraph`, `dav` (CardDAV and CalDAV together), plus `smtp`.

  All ship in the default set except `msgraph`. Every source config parses in every build, and opening a backend that was not compiled in reports it at runtime.

- Linked the system SQLite by default: `vendored` builds it from source alongside OpenSSL, so a plain `cargo install` needs sqlite3 headers and a released binary carries its own.

- Relicensed from `AGPL-3.0-only` to `MIT OR Apache-2.0`, aligning with the rest of the Pimalaya ecosystem.

- Bumped the Pimalaya libraries: io-pimdir 0.5, io-imap 0.6, io-smtp 0.3, io-webdav 0.3, io-http 0.5, io-pim-discovery 0.7, io-msgraph 0.3, ical-rs 0.5, vcard-rs 0.4, pimalaya-stream 0.3, pimalaya-cli 0.2 and pimalaya-config 0.2.

  SASL moved out of pimalaya-stream into the new io-sasl crate, so the SCRAM-SHA-256 the configuration has always offered is now runnable. The minimum supported Rust version is 1.89.

### Removed

- **BREAKING**: removed local file backends as sync sources (Maildir, then m2dir).

  A source is a remote and the pimdir store is the local replica, so a local file store beside it would be a second local copy. An existing tree comes in through io-pimdir's conversion tooling.

- **BREAKING**: removed the **Notmuch** backend, with no replacement in the `io-*` ecosystem yet.

- **BREAKING**: removed `folder.aliases`.

  The table parsed and nothing ever read it: substituting a friendly name for a backend id is display work, and neverest renders nothing. The store carries display metadata for the frontend that does.

- **BREAKING**: removed the built-in keyring and OAuth support.

  Secrets come from a command instead, so any secret manager works, and [ortie](https://github.com/pimalaya/ortie) issues and refreshes OAuth access tokens.

- **BREAKING**: removed `envelope.filter`, the `-o` output flag and the `-C` / `--color` flag.

  Color follows the terminal, and `--json` replaces `-o json`.

## [1.0.0-beta] - 2024-04-15

This version has been yanked. Use [0.2.0] instead.

### Added

- Added `--debug` as an alias for `RUST_LOG=debug`.

- Added `--trace` as an alias for `RUST_LOG=trace`.

- Added notes about `--debug` and `--trace` when an error occurs.

- Added `left|right.folder.aliases` to define custom folder aliases.

### Changed

- Replaced `anyhow` by [`color-eyre`](https://crates.io/crates/color-eyre) for better error management.

- Replaced `log` by [`tracing`](https://crates.io/crates/tracing) for better log management.

- Renamed `folder.filter` to `folder.filters` in order to match lib types.

- Renamed `envelope.filter` to `envelope.filters` in order to match lib types.

- Renamed `check` command to `doctor`.

## [0.1.0] - 2024-04-10

### Added

- Initiated the project from [Himalaya CLI](https://github.com/pimalaya/himalaya).

[Unreleased]: https://github.com/pimalaya/neverest/compare/v0.3.0...HEAD
[0.3.0]: https://github.com/pimalaya/neverest/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/pimalaya/neverest/compare/v0.1.0...v0.2.0
[1.0.0-beta]: https://github.com/pimalaya/neverest/compare/v0.1.0...v1.0.0-beta
[0.1.0]: https://github.com/pimalaya/neverest/compare/root...v0.1.0
