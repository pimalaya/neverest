# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

Upgrading from 0.3.0:

- Every store an earlier neverest wrote, 0.3.0 or a git build, is refused (`store-version`), never migrated: sync it with the neverest that wrote it until nothing is queued, upgrade, move it out of the neverest directory, then `neverest init` and sync (MIGRATION.md).
- Every account shares one store, `neverest/` under the state directory itself: a neverest directory still holding the per-account stores 0.3.0 kept is refused (`store-layout`), naming each, until they are moved out.
- `-a` is a filter: without it, `sync`, `check`, `drain` and `init` run over every account, and `default = true` is ignored. A script running one account names it with `-a`.
- `sync`, `drain`, `init` and `check` print one result listing every account under `--json` (`.accounts[]`, a sync's report under `.accounts[].report`), and `index` one listing every store (`.stores[]`).
- Store collection ids carry the account (`work/imap/INBOX`): a frontend naming a collection by its id names the new one, and reads one account through `pimdir.account`.
- `collection.delete` now defaults to false: set `collection.delete = true` on each endpoint whose collection deletions should reach the other.
- `sync --reset -a <account>` drops that account's collections and resyncs them, refusing while they hold work no server has, listing it; `--reset --force` drops the conflicts, unpushed changes and retained items too, a queued or parked action still refusing it.
- `sync` exits 2 for a parked send, intent or queue action too, and 3 when the run could not do all its work: a script must tell 2 and 3 from 1.
- A listing that leaves out more than half of at least 20 items a collection held, or names none while it held some, drops nothing and exits 2: `sync --accept-absences` confirms such absences, for one run.
- A store another user can read is refused (`store-exposed`): `chmod 700` its directory and `chmod 600` its pimdir.db.
- `sync --json` prints progress events before its report: read the last line, or skip the lines carrying `event`.
- Every `--json` output prints every key: an empty list as `[]`, an absent value as `null`, a bool as `false`. Test `.conflicts | length`, not whether `conflicts` is there.
- `check` checks every endpoint and exits 2, printing its report, when one does not answer; it exited 1 with the failure object of the first.

### Added

- Added `drain`: applies what frontends queued in the store without reading a credential or opening an endpoint, so a queued create gets its id at once, offline included.

  A store a sync holds answers `busy`, exit 3.

- Added `sync --intents-only`: performs what frontends queued (collection creations, sends, invitation replies and cancellations) and stops, listing no collection.

  An endpoint opens only when an intent needs it, so a send through an `smtp` table opens that channel alone.

- Added `sync --declare-only`: records every collection the sources hold, with its kind, name and role, syncing no item.

- Added `sync --reset --force`: drops the account's conflicts, unpushed changes and retained items with its collections, then resyncs them.

  A queued or parked action on the account's collections still refuses the reset, naming the row: `drain` applies a queued one, its frontend cancels a parked one.

- Added the orphan report to `check`: an account the store holds that no configured account names, as renaming an account's key leaves it, is listed under `orphans`, its collections left where they are.

- Added the `account-started` sync event, announcing each account of a run before its own events.

- Added the `store-layout` error code: a store laid out as an earlier neverest kept it (a store per account under the neverest directory, a neverest.json of an earlier shape, or a database with none), refused and never migrated.

- Added scoped mail syncs: `item.filter.since` in the account and `sync --since <WHEN>` bound a mail collection by its `Date` header.

  A message older than the floor is neither listed nor deleted, one with no usable `Date` is in every scope, and an explicit removal applies whatever the date. A wider scope lists only the band it lacks on IMAP, Gmail and Graph.

- Added paged rounds: a mail listing lands page by page, newest first, each page committed with its resume cursor, so an interrupted run resumes where it stopped.

  While a round is open, a run resuming it first lists what changed since it opened (IMAP, Gmail, Graph), so mail arriving during a long first sync shows at once.

- Added `sync --prime <N>`: every collection lists its newest page and downloads its N newest bodies before any collection lists its second page.

- Added `sync --bodies <all|listed>`: under `listed`, a run downloads only the bodies of what it listed.

- Added `sync --download-order <largest|newest>`: largest first by default, newest first to read recent mail early.

- Added `sync --max-wait <SECONDS>`, 120 by default: the longest single wait on a throttling provider before the source gives up for the run.

- Added a back-off on throttling for Microsoft Graph, Gmail, Google Calendar, Google People, CalDAV and CardDAV.

  A 429, a 502, a 503, a 504 or a Google rate limit is waited out as the provider states, else with a bounded exponential back-off, a create being sent again on a 429 or a rate limit only. A source throttled past `--max-wait` gives up for the run, reported under `throttled`.

- Added Gmail pacing, `gmail.units-per-minute` (6,000 by default).

- Added Gmail batched reads, `gmail.batch-size` (10 by default, at most 100) and `gmail.batches-in-flight` (1 by default).

  A batch with a refused read halves the next one, and a full batch with none doubles it back up to `gmail.batch-size`.

- Added collection roles in the store: every sync records the role each server states for a collection (`inbox`, `sent`, `drafts`, `trash`, `junk`, `archive`, `all`, `flagged`, `important`, or `default` for a calendar or an address book), never guessed from a name.

- Added capability declarations: each source declares in the store what it can push or perform, so a frontend refuses an unsupported action before queueing it.

  A CalDAV calendar whose component set leaves VTODO out, and every Google or Graph calendar, declares that it takes no tasks (`calendar.component.vtodo` `none`).

- Added the `collection-create` intent on IMAP, Gmail, CalDAV and CardDAV, performed at the start of a sync.

- Added the `collection-rename` and `collection-delete` intents on IMAP, Gmail, CalDAV and CardDAV, performed with the creations at the start of a sync.

  A rename keeps the parent, the mailboxes or labels under it following, and on an account syncing its source alone the store follows it, so the messages are not downloaded again. A delete parks while a collection stands under the one it names, and the store drops the collection once performed. Neither is offered on a collection stating a role (the inbox, a system folder or label, a default calendar); a delete only where `collection.delete = true`, and a rename not on an account with targets.

- Added the `mail.flags.important` and `mail.flags.junk` rows to a Gmail source's declaration: `$Important` is the IMPORTANT label, and `$Junk` is refused, spam being the SPAM collection.

- Added the `calendar-reply` and `calendar-cancel` intents on Microsoft Graph and Google Calendar, one occurrence of a series included.

- Added receipts for the intents neverest performs, so the frontend that queued one reads it applied.

- Added the sent copy: a `submit` asking for `copy` files the message there once sent, unless the provider files sent mail itself (Gmail, Graph).

- Added online meetings on push: an event carrying `X-PIMDIR-ONLINE-MEETING:TRUE` gets a Google Meet or a Graph online meeting.

- Added attachment stand-ins: a message body the store lands records one bodiless file per attachment in the account's attachments collection, referenced from the message.

- Added Microsoft Graph mail writes through the store: a move, a copy, and an add into Drafts, the one folder Graph creates a message in; an add elsewhere is rejected.

- Added CalDAV moves: a task filed into another calendar, or an item moved between calendars, is moved with WebDAV `MOVE`, so the server never holds its `UID` twice.

  A server without `MOVE` (405, 501, 404) gets a create then a delete; CardDAV and the other backends upload as before.

- Added `coverage`, `downloaded`, `unreached`, `unsettled` and `throttled` to the sync report, and `uncertain` to each `submitted` entry.

- Added the `wizard` cargo feature, on by default, gating `configure` and the offer a first run makes.

- Added `sync --accept-absences`: a listing leaving out more than half of at least 20 items a collection held, or naming none while it held some (a round, or the rebuild of a renumbered IMAP mailbox), drops what it left out, for this run.

  Without it such a listing drops nothing and is reported under `absences`.

- Added `absences`, `unreadable` and `overruled` to the sync report: the listings that left out most of a collection or all of it, the changes withheld because their body is missing from the store or does not match its name, and the bodies a policy did not adopt, kept as retained items, a body fetched to settle a divergence by the endpoint's policy included.

  A body the drain kept aside applying a queued action is named by its queue row (`action`), in the sync report and the `drain` output.

- Added `unanswered` to the sync report: the pushes a server gave no outcome for, pushed again by the next run, the run exiting 3.

- Added the `search` cargo feature, on by default: neverest builds the store's search index (index.db), which frontends query.

  A sync refreshes it last for at most `store.index-timeout` (60 seconds by default, `"0"` turning it off), a drain for 2 seconds; a refresh cut short goes on in the next run. The report says what it did under `index`, and a failure is a warning that changes no exit code.

- Added `index`: builds the store's search index whole, reading no credential and opening no endpoint.

  An index another indexer holds answers `busy`, exit 3.

- Added `store.index-timeout`, the longest a sync refreshes the search index.

- Added `partial` to the `conflict resolve` output: the capabilities a source supports in part that the next push relies on.

- Added `neverest-error`, the JSON Schema of the failure object.

- **BREAKING**: added progress events to `sync --json`, one line each before the report, carrying `event`: `opened`, `listed`, `collection-started`, `collection-progressed`, `collection-finished`, `scan-progressed` and `download-progressed`, described by `neverest json-schema neverest-sync.events`.

  A count prints at most once per percent. Text mode keeps the spinner, and `--json` no longer draws it.

- Added the JSON Schemas of `completion`, `manual` and `json-schema` (`neverest-completion`, `neverest-manual`, `neverest-json-schema`).

- Added `code` to the JSON failure object of a store failure: the code io-pimdir gives it (`store-version`, `sync-version`, `store-exposed`, `store-busy`, `store-owned`, `store-foreign`, `action-malformed`, `flag-invalid`, `index-fts5-missing`, `index-version` and the others its `PimdirError::code` lists), so a caller tells a reinstall, an upgrade or a retry from any other failure.

- Added the enumeration of a scoped mail round: every page of a round a date filter narrowed says so, and its last page hands every handle the collection holds, read with no date filter (IMAP `UID SEARCH ALL`, the Gmail label's ids, the Graph folder's ids from the pass closing the round).

  An undated draft received before the window, or a message whose `Date` runs ahead of its arrival, is no longer dropped from the store by a scoped round that did not list it.

### Changed

- **BREAKING**: changed every account naming no `store.root` to share one store, `neverest/` under the state directory, where each had `neverest/<account>/`.

  pimdir is multi-account by design: collections are grouped by account, a body two accounts hold is stored once, and a frontend reads every account from one store. Accounts naming one `store.root` share it too.

- **BREAKING**: changed the store's source ids to `<account>/<source>` and its collection ids to `<account>/<source>/<name>`.

  Source and collection ids are unique store-wide (pimdir STORAGE §4.3, §9.2): two accounts holding the wizard's `imap` source would have written into one collection and replaced each other's capabilities. Reports and events still name an endpoint as the configuration does. An account name holding `/` is refused.

- **BREAKING**: changed `-a/--account` into a repeatable filter: without it, `sync`, `check`, `drain` and `init` run over every account, one after the other under the store's one lock, and `index` builds every store the accounts use.

  `conflict show`, `conflict resolve` and `sync --reset` need exactly one account. An unknown name fails before anything runs, naming the configured accounts.

- **BREAKING**: changed the `--json` results of `sync`, `drain`, `init` and `check` to list every account (`accounts`), and of `index` to list every store (`stores`).

  An account that fails is reported with its error and code, the next one still runs, and the command exits 3.

- **BREAKING**: changed the sidecar (neverest.json) to keep the mode, the looked-up roles and the taskless calendars per account; one of the previous shape is refused (`store-layout`).

- **BREAKING**: changed `sync --reset` to start one account over inside the store it shares, dropping its collections rather than the store.

- Changed `default = true` on an account to be read and ignored, with a warning, and the wizard to write none.

- Changed the wizard to write one account covering every domain discovery found: one source per domain set up, mail, contacts and calendar, through the direct-backend sugar, and the mail source's `smtp` channel.

  It picks the best option per domain without a question and reviews the picks in plain words before any sign-in, signs each identity in once (one Ortie hand-off per authorization server, one password per group of servers of one name), tests it at once and stores a typed password in a keyring under `neverest-<account>-<source>`, then asks which collections each source syncs, mail starting without junk and trash. Nothing discovered asks for a server URL. No SASL mechanism, HTTP scheme or user id is asked any more.

- Changed a rendered account to read mail, its `smtp` channel, contacts then calendar, and to omit `collection.create` and `collection.delete` beside a collection filter when they hold their defaults.

- Changed the retention sweep to purge the account's own retained items alone, so one account's `store.purge-after` never empties another's trash.

- Changed the store-wide lock to make a second run over a held store report its accounts `busy` and exit 3 after the bounded wait, where it failed.

- Changed `init` to add an account to the store the accounts share, creating it when there is none, and to leave an initialized account as it is, saying so.

- Changed `conflict list` to list the conflicts of every account it runs over, each row naming its account.

- Changed `conflict resolve` and the merger to name the divergence they settle (pimdir STORAGE §15.3 `resolves`): a frontend's edit queued over a conflict now parks, naming it, where it settled the conflict and dropped the server's body silently. An item conflict between two sources, which `conflict list` does not show yet, parks a resolution rather than being settled unseen.
- **BREAKING**: changed `collection.delete` to default to false: a collection an endpoint no longer lists is deleted on the other only where that endpoint sets `collection.delete = true`.

  Absence is the only sign of a collection deletion IMAP and DAV give, and a listing can leave out a collection nobody deleted.

- **BREAKING**: changed `sync --reset` to refuse while the store holds work no server has (queued or parked actions, conflicts, unpushed changes, retained items), listing it.

  It is refused beside `--dry-run`, `-m`, `-x` and `-s`.

- Changed the `submit` payload to be read by io-pimdir's `PimdirSubmit`: one without `source` or a recipient, or naming a body (`object`, the message being the queue row's body), parks instead of being sent.

