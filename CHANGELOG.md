# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [1.0.0] - 2026-11-25

Neverest 1 is rebuilt around the pimdir store: every account syncs into one shared store, which the Pimalaya frontends read, search and queue their writes into. It syncs mail, contacts and calendars over IMAP, Gmail, Microsoft Graph, Google Calendar, Google People, CalDAV and CardDAV. The wizard is gone, unknown keys are refused, and `--json` follows the Pimalaya output contract.

A 0.x configuration and state are not carried over: start over from config.sample.toml, move the 0.x stores out of neverest/ under the state directory, then run `neverest sync --init`.

### Added

- Added `sync --init`, which initializes the accounts the run covers that are not yet initialized, then syncs.

  The result names them under `initialized`. A sync over an uninitialized account is refused, naming `neverest init -a <name>` and `neverest sync --init -a <name>`.

- Added `sync --queue-only`, which applies what frontends queued in the store without reading a credential or opening an endpoint.

  A queued create gets its id at once, offline included, its rows under the result's `queue`. A store a sync holds answers `busy` (exit 4).

- Added `sync --intents-only`, which performs what frontends queued (sends, collection changes, invitation replies and cancellations) and nothing else.
- Added `sync --declare-only`, which records every collection the sources hold with its kind, name and role, syncing no item.
- Added the `search` cargo feature, on by default, which builds the store's search index (index.db) that frontends query.

  A sync refreshes it last for at most `store.index-timeout` (60 seconds by default, `"0"` turning it off), reported under `index`. `sync --index-only` builds it whole, opening no endpoint.

- Added `mail.filter.since` and `sync --since <WHEN>` (`30d`, `6mo`, a date or an RFC 3339 instant), which bound mail by its `Date` header.

  Mail older than the floor is neither listed nor deleted. A wider scope later lists only the band it lacks on IMAP, Gmail and Graph.

- Added paged mail listings, newest first, each page's bodies downloading as it lands.

  Mail is readable before its mailbox is listed, and an interrupted run resumes where it stopped.

- Added `sync --prime <N>`, `sync --bodies <all|listed>` and `sync --download-order <largest|newest>`, which make recent mail readable early on a large account.
- Added a back-off on throttling for Microsoft Graph, Gmail, Google Calendar, Google People, CalDAV and CardDAV.

  A source throttled past `sync --max-wait` (120 seconds by default) gives up for the run and is reported under `throttled`.

- Added `gmail.units-per-minute` (6000 by default), `gmail.batch-size` (10, at most 100) and `gmail.batches-in-flight` (1).
- Added a guard on mass absences, reported under `absences` (exit 3).

  A listing leaving out more than half of at least 20 items, or naming none while the collection held some, drops nothing. `sync --accept-absences` confirms such absences for one run.

- Added the holding of a change a server refuses for good, reported by every run as a `refusals` error (exit 3).

  An HTTP 4xx naming the request or an IMAP `NO [CANNOT]` or `[TOOBIG]` holds it in the store, pushed again only once the item changes.

- Added a clean stop on SIGINT and SIGTERM: `sync` lets what is in flight land, prints its result with `interrupted` set and exits 4.

  A second signal ends it at once (130).

- Added collection roles and capabilities to the store.

  Each source records the role a server states for a collection (`inbox`, `sent`, `drafts`, `trash`, `junk`, `archive`, `default` and others) and what it can push, so a frontend refuses an unsupported action before queueing it.

- Added the `collection-create`, `collection-rename` and `collection-delete` intents on IMAP, Gmail, CalDAV and CardDAV.

  Each intent neverest performs leaves a receipt the frontend that queued it reads.

- Added the `calendar-reply` and `calendar-cancel` intents on Microsoft Graph and Google Calendar.
- Added the sent copy: a queued `submit` asking for `copy` files the message there once sent.

  Gmail labels the sent message instead. Graph files sent mail in Sent Items alone, so a copy asked elsewhere is refused when queued.

- Added native sending on Gmail through `messages.send`, so a Gmail account needs no `smtp` table.

  An `smtp` table, under the account or the source, still sends in its place. The token needs the `gmail.send` or the `https://mail.google.com/` scope.

- Added Microsoft Graph mail moves, copies and adds into Drafts.

  Graph creates a message in Drafts alone, so a frontend refuses an add elsewhere up front.

- Added the push of a recurring event's overrides and cancelled occurrences to Microsoft Graph and Google Calendar.

  An override neither provider can take is refused, coded `unsupported`.

