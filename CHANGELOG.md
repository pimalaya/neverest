# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/), and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Added Microsoft Graph mail writes through the store: a move pushes through Graph's own move, a copy through its copy, and an added message lands in Drafts, the one folder where Graph can create it, Graph filing every uploaded message as a draft. An add into another folder is rejected rather than filed there as a draft. Graph gives a moved or copied message a new id, and the store follows it.
- Added the `wizard` cargo feature, on by default, gating the interactive configuration: the `configure` command and the offer a first run makes. A build without it drops the prompts and the dependencies only they use, and a missing configuration points at the documented sample instead.
- Added capability declarations (pimdir draft-03, STORAGE §15.6): every run, before the queue drains, each mail, contacts and calendar source declares in the store what it can push or perform, from its backend and its configured rights alone, so himalaya, cardamum and calendula refuse an unsupported action before it is queued. JMAP sources stay undeclared.
- Added the sent copy: a `submit` asking for `copy` files the message there once sent, by an `add` queued in the same run, unless the provider files sent mail itself (Gmail, Graph). A failed send files no copy.
- Added the join link of a Graph online meeting (Teams) to the iCalendar of its event, as `X-MICROSOFT-SKYPETEAMSMEETINGURL` (io-msgraph 0.4.6).
- Added the calendar invitation intents (pimdir Annex B.2): a queued `calendar-reply` answers an invitation and a `calendar-cancel` cancels a meeting the account organises, through Graph's event actions (the comment carried) or Google's own notifications (`sendUpdates=all`, a cancellation without the comment). Each sync performs them first, one-way included, and reports them under `intents`; a refusal or a gone item parks, a transient failure retries. They are declared on each calendar the source syncs; CalDAV declares them `none`.

### Changed

- Changed the one-source sync to keep each body as it arrives: every downloaded batch is written to the store at once, instead of the whole account once everything had downloaded. A mail is readable minutes after a first sync starts, and a run stopped halfway (quit, sleep, a lost connection, a caller's time limit) keeps what it downloaded, the next run fetching only the rest.
- Changed `sync` to exit 3 when the run could not do all its work, a rerun picking it up: a source it could not reach, or a hunk, a send or an intent that failed without parking. An unreachable server used to exit 0, like a run with nothing to do. Exit 3 wins over the conflict code 2.
- Changed `check` to also open and authenticate the SMTP channel a source declares, without sending anything, so a wrong submission server or password fails the check rather than the first send. Each endpoint now reports `smtp`, whether its channel was checked.
- Changed a `submit` naming its sending source to be sent by that source alone.
- Changed Google Calendar updates and deletes to notify the attendees (`sendUpdates`): an update when the event leaves them to the server (`SCHEDULE-AGENT`), a delete when the event has attendees.
- Changed a new Google Calendar event the account organises, scheduled on the server, to be inserted with its UID and invite its attendees, rather than imported, which notified nobody.

### Fixed

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