- **BREAKING**: changed `sync` to exit 2 when a send or an intent parked, or while the store holds a parked queue action, where it exited 0.

- **BREAKING**: changed `sync` to exit 2 when a listing left out more than half of at least 20 items a collection held, or named none while it held some, or a change was withheld over a body the store cannot serve, where the listing dropped them and the run exited 0.

- **BREAKING**: changed `sync` to exit 3 when the run could not do all its work, where it exited 0: an unreached endpoint (an SMTP channel included), a hunk, a send or an intent failed without parking, a throttled source, a round left open, or a collection still moving after the last convergence pass.

  Exit 3 wins over 2.

- **BREAKING**: changed the id of a CalDAV calendar or CardDAV address book outside the home set to its path, such as Google's `/caldav/v2/<calendar>/events`.

  A store synced before lists such collections as new.

- Changed a conflict resolution and a merged conflict to name the body they were decided on, so an edit that replaced it meanwhile parks rather than being dropped.

- Changed a divergence between two endpoints to be recorded by the store, which keeps the body they agreed on until both reconciled, where neverest read it from a snapshot held in memory for the run.

- Changed submission to at-most-once: a send that may have been delivered despite failing is parked as uncertain and never sent again, a run killed mid-send included.

- Changed `sync` to drain the queue once, at its start, before reading any credential or opening any endpoint.