- Added online meetings on push: an event carrying `X-PIMDIR-ONLINE-MEETING:TRUE` gets a Google Meet or a Graph online meeting.
- Added attachment stand-ins: a landed message body records one bodiless file per attachment in the account's attachments collection.
- Added CalDAV moves through WebDAV `MOVE`, a server without it getting a create then a delete.
- Added `collection.rename` and `collection.capped` to every backend table.

  `collection.rename` says whether a frontend may rename a collection (false on a `one-way` account by default). `collection.capped` marks a server listing at most a number of items, so what it leaves out is kept.

- Added `proxy.url`, `proxy.username` and `proxy.password` at the account, under every backend table and under `smtp`.

  Without them, the `all_proxy` and `https_proxy` environment variables apply.

- Added an account-level `tls` table, inherited field by field by every backend's and every `smtp`'s `tls` table.
- Added `tls.client.cert` and `tls.client.key` under every backend table and `smtp`, a client certificate for mutual TLS.
- Added `imap.sasl-ir`, `imap.id.auto` and `imap.id.fields`.
- Added `carddav.home` and `caldav.home`, the home set as a URL with nothing discovered.
- Added `auth = "none"` to CardDAV and CalDAV, the default of a table with no `auth`.

  A 401 then says no `auth` is configured.

- Added progress events to `sync --json`, one line each before the result, described by the `neverest-sync.events` schema.

  Read the last line, or skip the lines carrying `event`.

- Added a stable `code` to the JSON failure object and to every failing row, described by the `neverest-error` schema.

  Codes include `store-version`, `store-layout`, `store-exposed`, `backend-not-built`, `account-missing`, `account-uninitialized`, `conflict-not-found`, `item-duplicate`, `permission-denied`, `quota-exceeded`, `rate-limited`, `recipient-refused` and `usage-invalid`.

- Added `account list` (alias `ls`), listing the accounts with the backend tables each writes, those this build leaves out under `notBuilt`.
- Added `coverage`, `downloaded`, `unsettled`, `unreadable`, `overruled`, `unanswered`, `waiting`, `disagreed`, `unlisted`, `forbidden`, `clipped` and `intents` to the sync report.
- Added `direction` to the `add-flags`, `remove-flags` and `delete` hunks.

  `pull` is a server change the store took in, `push` one the run applied on that server.

### Changed

- **BREAKING**: Changed every account naming no `store.root` to share one store, neverest/ under the state directory.

  Collection ids carry the account (`work/imap/INBOX`), so a frontend reads every account from one store. An account name holding `/` is refused.

- **BREAKING**: Changed `-a` into a repeatable filter, every command running over every account without it.

  `conflict show`, `conflict resolve` and `sync --reset` need exactly one account. An account that fails is reported and the next one runs (exit 4); a bare `conflict list` names those it skipped as uninitialized under `uninitialized`.

- **BREAKING**: Changed the exit statuses to the Pimalaya contract: 2 a refused command line, 3 a run that left something for a person (2 in 0.3.0), 4 a run that could not do all its work.

  Under `--json`, a refused command line prints the failure object coded `usage-invalid`.

- **BREAKING**: Changed the `--json` results of `sync`, `init` and `account check` to list every account under `accounts`, a sync's report under `.accounts[].report`.
- **BREAKING**: Changed every `--json` output to print every key it declares (an empty list as `[]`, an absent value as `null`), one value per line when piped.
- **BREAKING**: Changed a sync report's patches into the `collectionPatch` and `itemPatch` arrays of the hunks applied.
- **BREAKING**: Changed every failure of a sync report into one row of `errors`, tagged by `kind`.

  A row is pimalaya-cli's item error `{id, code, error, sources}` then the keys its kind needs. The kinds are `collection-patch`, `item-patch`, `failures`, `rejected`, `duplicates` (0.3.0's `refused`), `refusals`, `submitted`, `intents` and `unreached`.

  A `rejected`, `duplicates` or `refusals` row and a parked send or intent exit 3, any other row 4.

- **BREAKING**: Changed a parked queue row, of a sync and of `sync --queue-only`, to read as `pimdir queue list` prints a row.

  Its keys are `row` (was `id`), `handle`, `kind`, `collection`, `linkId`, `itemId`, `producer`, `queuedAt`, `attempts`, `unconfirmed` and `error`.

- **BREAKING**: Changed the `conflict` commands to print an item's `id` as a string and its handle on the source as `remoteId`.

  `conflict show --json` prints each body as `{contents, encoding}`: the text when it is valid UTF-8, its base64 otherwise, `null` when absent. A sync report's conflict row prints the `id` `conflict resolve` takes and the `collection` as `conflict list` spells it.

