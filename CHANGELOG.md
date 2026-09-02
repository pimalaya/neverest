# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).


## [Unreleased]

## [1.0.0] - 2026-09-02

Neverest v1 is a full rewrite on top of the I/O-free `io-*` ecosystem. The CLI, the configuration schema and the sync engine all changed shape.

[MIGRATION.md](./MIGRATION.md) carries the upgrade path from v0.1.0. Nothing of an old setup is read: the configuration is rewritten and the first run starts from an empty store.

### Added

- Added the local **pimdir store**, the single local copy an app reads.

  Each account keeps one store at `$XDG_STATE_HOME/neverest/<account>/` (override with `store.root`): a SQLite index beside a content-addressed blob directory. Its presence is the single source of truth for "this account is initialized".

  Every collection is grouped under the account that syncs it (pimdir SPEC §9.2), so a store shared by two hand-written accounts says whose collection is whose. Every item carries a sort key (SPEC §9.3), so a frontend pages a collection in its natural order without reading a body.

- Added the `init` command, run once per account before the first sync.

  It opens every configured source so credential and network errors surface up front, then creates the empty store. `sync` refuses to run without it, and `init` refuses to run over it.

- Added **named endpoints and a declared mode**: an account holds a `sources` table, optionally a `targets` one, and the flags `one-way` and `retain`.

  A map key is the pimdir source id, so an endpoint's name is what every binding it owns is recorded under, and a positional list would reassign them all on a reorder.

  A backend written directly under the account (`imap.server = "…"`) is sugar for a source named after its protocol, which is the whole configuration for a single-provider account and the only shape the wizard writes.

  What an account does is its arity plus the two flags, so nothing is inferred from a coincidence between two sources.

  One source and one target sync both ways, several targets are one-way only, and several sources with no target is the offline replica, each merging with the store and isolated from the others. Every other combination is refused at load, naming the cell reached and the nearest legal one.

  An account may hold sources of several kinds: mail, contacts and calendar under one account and one store.

- Added `one-way`, which declares authority rather than leaving both sides to merge.

  The `sources` side wins: a difference resolves in its favour and the other side's change is discarded, so no conflict is recorded and no divergence reported. The other side is still enumerated every run, or every item would be re-pushed, and its state decides what the run has left to do rather than who wins.

- Added `retain`, which declares whether the store is a replica or only the ledger.

  The store is the ledger in every mode, holding the item spine and the checkpoints that make enumeration incremental, and a body-less copy still needs it. `retain` says whether it additionally holds bodies and is readable by a frontend. It defaults from the destination, true with no target and false with one.

  `retain = true` alongside targets is honoured rather than refused, migrating while keeping a local copy being a thing to want. It makes the store a backup rather than a cache, so `sync --reset` then destroys data.

- Added the **mode guard**: the account's mode is stamped beside the store and compared on every run.

  Turning `one-way` on over an account that synced both ways is refused, the run that follows being the one that discards what the previous mode was merging. `sync --accept-mode` records the answer so it does not come back. A `retain` dropping to false and a change in the number of endpoints are reported and do not block.