- Changed every listed member to arrive named: IMAP, Graph and Gmail read what names a message in the listing itself, and DAV, Google and Graph contacts and events fetch each page's bodies before it lands.

- Changed body downloads to follow the listing page by page, each batch landing at once, so mail is readable before its mailbox is listed and an interrupted run keeps what it downloaded.

- Changed the one-source sync to take its collections by role: the inbox (or the default calendar or address book), sent, drafts, the others by name, the all-mail archive, junk, then the trash.

- Changed the Microsoft Graph sources to open three connections, or their `pool-size`, where they opened one.

- Changed Microsoft Graph mail body fetches to JSON batches of at most 20 messages and 4 MiB, each body written as it is decoded.

- Changed the Microsoft Graph mail listing to read a delta page by page, keeping a few octets a message.

- Changed the Gmail source to read each message and the mailbox history once a run, whatever the labels holding the message.

- Changed `check` to list each source's collections with their name and role, mark the default calendar and address book, list the source's capabilities, and open the SMTP channel without sending.

  `check` is a doctor: a program reads the store instead.

- Changed Google Calendar writes to notify the attendees (`sendUpdates`), and a new event the account organises to be inserted with its UID, inviting its attendees.

- Changed the UID of a Microsoft Graph event that came in by mail to the organizer's UID, which Exchange wraps in its global object id.

  Stored UIDs of such events change at the next sync.