- **BREAKING**: Changed the conflicts to include an item's own conflict, when two endpoints of an account disagree.

  `conflict list`, `show` and `resolve` name it with `source` and `remoteId` `null`, a run merges it against the body the endpoints last agreed on, and `outstandingConflicts` counts it.

- **BREAKING**: Changed `check` into `account check`, `check` staying a hidden alias.

  It checks every endpoint and exits 3 when one does not answer, where it failed on the first. `orphans` lists the accounts the store holds that the configuration no longer names.

  Each account lists its endpoints under `backends` (was `sources`), each row `{backend, ok, code, error, tls}` then `source`, `collections` with their roles, `capabilities` and `smtp` (opened without sending), then the account's `mode`.

- **BREAKING**: Changed the configuration to refuse every key neverest does not read, at every level, every key an earlier neverest read included.
- **BREAKING**: Changed Microsoft Graph to one `msgraph` table, its domains under `msgraph.mail`, `msgraph.contact` and `msgraph.calendar`.

  The `msgraph-contacts` and `msgraph-calendar` tables are gone. Each domain is a source named `msgraph-mail`, `msgraph-contact` or `msgraph-calendar`; an `msgraph` table naming no domain is refused, so add `msgraph.mail = true` to sync mail.

- **BREAKING**: Changed `pool-size` into a key of the Graph domain tables alone; IMAP connections are bounded by `connections`.
- **BREAKING**: Changed `conflict.merger` into `contact.merger` and `calendar.merger`, written once above the accounts.

  An account naming its own overrides it. `conflict resolve --interactive` runs the merger of the conflict's kind, and refuses naming the key when that kind has none.

- **BREAKING**: Changed the `dav` cargo feature into `carddav` and `caldav`.
- **BREAKING**: Changed `-c` to take one path, repeated to merge several files in order, where it split one value on `:`.
- **BREAKING**: Changed `collection.delete` to default to false.

  Set it on each endpoint whose collection deletions should reach the other.

- **BREAKING**: Changed a target's `smtp` table into a configuration error.

  An account sends through a source, and a queued send naming a target parks.

- **BREAKING**: Changed a configured host that is not ASCII into a load error naming its Punycode form (`xn--...`).
- **BREAKING**: Changed a store another user can read to be refused (`store-exposed`).

  `chmod 700` its directory and `chmod 600` its pimdir.db.

- **BREAKING**: Changed submission to at-most-once.

  A send that may have been delivered despite failing parks as unconfirmed and is never sent again, a run killed mid-send included. A `submit` naming its sending source is sent by that source alone, and one without a recipient parks.

- **BREAKING**: Changed `sync --reset` to refuse while the account holds work no server has.

  A queued or parked action always refuses it. Conflicts, unpushed changes, retained items and items holding local properties (a label) refuse it unless `--force` drops them.

- **BREAKING**: Changed the text output to plain lines on stdout before the result, with no spinner and no colour.

  A full `sync` prints its progress to a pipe too, a download once per percent; `account check`, `init`, `sync --queue-only` and `sync --index-only` print it on a terminal alone. Scripts read `--json`, whose last line is the result.

- Changed `init` to add an account to the shared store, creating it when there is none, and to leave an initialized account as it is.
- Changed a run over a store another run holds to report its accounts `busy` and exit 4 after the bounded wait.
- Changed `completion`, `manual` and `json-schema` without their names or `--dir` into a refused command line (exit 2, coded `usage-invalid`).
- Changed a bare `neverest` without a configuration to fail (exit 1), showing a person at a terminal the commands creating one from the shipped sample.
- Changed config.sample.toml, rewritten and shipped as share/doc/neverest/config.sample.toml.
- Changed a frontend's edit or move queued over an unsettled conflict to park, naming the conflict.
- Changed a Gmail source to refuse a keyword other than `$Important` before it is queued, where Gmail dropped it.
- Changed Google Calendar writes to notify the attendees (`sendUpdates`).
- Changed Google and Graph conference links into standard `CONFERENCE` properties (RFC 7986).
- Changed syncs to read less from Microsoft Graph, Gmail, Google People, Google Calendar, CalDAV and CardDAV.

  Graph opens three connections (or `pool-size`), Gmail reads a message once a run whatever its labels, and Google People reads 200 contacts a request. A settled collection, a quiet Gmail label or a DAV collection whose sync token has not moved is listed once a run, or not at all.

- Changed `store.root` and `tls.cert` to expand `$VAR` as well as `~`.
- Changed a failing secret command's error to end with ", run `<command>` yourself to see why".

