# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).


## [Unreleased]

Neverest v1 is a full rewrite on top of the I/O-free `io-*` ecosystem. The CLI, the configuration schema and the sync engine all changed shape; [MIGRATION.md](./MIGRATION.md) carries the upgrade path from v0.1.0.

### Added

- Added the local **pimdir store**, the single local copy an app reads.

  Each account keeps one store at `$XDG_STATE_HOME/neverest/<account>/` (override with `store.root`): a SQLite index beside a content-addressed blob directory. Its presence is the single source of truth for "this account is initialized". Every collection is grouped under the account that syncs it (pimdir SPEC §9.2), so a store shared by two hand-written accounts says whose collection is whose.

- Added the `init` command, run once per account before the first sync.

  It opens every configured side so credential and network errors surface up front, then creates the empty store. `sync` refuses to run without it, and `init` refuses to run over it.

- Added **Microsoft Graph** backend support via `io-msgraph`.

  Delta-query enumeration (the `@odata.deltaLink` as the sync checkpoint, an expired link restarting a full round), bodies through the raw MIME endpoint, and flag and delete pushes. Appends and moves into Graph are pull-only. Authentication is a bearer access token, resolved through the standard secret-command idiom; neverest runs no OAuth flow itself.

- Added **CardDAV** backend support via [io-webdav](https://github.com/pimalaya/io-webdav), behind the `carddav` cargo feature.

  Address books are collections, keyed by their path segment rather than their display name, which is optional, mutable and free to collide. Enumeration is RFC 6578 `sync-collection`, the server's token riding as the engine's opaque checkpoint: a rejected token falls back to a full report and a truncated one is drained. Bodies come from `addressbook-multiget`, and writes are `PUT` and `DELETE` conditional on the last-synced ETag.

  This is the first non-mail kind and the first mutable-content backend. Cards are edited in place, so it is the first backend to exercise the revision plumbing, the conditional-write path and the conflict handling that mail leaves inert. A card's link id is its vCard `UID`, falling back to a digest of the body for a card carrying none, and its summary carries the `UID`, `FN` and every `EMAIL` so a reader renders a contact list without fetching bodies. Cards cross neverest as opaque bytes, so a property it does not understand cannot be lost. A contacts account pairs with another contacts side or with the store alone, and an `<side>.smtp` table on a DAV side is refused.

  The feature is out of the default set until its live suite runs in CI.

- Added the queued **`submit` intent** and its send channel.

  A frontend enqueues a submission through the store's action queue, naming the body blob and the envelope, and the queue row pins the body until the send. Every run performs the pending ones through the first side offering a channel: its own `<side>.smtp` table, else its native send. A sent intent is acknowledged, which releases its body; a transient failure leaves it pending for the next run and a permanent one parks it with its error. A build with no send channel leaves the intents pending rather than parking them. Submission is at-least-once, so deduplication is the receiving provider's job.

- Added `store.purge-after`, the retention sweep.

  The store retains an item instead of deleting it when its last binding vanishes: hidden from the sync and from listings, body kept. After each sync neverest purges every retained item older than this human delay (`"90d"`, `"12h"`, `"0"`), runs the store's garbage collector, and reports the items, objects and bytes it reclaimed. Unset means never purge, `"0"` reproduces a terminal delete, and `sync --no-purge` skips the sweep for one run. Combined with a read-only remote side it makes a backup a remote expunge cannot lose.

- Added `store.hydration = "full"`, mirroring every body into the store instead of only the crossing ones.

- Added the **relay** path: a body crossing two IMAP sides is streamed server-to-server, the store keeping only the spine.

  It is the default for a two-IMAP account. Any other pairing, or `store.retention = "retain"`, retains instead.

- Added the **handle-space rebuild**: an IMAP `UIDVALIDITY` change detected across a pull drives io-replica's rekey.

  Cached bodies, summaries and pending state are carried over by link id, and the collection's `generation` bumps atomically with the rebuild, so a store frontend derives its epoch from the store alone. Graph sides never bump, their message ids surviving a delta reset.

- Added a warnings section for an identity a collection holds twice, in the text report and under `ambiguous` in `--json`.

  It names the collection and every id involved, and is re-reported on every run until the collection holds the identity once. Neverest repairs nothing: which copy to keep is the user's call, with their own client. Detection, the derive-nothing rules and the persistence live in io-replica and io-pimdir.

- Added the `<side>.<backend>.item.update` permission, gating in-place body edits.

  It defaults to `true` and is optional, so an existing configuration parses unchanged. It only bites on a mutable-content backend.

- Added the per-account `sync.lock` advisory file lock, so two concurrent runs no longer corrupt the store.

  It lives in the actual store directory, honouring `store.root`, and a second run waits up to 60 seconds for the holder before exiting with a clear error.

- Neverest is now the pimdir store's sole owner: every run first drains the store's action queue.

  Each action a frontend enqueued is applied exactly once, and the run reports per-collection applied counts plus any permanently unappliable action, until repaired. The sync then pushes the resulting dirty state.

### Changed

- **BREAKING**: the sync engine runs on the [io-replica](https://github.com/pimalaya/io-replica) replica engine instead of a hand-rolled three-way diff.

  The two sides of an account are two sources of one shared collection in the store, so cross-side propagation of items, flags and deletions falls out of the shared hub rather than a hand-rolled cross-merge.

- **BREAKING**: the sync vocabulary is kind-neutral, turning neverest from a mail sync into a generic PIM sync.

  Everything above the backend seam speaks collections and items rather than mailboxes and messages; each protocol adapter keeps its own nouns behind the seam. The per-account `mailbox` and `message` tables became `collection` and `item`, and so did the per-side permission tables. Both old spellings keep working as serde aliases for one release. The `--include-mailbox`, `--exclude-mailbox` and `--all-mailboxes` flags became `--include-collection`, `--exclude-collection` and `--all-collections`, keeping the old long names as aliases and the `-m`, `-x` and `-A` short flags. The `--json` report's `mailbox` and `email` patch sections are now `collection` and `item`, with no alias.

- **BREAKING**: the reserved `Outbox` collection is gone, a queued submission being a `submit` action now.

  Neverest no longer creates an `Outbox` collection, no longer matches that name case-insensitively, and no longer hides it from listings, so a remote folder called `Outbox` syncs like any other. Anything that wrote a message into a local `Outbox` to have it sent must enqueue a `submit` action instead. The `--json` report renames the `outbox` section to `submitted` and the text report renames it to `Submissions`.

- **BREAKING**: the SMTP submission channel moved from the account root into the side it completes, `<side>.smtp.*` instead of `smtp.*`.

  The channel is a property of one provider, not of the account, and whether a side needs one at all depends on its backend. Queued intents are performed through the first side offering a channel, its own `smtp` table before its native send. A configuration keeping `smtp` at the root now fails to parse.

- **BREAKING**: per-side permissions are enforced per operation.

  They map onto io-replica's per-kind push rights one to one, and a forbidden kind is kept pending by the engine while the others still propagate. If you relied on a tightened permission block, it takes effect now where it previously did not.

- **BREAKING**: bodies are named by the hash the store records (pimdir SPEC §5) rather than by neverest's own digest.

  Every consumer of one store now names the same body identically, which is what makes the object store dedup at all. An existing store keeps its old object names and is not rewritten; `sync --reset` re-lands its bodies under the recorded algorithm.

- **BREAKING**: renamed `completions` and `manuals` to `completion` and `manual`, the plural staying as a hidden alias.

- The sync seam carries content revisions end to end and implements conditional in-place updates.

  An edit is written `If-Match` the last-synced revision, and a remote that moved since is rejected rather than overwritten, so the engine re-merges and records a conflict instead of destroying someone's edit. Conflicts are reported in both output modes and re-reported every run until resolved; neverest never resolves one by itself.

- Every summarised item carries a sort key (pimdir SPEC §9.3), so a reader pages a collection in its natural order.

  The `Date:` header in RFC 3339 UTC at seconds precision for mail, derived identically at both tiers so hydrating a message never re-sorts it, and the casefolded display name for a card. Content with nothing to derive from keeps the empty key, which the store reads as unknown.

- The driver no longer knows what a UIDVALIDITY is: the rebuild guard reads an opaque epoch through the backend, and the IMAP checkpoint codec moved into the IMAP adapter.

- A store an earlier draft of the pimdir format wrote is refused naming the command that fixes it, the store being a derived cache rather than something to migrate.

- The configuration wizard asks for a single input, an email address, and derives the account name from its domain.

  Discovery runs every mechanism in parallel (fixed provider rules, PACC, Thunderbird Autoconfiguration, RFC 6186 SRV, RFC 6764 DAV) under a deadline rather than trying them in series, and every reachable service is proposed. Only backends compiled into the running build are offered, and only the SASL mechanisms both the server advertises and the configuration can express. Every configured connection is tested before the file is written. A bare `neverest` runs the wizard, offering the generated configuration for saving, never overwriting without confirmation, and printing it on stdout when you decline; `--json` or a redirected stdout skips the prompts, so `neverest > config.toml` writes the file itself. `neverest configure` runs the same flow over an existing account, preserving everything the wizard did not decide.

- The IMAP and SMTP `alpn` fields are optional rather than defaulted in place, so io-imap and io-smtp own their own default.

  The SMTP channel therefore offers the `smtp` ALPN token (RFC 7595) where it previously offered none; set `<side>.smtp.alpn = []` to restore the old behaviour.

- Every remote is a cargo feature: `imap`, `msgraph`, `carddav`, plus `smtp` for the submission channel.

  All but `carddav` ship in the default set. Every side config parses in every build, and opening a side whose backend was not compiled in reports it at runtime.

- Relicensed from `AGPL-3.0-only` to `MIT OR Apache-2.0`, aligning with the rest of the Pimalaya ecosystem.

- Bumped the Pimalaya libraries: io-replica 0.4, io-pimdir 0.3, io-imap 0.6, io-smtp 0.3, io-webdav 0.2, io-http 0.5, io-pim-discovery 0.7, io-msgraph 0.3, pimalaya-stream 0.3, pimalaya-cli 0.2 and pimalaya-config 0.1. SASL moved out of pimalaya-stream into the new io-sasl crate, so the SCRAM-SHA-256 the configuration has always offered is now runnable. The minimum supported Rust version is 1.89.

### Fixed

- Queued submissions no longer fail against a server that checks the greeting.

  The SMTP session greeted with `EHLO localhost`, which RFC 5321 §4.1.4 entitles a server to reject, and one that does answers `550 5.5.0 Invalid EHLO domain`, so the session died before `MAIL FROM`. The failure being transient, the intent stayed pending behind a warning: the queue filled and no mail ever left. The greeting is now the loopback address literal RFC 5321 §4.1.3 reserves for a client with no resolvable name.

- A side that may not delete no longer resurrects the item on every side.

  A refused delete was undone rather than held, which states that the side still holds the member; the shared store reads that as the item being alive and clears the deletion everywhere, so removing an item once brought it back. The tombstone now stays until a run that may push delivers it.

- A store's disk is now reclaimed.

  The store deliberately collects nothing by itself, leaving reclamation to its owner, which is neverest. Nothing ran it, so every dereferenced body stayed on disk for ever while `store.purge-after` reported bytes it had merely released. The sweep now runs the collector after a purge that took something, and a store that has been running for a while reclaims its backlog on the next sweep. Orphan blobs left by a crash are still `pimdir gc`'s to take.

- An identity a collection holds twice no longer costs the other side its copy.

  Two messages with the same `Message-ID` used to pair arbitrarily: deleting the paired copy propagated a delete that removed the only copy on the other side, and a later checkpoint loss revived the retained row and re-appended it while reporting `already in sync`. The engine now freezes such an identity and derives nothing for it in either direction.

- A relayed copy is now reported.

  A relayed body never reaches the projection the report is built from, so every relayed append was invisible and a run that appended messages could print `already in sync`. Each relay is itemized where it happens.

- A handle-space rebuild no longer freezes the collection.

  A `UIDVALIDITY` bump renumbers every message, and the store read the rebuild as one server reporting each identity under a second handle: it kept the voided handles, marked every item ambiguous and stopped syncing in both directions. Fixed in io-pimdir.

- A CardDAV side no longer dies after its first request.

  A server that closes the connection between requests left every later exchange writing into a socket the peer had hung up on, so discovery itself failed. The connection is reopened and the exchange run again, carrying the discovered principal and home-set URLs over. Only an end-of-stream or reset failure is retried, so a create or a delete is never replayed against a server that acted on it.

- A DAV collection no longer fails its scan.

  Freshly probed items were raised to the `Meta` tier whatever the kind, which asks a CardDAV side for a summary tier it has none of. The tier is now the kind's.

- A body is hydrated by the absence of a stored body rather than by the item's detail level, so an item whose stale body was dropped is picked up again.

### Removed

- **BREAKING**: removed local file backends as sync sides (Maildir, then m2dir).

  A side is a remote, and the pimdir store is the local replica, so a local file store beside it would be a second local copy. An existing on-disk tree is brought in through io-pimdir's conversion tooling instead.

- **BREAKING**: removed the **Notmuch** backend, with no replacement in the `io-*` ecosystem yet.

- **BREAKING**: removed collection aliases (`collection.alias`, the v0.1 `folder.aliases`).

  The table parsed and nothing ever read it: substituting a friendly name for a backend id is display work, and neverest renders nothing. The store already carries per-collection display metadata for the frontend that does. A configuration still declaring the table is refused rather than silently ignored.

- **BREAKING**: removed the built-in keyring and OAuth support.

  Secrets come from a command instead, so any secret manager works, and [ortie](https://github.com/pimalaya/ortie) issues and refreshes OAuth access tokens.

- **BREAKING**: removed `envelope.filter`, the per-side folder aliases, the `-o` output flag and the `--color` flag. Color now follows the terminal, and `--json` replaces `-o json`.

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

[Unreleased]: https://github.com/pimalaya/neverest/compare/v1.0.0-beta...HEAD
[1.0.0-beta]: https://github.com/pimalaya/neverest/compare/v0.1.0...v1.0.0-beta
[0.1.0]: https://github.com/pimalaya/neverest/compare/root...v0.1.0