- Changed Graph and Google conference links to standard `CONFERENCE` properties (RFC 7986), where `X-MICROSOFT-SKYPETEAMSMEETINGURL` and `X-GOOGLE-CONFERENCE` were written.

- Changed a `submit` naming its sending source to be sent by that source alone.

- Changed piped `--json` output to print one value per line, each `json-schema` schema carrying an `$id` naming the contract version.

- Changed the JSON Schemas to describe a value as printed: a key always printed is required, `null` or not.

- **BREAKING**: changed every `--json` output to print every key it declares, rather than leaving out an empty list, an absent value or a false `default`.

- **BREAKING**: changed `mode` in the `check` and `init` output from a sentence to an object, `{sources, targets, oneWay, retain}`.

- **BREAKING**: changed `check` to check every endpoint whatever the others answer: each row carries `backend`, `ok`, `code` and `error`, a failed one listing no collection, and a failed endpoint or SMTP channel exits 2 with the report.

  `smtp` is the channel's own row, `{backend, ok, code, error}`, or `null` when the endpoint declares none; it was a bool. Rows are sorted by name.

- Changed a collection whose full hydration fails to end on its error rather than `done`.

- Changed a body fetched to settle a divergence to settle it by the endpoint's policy: a one-way target's own edit is kept as a retained item and reported under `overruled`, a source's adopted, a two-way endpoint's left for the merge.