### Removed

- **BREAKING**: Removed the configuration wizard: the `configure` command (alias `cfg`) and the offer a bare `neverest` made without a configuration.
- **BREAKING**: Removed the `default` account key, now refused as unknown: `-a` names the accounts a command runs over.
- **BREAKING**: Removed the `mailbox`, `message` and `filters` aliases of `collection`, `item` and `collection.filter`.
- **BREAKING**: Removed the `NEVEREST_CONFIG` environment variable: pass `-c`.
- Removed the `NEVEREST_PROFILE` profiler.

### Fixed

- Fixed an IMAP delete expunging every message marked `\Deleted` in its mailbox.

  It sends `UID EXPUNGE` on its UID, and a server without UIDPLUS gets no delete.

- Fixed an IMAP `SELECT` answer without `EXISTS` being read as an empty mailbox, which deleted every message.
- Fixed a listing cut short being read as complete, which deleted what it left out.

  This covers an IMAP row that does not decode, and a CalDAV or CardDAV answer truncated, cut short or listing a member with an error. It is reported as a `failures` error and the run exits 4.

- Fixed a renumbered IMAP mailbox (a moved `UIDVALIDITY`) pushing flags to the wrong messages and dropping every message as deleted.

  It is rekeyed by link id, and the messages the rebuild finds new are reported added.