- Added **CardDAV** and **CalDAV** backend support via [io-webdav](https://github.com/pimalaya/io-webdav), behind the `dav` cargo feature.

  Address books and calendars are collections, keyed by their path segment rather than their display name, which is optional, mutable and free to collide.

  Enumeration is RFC 6578 `sync-collection`, the server's token riding as the engine's opaque checkpoint. A rejected token falls back to a full report, a truncated one is drained, and a server implementing no `sync-collection` at all is listed with a `PROPFIND` at Depth 1 instead.

  Bodies come from `addressbook-multiget` and `calendar-multiget`, and writes are `PUT` and `DELETE` conditional on the last-synced ETag, so a remote that moved is rejected rather than overwritten.

  One adapter serves both protocols: they differ in the home set they discover, the collection they list and the extension a new resource is named with, and in nothing else the sync sees.

  These are the first mutable-content backends, so they are the first to exercise revisions, conditional writes and conflicts, which mail leaves inert. A link id is the vCard or iCalendar `UID`, falling back to a digest of the body for a resource carrying none, and a collection holding one `UID` under two resources syncs as two items.

  A card's summary carries the `UID`, `FN` and every `EMAIL`; a calendar resource's carries its component, `SUMMARY`, `LOCATION` and start, sorted by that start resolved to UTC through the `VTIMEZONE` the resource itself carries. Both cross neverest as opaque bytes, so a property it does not understand cannot be lost.

  A calendar item is the object **resource**, not the component, so a recurring series and its overrides are one item (RFC 4791 §4.1). A DAV source pairs with another source of its own kind or with the store alone, and an `smtp` table on one is refused.

- Added **Microsoft Graph** backend support via `io-msgraph`.

  Delta-query enumeration (the `@odata.deltaLink` as the sync checkpoint, an expired link restarting a full round), bodies through the raw MIME endpoint, and flag and delete pushes. Appends and moves into Graph are pull-only.

  Authentication is a bearer access token, resolved through the standard secret-command idiom. Neverest runs no OAuth flow itself.

- Added the queued **`submit` intent** and its send channel.

  A frontend enqueues a submission through the store's action queue, naming the body blob and the envelope, and the queue row pins the body until the send. Every run performs the pending ones through the one source offering a channel: its own `smtp` table, else its native send.

  A sent intent is acknowledged, which releases its body. A transient failure leaves it pending for the next run, a permanent one parks it with its error, and a build with no send channel leaves the intents pending rather than parking them. Submission is at-least-once, so deduplication is the receiving provider's job.

  The `smtp` table mirrors the `imap` one field for field, submission being the other half of the same mail account: a bare authority (read as `smtps://`) or a full URL, the same `tls` block, and one `sasl` mechanism out of the same six. Omitting `sasl` is the unauthenticated relay, which stops after `EHLO` and sends no `AUTH` at all.

- Added `store.purge-after`, the retention sweep.

  The store retains an item instead of deleting it when its last binding vanishes: hidden from the sync and from listings, body kept. After each sync neverest purges every retained item older than this human delay (`"90d"`, `"12h"`, `"0"`), runs the store's garbage collector, and reports the items, objects and bytes it reclaimed.

  Unset means never purge, `"0"` reproduces a terminal delete, and `sync --no-purge` skips the sweep for one run. Combined with a read-only source it makes a backup a remote expunge cannot lose.

- Added the **three-way merge** a run resolves a content conflict with.

  Most divergence is not disagreement: one side changed a phone number and the other a note, and the base the last sync agreed on proves it by naming which side touched which field.

  Every run merges the base, local and remote bodies of each conflicted item, dispatched on the collection's kind (vcard-rs for contacts, ical-rs for calendars, tasks and journals), and clears the conflict as an ordinary edit whenever the merge reports no collision.

  Two endpoints of one account are merged with each other on the same terms, against the body they both came from, which no endpoint's own reconcile can see. Both sides setting one field differently is a genuine disagreement and parks for a person, whichever pair diverged.

  The merge is built in rather than configured: it is a pure function over bodies the store already holds, there is no taste in it, and the format vocabulary is closed. Mail is immutable-content and reaches none of it.

- Added the `conflict` command, which is the only place a content collision is decided.

  `conflict list` names every divergence the account's store is holding, `conflict show <id>` prints the three bodies a decision is made from, and `conflict resolve <id>` settles one. An item is addressed by the public id every other command shows, narrowed by `--source` when it diverged on more than one.

  `--prefer-local` and `--prefer-remote` discard a side, which is acceptable because a person asked for it by name and is exactly what a background run must never do on its own. Deciding is never reached from a sync, whatever is attached to its terminal.

  Neverest raises no desktop notification of its own: `conflicts` is what a run marked and `outstandingConflicts` what the store holds waiting, so a caller reading `--json` notifies on entry, once, with no state to keep, and can name the item. The README and config.sample.toml carry the recipe.

- Added `conflict.merger` and `conflict resolve --interactive`, which hand a collision to a program of your own.

  Following git mergetool, the three bodies are appended positionally as filesystem paths, base first, then the divergent sides, then the path to write, which is tcal's own argument order and makes `conflict.merger = "tcal merge"` the whole configuration.

  A command carrying any of `{base}`, `{local}`, `{remote}` and `{output}` is substituted instead, for a tool with an argument shape of its own.

  The result is taken only on a zero exit with the output written, compared by content rather than by timestamp, since an editor exits zero on a bare quit. It is then read as a body of that item, so one no parser reads and one stating another `UID` are both refused.

  A decision the store moved out from under is refused rather than pushed over what arrived meanwhile, since an unresolved conflict tracks the newest remote revision on every run.

  Under `--interactive` the fresh bodies are exported again and the merger asked once more. No lock is held across the merger, so a sync is never blocked behind a person sitting in an editor.

- Added exit code 2 for a run that reconciled its collections and left something waiting for a person.

  A parked conflict, a duplicate `UID` a side refuses and a write a remote would not take are one class: item-wide, unresolved, re-reported every run and unchanged by a rerun.

  Failing the run instead would stop the other ten thousand items over one duplicated phone number, and under a supervisor restarting on failure it would loop over a state no supervisor can fix.

  The outstanding count sits beside it in both output modes, read from the store rather than from the run's own tally, so it is the number of decisions waiting rather than the number this run discovered.

- Added a warnings section for what a run could not deliver, in the text report and under `refused` and `rejected` in `--json`.

  A create a side refused because it already holds the item's `UID` names the side, the collection and the `UID`. A write a remote rejected names its reason and takes back the hunk it was derived from, so a run that wrote nothing never reads as having written.

  Both are re-reported on every run until a person acts. Neverest repairs neither, which copy to keep being the user's call, in their own client.

- Added the per-account `sync.lock` advisory file lock, so two concurrent runs no longer corrupt the store.

  It lives in the actual store directory, honouring `store.root`, and a second run waits up to 60 seconds for the holder before exiting with a clear error.

- Added the store's action queue, neverest being its sole owner: every run drains it before it syncs.

  Each action a frontend enqueued is applied exactly once against the source that owns its collection, and the run reports per-collection applied, skipped and parked counts. The sync then pushes the resulting dirty state.

- Added the **handle-space rebuild**: an IMAP `UIDVALIDITY` change detected across a pull drives io-replica's rekey.

  Cached bodies, summaries and pending state are carried over by link id, and the collection's `generation` bumps atomically with the rebuild, so a store frontend derives its epoch from the store alone. Graph sources never bump, their message ids surviving a delta reset.

- Added `sync --source <name>`, narrowing a run to the named sources.

- Added the `<protocol>.item.update` permission, gating in-place body edits.

  It defaults to `true` and is optional, so an existing configuration parses unchanged. It only bites on a mutable-content backend.

- Added the `json-schema` command, aliased `json-schemas`, describing what each data command prints under `--json`.

  One schema per command path, printed to the standard output for a single command or written as one file per command with `--dir`. The sync payload is the substantial one: a consumer reading `conflicts` or `outstandingConflicts` out of it has the shape written down rather than inferred from a sample run.

### Changed

- **BREAKING**: the sync engine runs on the [io-replica](https://github.com/pimalaya/io-replica) replica engine instead of a hand-rolled three-way diff.

  An account's sources are the sources of one shared collection in the store, so cross-source propagation of items, flags and deletions falls out of the shared hub.

- **BREAKING**: `left` and `right` are gone, replaced by the `sources` and `targets` tables and the `one-way` flag.

  They are refused at load in any form, as keys, as aliases and as source ids, naming what declares the direction they never could.

- **BREAKING**: the sync vocabulary is kind-neutral, turning neverest from a mail sync into a generic PIM sync.

  Everything above the backend seam speaks collections and items rather than folders and messages, and each protocol adapter keeps its own nouns behind it. The per-account `mailbox` and `message` tables became `collection` and `item`, and so did the per-source permission tables, both old spellings staying as serde aliases for one release.

  The `--include-mailbox`, `--exclude-mailbox` and `--all-mailboxes` flags became `--include-collection`, `--exclude-collection` and `--all-collections`, keeping the old long names as aliases and the `-m`, `-x` and `-A` short flags. The `--json` report's `mailbox` and `email` patch sections are now `collection` and `item`, with no alias.

- **BREAKING**: every `--json` key is camelCase.

  It matches the wire formats the endpoints speak (JMAP per RFC 8620, Microsoft Graph, the Google APIs), and keeps every key reachable by dot access in jq and JavaScript, which neither `outstanding_conflicts` nor `message-id` was. A variant travelling as a value keeps its own spelling, and TOML keys stay kebab-case.

- **BREAKING**: `collection.filter` belongs to the source it filters rather than to the account.

  An account may hold sources of several kinds, and an `include = ["INBOX"]` means nothing to a contacts source. Filters are consequently asymmetric: a collection may be synced on one source and skipped on another. An account-level `collection` table is refused, naming its replacement.

- **BREAKING**: the SMTP submission channel belongs to the source it completes.

  It is written `sources.<name>.smtp.*`, or `smtp.*` under an account whose mail backend is the direct-backend sugar. At most one source per account may declare one, and two are refused at load rather than silently resolved by configuration order.

- **BREAKING**: per-source permissions are enforced per operation.

  They map onto io-replica's per-kind push rights one to one, and a forbidden kind is kept pending by the engine while the others still propagate. A tightened permission block takes effect now where it previously did not.

- **BREAKING**: renamed `doctor` back to `check`, and `completions` and `manuals` to `completion` and `manual`, the plurals staying as hidden aliases.

- **BREAKING**: `check` and `init` print one payload rather than a run of prose lines.

  Two or three separate messages meant two or three JSON documents on the standard output and nothing a parser could read. `check` reports the account's mode with one entry per endpoint it opened and how many collections it listed, and `init` the store it created.

- Every `server` accepts a bare authority, port included, as well as a full URL.

  An authority takes the backend's default scheme (`imaps`, `smtps`, `https`) and a value carrying one is used verbatim, so `posteo.de:8843` resolves rather than reaching a backend hostless.

- Every configured secret resolves once per run instead of once per opened connection.

  A `password.command` used to be spawned inside the connection layer, so an IMAP source at the default `-j 4` ran it four times concurrently before its first request, and an account naming one `pass` entry from four tables paid six `gpg` invocations per sync. A run now resolves them all up front, memoizing identical commands.

  A credential that fails to resolve fails its own endpoint rather than the account, so a stale entry for calendars no longer leaves mail unsynced. The wait has a spinner of its own and each command is logged at `debug` with the time it took, never with its value or its arguments.

- The configuration wizard asks for a single input, an email address, and derives the account name from its domain.

  Discovery runs every mechanism in parallel (fixed provider rules, PACC, Thunderbird Autoconfiguration, RFC 6186 SRV, RFC 6764 DAV) under a deadline rather than trying them in series, and every reachable service is proposed.

  Only backends compiled into the running build are offered, and only the SASL mechanisms both the server advertises and the configuration can express.

  A bare `neverest` prints the help, and offers the wizard only when it finds no configuration. A command that finds none offers it too, then fails naming the path it looked at when nothing landed. Scripts and JSON callers skip the offer and get that failure straight away.

- `neverest configure` generates an account and never edits one.

  It reads the configuration on disk for two things only, the account names it takes and whether one of them claims `default`, then appends the generated `[accounts.<name>]` table as plain text, so comments, ordering and hand-written formatting survive.

  The name is suffixed (`posteo-2`) until it is free, and the account claims `default` only when no other one does.

  `--json` or a redirected stdout prints the account and touches no file, so `neverest configure > config.toml` writes it itself, and declining the prompt prints it too. Changing an account already configured is a job for your editor, against [config.sample.toml](./config.sample.toml).

- The IMAP and SMTP `alpn` fields are optional rather than defaulted in place, so io-imap and io-smtp own their own default.

  The SMTP channel therefore offers the `smtp` ALPN token (RFC 7595) where it previously offered none. Set `smtp.alpn = []` to restore the old behaviour.

- Every remote is a cargo feature: `imap`, `msgraph`, `dav` (CardDAV and CalDAV together), plus `smtp` for the submission channel.

  All of them ship in the default set. Every source config parses in every build, and opening a source whose backend was not compiled in reports it at runtime.

- Relicensed from `AGPL-3.0-only` to `MIT OR Apache-2.0`, aligning with the rest of the Pimalaya ecosystem.

- Bumped the Pimalaya libraries: io-replica 0.5, io-pimdir 0.4, io-imap 0.6, io-smtp 0.3, io-webdav 0.3, io-http 0.5, io-pim-discovery 0.7, io-msgraph 0.3, ical-rs 0.5, vcard-rs 0.4, pimalaya-stream 0.3, pimalaya-cli 0.2 and pimalaya-config 0.2.

  SASL moved out of pimalaya-stream into the new io-sasl crate, so the SCRAM-SHA-256 the configuration has always offered is now runnable. The minimum supported Rust version is 1.89.

### Removed

- **BREAKING**: removed local file backends as sync sources (Maildir, then m2dir).

  A source is a remote, and the pimdir store is the local replica, so a local file store beside it would be a second local copy. An existing on-disk tree is brought in through io-pimdir's conversion tooling instead.

- **BREAKING**: removed the **Notmuch** backend, with no replacement in the `io-*` ecosystem yet.

- **BREAKING**: removed collection aliases (`collection.alias`, the v0.1 `folder.aliases`).

  The table parsed and nothing ever read it: substituting a friendly name for a backend id is display work, and neverest renders nothing. The store carries per-collection display metadata for the frontend that does. A configuration still declaring the table is refused rather than silently ignored.

- **BREAKING**: removed the built-in keyring and OAuth support.

  Secrets come from a command instead, so any secret manager works, and [ortie](https://github.com/pimalaya/ortie) issues and refreshes OAuth access tokens.

- **BREAKING**: removed `envelope.filter`, the per-source folder aliases, the `-o` output flag and the `--color` flag.

  Color follows the terminal, and `--json` replaces `-o json`.

## [1.0.0-beta] - 2024-04-15

### Added

- Added `--debug` as an alias for `RUST_LOG=debug`.
- Added `--trace` as an alias for `RUST_LOG=trace`.
- Added notes about `--debug` and `--trace` when error occurs.
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
[1.0.0]: https://github.com/pimalaya/neverest/compare/v1.0.0-beta...v1.0.0
[1.0.0-beta]: https://github.com/pimalaya/neverest/compare/v0.1.0...v1.0.0-beta
[0.1.0]: https://github.com/pimalaya/neverest/compare/root...v0.1.0