- Changed the store a dry run copies to be its user's alone, as the store is.

- Changed the search index to read floating times and all-day dates in the user's time zone, `TZ` else the system's, rather than in UTC.

  A zone with no IANA name reads as UTC, with a warning. The first refresh in another zone re-expands every calendar item.

- Bumped io-pim-discovery to 0.11.

  Discovery falls back to the autoconfig of the provider a domain's MX records name, and resolves the CalDAV and CardDAV roots a provider advertises (RFC 6764).

- Bumped io-pimdir to 0.9 (pimdir storage-draft-09, sync-draft-08, search-draft-09): a store any earlier neverest wrote is refused, never migrated.

- Changed a local edit over a body the store never read to wait for it: the run reads that body first, so a server edit made meanwhile conflicts instead of being overwritten (io-pimdir 0c70144).

- Changed a copy or a move of an item whose body the store does not hold to fail unless its source syncs the target (io-pimdir 0c70144).

- Bumped rusqlite to 0.41.

- Bumped ical-rs to 0.7.

- Bumped io-gcal to 0.3.

  An event giving its end as a `DURATION` is pushed to Google Calendar, no longer refused.

- Bumped io-gmail to 0.5.

- Bumped io-gpeople to 0.6.

- Bumped io-http to 0.6.

- Bumped io-msgraph to 0.7.

- Bumped io-smtp to 0.6.

- Bumped io-webdav to 0.7.

  A CalDAV or CardDAV multistatus is parsed 3.4 to 9 times faster.

- Bumped vcard-rs to 0.6.

### Fixed

- Fixed `sync --accept-mode` being refused like the run it answers: it lets that run through and records the new mode.

- Fixed a send the SMTP server refused for a sending rate or quota (a 5xx such as `550 5.4.5`) being parked: it stays queued for a later run, and the run attempts no other send.

- Fixed an IMAP or Graph mail copy whose answer was lost being filed again under a date scope: the target is searched by `Message-ID` first, and a message found there is bound instead.

- Fixed a message sent to oneself through Gmail's IMAP and SMTP, its copy asked in Sent, being marked read in the inbox: Gmail files that copy itself, so none is staged.

- Fixed an IMAP `SELECT` answer without `EXISTS` read as an empty mailbox, which deleted every message: such a mailbox is searched.

- Fixed a renumbered IMAP mailbox (a moved `UIDVALIDITY`) listed under its new UIDs before it was rekeyed, which pushed flags to the wrong messages and dropped every message as deleted.

  The collection is rekeyed by link id first, and a rekey that failed runs again at the next sync.

- Fixed a CardDAV or CalDAV listing the server truncated read as complete, which inferred every member past the cut deleted.

- Fixed a body deleted between its listing and its download failing the run: a read answered 404 (410 on DAV, Google Calendar and Google People too) is gone, and the member stays as listed until the next listing takes it away.

  A CardDAV, CalDAV, Google Calendar or Google People member every read finds missing no longer fails its collection in every run.