- Fixed an IMAP server advertising QRESYNC and enabling nothing (iCloud) never bringing new mail, flag changes or expunges.
- Fixed IMAP moves on servers advertising no MOVE, such as OVH ([#35](https://github.com/pimalaya/neverest/issues/35)).

  The move is a `UID COPY` then the expunge of the source UIDs alone; a server without UIDPLUS refuses it before any command.

- Fixed an IMAP body download after a failed batch storing a message under another's handle.

  A message over 100 MiB is fetched alone, and an HTTP response over 256 MiB fails rather than being buffered.

- Fixed an IMAP push aborting on every run over a message flagged `\Junk`, `\Important` or `\Forwarded`.
- Fixed an IMAP append on a server without UIDPLUS being bound to another message whose `Message-ID` holds its own.
- Fixed a message moved into an IMAP mailbox holding another with the same `Message-ID` being bound to that other message.

  A twin the store listed before the move still shows the moved message's body.

- Fixed an IMAP relay that met a refused message failing its whole mailbox on every run.

  The message is reported rejected and the relay goes on. A body of another size than listed never completes its append.

- Fixed CalDAV and CardDAV writes retried or misread every run: a delete of a member already gone, a 207 with a failed member, a `UID` holding `:`.
- Fixed a body deleted between its listing and its download failing the run.
- Fixed a write on an item already gone failing every run on Microsoft Graph, Gmail, Google Calendar and Google People.
- Fixed a flag another client changed between a run's listing and its push being overwritten.
- Fixed a collection deleted on the server staying in the store with its items.

  An account with no target retires it, reported as a `retire` collection hunk. One a queued or parked row anchors on is kept, reported waiting for a person (exit 3).

- Fixed a collection new on the server landing in the store unreported: it is a `fetch` collection hunk.
- Fixed a queued delete the server's edit overruled reading as applied: it is a `delete-overruled` item hunk.
- Fixed a queued move reporting its create twice.
- Fixed a move between address books or calendars reporting only its delete.
- Fixed a flag kept in the store alone, on a card or a calendar item, reported as pushed on every run.
- Fixed a write a source's rights forbid being listed as applied on every run: it is counted under `forbidden`.
- Fixed a pair collection whose sync failed exiting 0 as "already in sync": it is a `sync` hunk with its error (exit 4).
- Fixed failed store steps (reading or draining the queue, landing bodies, counting conflicts) leaving the result clean: each is a `failures` error (exit 4).
- Fixed a text run whose every change failed reading as partially synchronized: it prints "not synchronized: 3 errors".
- Fixed a dry run printing `applied N queued action(s)`: it prints `would apply N queued action(s)`.
- Fixed `sync --dry-run` copying a store a frontend may be writing as files: its replica is one snapshot.
- Fixed an edit made on a one-way target alone being pulled into the store, or parked as a divergence.

  The store's body is pushed back over it, the target's edit kept as a retained item under `overruled`.

- Fixed two endpoints holding one `UID` under two bodies before their first sync ending with the source's body on both: it parks ([#31](https://github.com/pimalaya/neverest/issues/31)).
- Fixed `sync --accept-mode` being refused like the run it answers.
- Fixed `sync -m` and `sync -x` unable to name an iCloud or Google calendar listed under an opaque id.

  An entry no id matches also matches the collections it is the display name of. A configured `collection.filter` still matches ids alone.

- Fixed a target's `collection.filter` being ignored.
- Fixed `conflict resolve --interactive` exiting 0 when the merger decided nothing: it prints `aborted` and exits 3.
- Fixed `conflict resolve` and the conflict auto-merge misjudging their own result when another app's queued row parked in the same drain.
- Fixed a merged event holding an alarm getting its `SEQUENCE` twice, once inside the `VALARM`.
- Fixed a card or an event whose `UID` follows a blank line or a large inline photo being copied each way between two endpoints.
- Fixed a message stored twice when its listing and its body key it differently (a commented or folded `Message-ID`, a field Graph decodes otherwise).
- Fixed a second rekey or a delivered copy making another source remove a message from its server.
- Fixed a queued move into its own collection deleting the item: it parks.
- Fixed a joined mail binding not pushing its flags.
- Fixed a locally edited contact or event losing its summary, and an item listed with the summary of a body it does not hold.
- Fixed a refused offered create being pushed and refused every run: it is held as that source's pending create.
- Fixed a hostile time zone rule aborting a calendar pull.
- Fixed a recurrence rule's `UNTIL` shifted wrongly between UTC and a series zone only the time zone database knows.
- Fixed Microsoft Graph recurring series failing to read over more than five years or with a `numbered` or `noEnd` range, or doubling their cancelled occurrences.

  An endless series starting earlier (a birthday Outlook stores from 1604) is listed under `clipped` with the date its exceptions are read from. Older exceptions keep their last synced body.

- Fixed the date of a Microsoft Graph message, which was its arrival time: it is its `Date` header.
- Fixed Microsoft Graph contact folders past the first page of 100 or below the second level going unread.
- Fixed Microsoft Graph mail folders below the second level never syncing, and a top-level `A/B` colliding with the child `B` of `A`.

  A `/` in a folder name is escaped as `%2F` in its id.

- Fixed an event deleted on a Graph calendar right after its creation coming back.

  An event without a description is created with no body. Exchange still rewrites one with a description a moment later.

- Fixed an event at a floating time queued on a Google or Graph calendar being refused by every sync.

  It is anchored in the calendar's zone, else the user's, and a recurrence bound is written in UTC.

- Fixed hidden Google calendars missing from the listing, so hiding one in the Google UI read as deleting it.
- Fixed a Google contact update erasing what a vCard cannot show: an organization after the first, a relation other than spouse or child, a custom type.
- Fixed a Google Calendar or Google People request written to a stream that failed during the one before.
- Fixed a Google contacts listing answering a negative `totalItems` failing to parse.
- Fixed a Google calendar that refuses every sync token, a public holiday one, warning at each run.
- Fixed a delete or a move out of Gmail's Drafts or Sent being refused by Gmail on every run.

  A delete trashes the message, and a move other than to the trash is refused at once.

- Fixed a send the SMTP server refused for a sending rate or quota being parked: it stays queued for a later run.
- Fixed a credential command prompting for a password (`systemd-ask-password`) hanging the run.
- Fixed a bare `neverest --json` printing the text help: it fails coded `usage-invalid` (exit 2).
- Fixed install.sh saving an HTTP error page as the archive.

### Security

- Fixed text output printing what servers wrote raw (bodies, names, ids, refusal reasons): every text line and error report goes through the sanitizer.
- Changed `--log-file` to create its file at 0600 and refuse a symbolic link.
- Changed a TOML parse error to no longer print the source line, and a refused `Secret` to never echo its value.
- Fixed RUSTSEC-2026-0346 to RUSTSEC-2026-0349 by bumping rustls to 0.23.46.
- Fixed RUSTSEC-2026-0382, RUSTSEC-2026-0383, RUSTSEC-2026-0385 and RUSTSEC-2026-0387 by bumping crypto-bigint, ctutils, rustls-webpki and webpki-root-certs.
- Fixed RUSTSEC-2026-0592 by bumping url to 2.5.9.

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

[Unreleased]: https://github.com/pimalaya/neverest/compare/v1.0.0...HEAD
[1.0.0]: https://github.com/pimalaya/neverest/compare/v0.3.0...v1.0.0
[0.3.0]: https://github.com/pimalaya/neverest/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/pimalaya/neverest/compare/v0.1.0...v0.2.0
[1.0.0-beta]: https://github.com/pimalaya/neverest/compare/v0.1.0...v1.0.0-beta
[0.1.0]: https://github.com/pimalaya/neverest/compare/root...v0.1.0