- Fixed hidden Google calendars missing from the listing, so hiding one in the Google UI read as deleting it.

- Fixed Microsoft Graph contact folders read as one page of 100, two levels deep: every page and every depth is read.

- Fixed a collection deleted on one endpoint of a two-endpoint account being created again from the other.

- Fixed a store written by a newer neverest refused as one predating it, with the advice to reset it: it asks for an upgrade.

- Fixed neverest.json being rewritten in place, so a killed run could leave a torn file: it is written aside, synced and renamed.

- Fixed the relay appending a message twice into a target that had not finished listing its mailbox: a message is relayed once the target closed a round covering its date.

- Fixed two endpoints holding one `UID` under two bodies before their first sync ending with the source's body on both: the divergence parks ([#31](https://github.com/pimalaya/neverest/issues/31)).

- Fixed a one-way account parking a divergence on its target: the store's body is pushed, as `one-way` declares.

  The target's own edit is kept in the store as a retained item and reported under `overruled`, an unread one fetched first.

- Fixed a calendar conflict parking over `DTSTAMP`, `LAST-MODIFIED`, `CREATED` or `SEQUENCE` alone: such edits merge, the higher `SEQUENCE` kept.

- Fixed a move staged through the store landing twice in its IMAP target: a source's collections push one at a time.

- Fixed an IMAP delete expunging every message marked `\Deleted` in its mailbox: it sends `UID EXPUNGE` on its UID, and a server without UIDPLUS gets no delete.

- Fixed a CalDAV or CardDAV delete of a member already gone (404, 410) retried every run: it is done.

- Fixed a CardDAV or CalDAV item whose `UID` holds a `:` read as two resources and copied again every run: a `:` is spelled `%3A`.

- Fixed a DAV request failing when the server closed the connection before answering (Radicale after a `PUT`): it is sent again once.

- Fixed a flag kept in the store only, on a card or a calendar item, pushed and rejected every run: it stays local.

- Fixed a flag another client changed between a run's listing and its push being overwritten: a push names only the flags it adds and removes, on IMAP, Microsoft Graph and Gmail.

- Fixed a `UID` or `Message-ID` beginning like a key the store derives (`alt:`, `hash:`, `dup:`) named to the server with the store's `esc:` escape.

- Fixed the date of a Microsoft Graph mail, which was its arrival time: it is its `Date` header (`sentDateTime`).

- Fixed a Microsoft Graph series over more than five years failing to read: it is read in windows of five years, an open-ended one within five years either side of today.

- Fixed a Microsoft Graph series with a `numbered` or `noEnd` range failing to read (the Birthdays calendar).

- Fixed a Microsoft Graph series doubling its cancelled occurrences on read.

- Fixed a Microsoft Graph calendar listing an event twice across a page boundary: one id at two revisions fails the listing.

- Fixed a flag change, a move or a delete on a Microsoft Graph message gone failing every run: it is done, as is the delete of a Graph contact or event gone.

- Fixed a Gmail write on a message gone failing: it is done.

- Fixed Microsoft Graph requests failing on a connection Graph closed while it idled: an idle connection is reopened, and a read meeting a closed one is sent again once.

- Fixed a Gmail message filed under several labels downloading its body once a label.

- Fixed a Google People sync failing on an expired sync token answered 400: it restarts, as on 410.

- Fixed a Google Calendar series updated from its first `VEVENT` rather than its master.

- Fixed iCalendar lines written from Microsoft Graph or Google left unfolded: they fold at 75 octets.

- Fixed the size, attachment mark and invitation of a message: each body entering the store states them, downloaded or filed again from a body the account holds.

- Fixed an HTTP response buffered whatever its size: a head over 64 KiB or a body over 256 MiB fails its exchange, and a connection closed mid-body fails it rather than hang the run.

- Fixed an IMAP batched body fetch buffering a message of any announced size: one over 100 MiB is fetched alone.

### Security

- Bumped crypto-bigint to 0.7.10, ctutils to 0.4.6, rustls-webpki to 0.103.16 and webpki-root-certs to 1.0.11, fixing RUSTSEC-2026-0382, RUSTSEC-2026-0383, RUSTSEC-2026-0385 and RUSTSEC-2026-0387.

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
