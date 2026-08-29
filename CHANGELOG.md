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

  It opens every configured source so credential and network errors surface up front, then creates the empty store. `sync` refuses to run without it, and `init` refuses to run over it.

- Added **Microsoft Graph** backend support via `io-msgraph`.

  Delta-query enumeration (the `@odata.deltaLink` as the sync checkpoint, an expired link restarting a full round), bodies through the raw MIME endpoint, and flag and delete pushes. Appends and moves into Graph are pull-only. Authentication is a bearer access token, resolved through the standard secret-command idiom; neverest runs no OAuth flow itself.

- Added **CardDAV** and **CalDAV** backend support via [io-webdav](https://github.com/pimalaya/io-webdav), behind the `dav` cargo feature.

  Address books and calendars are collections, keyed by their path segment rather than their display name, which is optional, mutable and free to collide. Enumeration is RFC 6578 `sync-collection`, the server's token riding as the engine's opaque checkpoint: a rejected token falls back to a full report and a truncated one is drained, and a server implementing no `sync-collection` at all is listed with a `PROPFIND` instead. Bodies come from `addressbook-multiget` / `calendar-multiget`, and writes are `PUT` and `DELETE` conditional on the last-synced ETag. One adapter serves both protocols: they differ in the home set they discover, the collection they list and the extension a new resource is named with, and in nothing else the sync sees.

  These are the non-mail kinds and the first mutable-content backends. A card or an event is edited in place, so they are the first to exercise the revision plumbing, the conditional-write path and the conflict handling that mail leaves inert. Their link id is the vCard or iCalendar `UID`, falling back to a digest of the body for a resource carrying none. A collection holding one `UID` under two resources syncs as two items, and a new resource is named after the item's key rather than its `UID` alone, so the second copy is pushed beside its twin rather than over it. A card's summary carries the `UID`, `FN` and every `EMAIL`; a calendar resource's carries its component, `SUMMARY`, `LOCATION` and start, and it sorts by that start resolved to UTC through the `VTIMEZONE` the resource itself carries. Both cross neverest as opaque bytes, so a property it does not understand cannot be lost. A calendar item is the object **resource**, not the component, so a recurring series and its overrides are one item (RFC 4791 §4.1). A DAV source pairs with another source of its own kind or with the store alone, and an `smtp` table on one is refused.

- Added the queued **`submit` intent** and its send channel.

  A frontend enqueues a submission through the store's action queue, naming the body blob and the envelope, and the queue row pins the body until the send. Every run performs the pending ones through the one source offering a channel: its own `smtp` table, else its native send. A sent intent is acknowledged, which releases its body; a transient failure leaves it pending for the next run and a permanent one parks it with its error. A build with no send channel leaves the intents pending rather than parking them. Submission is at-least-once, so deduplication is the receiving provider's job.

  The `smtp` table mirrors the `imap` one field for field, submission being the other half of the same mail account: a bare authority (read as `smtps://`) or a full URL, the same `tls` block, and one `sasl` mechanism out of the same six. An account authenticating IMAP with an OAuth token therefore authenticates submission the same way, where a channel restricted to a login and a password could only refuse it, and the wizard offers to reuse the IMAP table whatever it names. Omitting `sasl` is the unauthenticated relay, which stops after `EHLO` and sends no `AUTH` at all.

- Added `store.purge-after`, the retention sweep.

  The store retains an item instead of deleting it when its last binding vanishes: hidden from the sync and from listings, body kept. After each sync neverest purges every retained item older than this human delay (`"90d"`, `"12h"`, `"0"`), runs the store's garbage collector, and reports the items, objects and bytes it reclaimed. Unset means never purge, `"0"` reproduces a terminal delete, and `sync --no-purge` skips the sweep for one run. Combined with a read-only source it makes a backup a remote expunge cannot lose.

- Added **named endpoints and a declared mode**: an account holds a `sources` table, optionally a `targets` one, and the flags `one-way` and `retain`, rather than a `left` and a `right`.

  A map key is the pimdir source id, so an endpoint's name is what every binding it owns is recorded under; a positional list would reassign them all on a reorder. A backend written directly under the account (`imap.server = "…"`) is sugar for a source named after its protocol, which is the whole configuration for a single-provider account and the only shape the wizard writes. The sugar and its expansion produce the same source id, so expanding one by hand changes nothing on disk. An account may hold sources of several kinds: mail, contacts and calendar under one account and one store.

  What the account does is its arity plus the two flags, so there is no mode to name and nothing is inferred from a coincidence between two sources. One source and one target sync both ways; adding `one-way` makes the source overwrite the target; one source and several targets is one-way only, bidirectional propagation across more than two endpoints having no resolution order. Several sources and no target is the offline replica, each merging with the local store and isolated from the others, and `one-way` there makes the sources overwrite it. Every other combination is refused at load, naming the cell reached and the nearest legal one.

- Added `one-way`, which declares authority rather than leaving both sides to merge.

  The `sources` side wins: a difference resolves in its favour and the other side's change is discarded, so no conflict is recorded and no divergence reported. It does not mean the other side goes unread, since it is still enumerated every run or every item would be re-pushed; its state decides what the run has left to do and never who wins. Changes are overwritten, not merged, which is the rsync reading and not the one a two-way account gives.

- Added `retain`, which declares whether the store is a replica or only the ledger.

  The store is the ledger in every mode, holding the item spine and the per-collection checkpoints that make enumeration incremental, and it is required even where no body is kept: a body-less IMAP to IMAP copy still needs to know what it has already copied. `retain` says whether it additionally holds bodies and is readable by a frontend. It defaults from the destination, true with no target and false with one, and `retain = true` alongside targets is honoured rather than refused: migrating while keeping a local copy is a thing to want. Note that it makes the store a backup rather than a cache, so `sync --reset` then destroys data.

  Whether a crossing is streamed straight from one remote to the other or staged in the store and released is an internal choice, taken where the pairing allows it, and is invisible in the configuration and in every report. A store holding only the bodies that happened to cross is not a state anyone can be in.

- Added the **mode guard**: the account's mode is stamped beside the store and compared on every run.

  Turning `one-way` on over an account that synced both ways is refused, the run that follows being the one that discards what the previous mode was merging; `sync --accept-mode` records the answer so it does not come back. A `retain` that drops from true to false, and a change in the number of endpoints, are reported and do not block. The comparison is on those transitions and not on configuration change in general, so a rotated credential or a new filter never forces a resync. A first run under `one-way` has no recorded mode to compare against and is not gated; `init` states what the account will do in words instead.

- Added `sync --source <name>`, narrowing a run to the named sources.

- Added the **handle-space rebuild**: an IMAP `UIDVALIDITY` change detected across a pull drives io-replica's rekey.

  Cached bodies, summaries and pending state are carried over by link id, and the collection's `generation` bumps atomically with the rebuild, so a store frontend derives its epoch from the store alone. Graph sources never bump, their message ids surviving a delta reset.

- Added a warnings section for a copy a side refused because it already holds the item's `UID`, in the text report and under `refused` in `--json`.

  It names the side, the collection and the `UID`, and is re-reported on every run until that side stops holding the identity twice: the run wrote nothing for that item, and the line carries the one action that resolves it. Neverest repairs nothing, which copy to keep being the user's call, with their own client. A collection holding one identity under two resources is mirrored as two items and reported nowhere: the store holds what the source holds.

- Added the `<protocol>.item.update` permission, gating in-place body edits.

  It defaults to `true` and is optional, so an existing configuration parses unchanged. It only bites on a mutable-content backend.

- Added the per-account `sync.lock` advisory file lock, so two concurrent runs no longer corrupt the store.

  It lives in the actual store directory, honouring `store.root`, and a second run waits up to 60 seconds for the holder before exiting with a clear error.

- Neverest is now the pimdir store's sole owner: every run first drains the store's action queue.

  Each action a frontend enqueued is applied exactly once, and the run reports per-collection applied counts plus any permanently unappliable action, until repaired. The sync then pushes the resulting dirty state.

- Added the **three-way merge** a run resolves a content conflict with, behind the `merge` cargo feature (on by default, and it implies `dav`).

  Most divergence is not disagreement: one side changed a phone number and the other a note, and the base the last sync agreed on proves it by naming which side touched which field. Every run therefore merges the base, local and remote bodies of each conflicted item, dispatched on the collection's kind (vcard-rs for contacts, ical-rs for calendars, tasks and journals), and clears the conflict as an ordinary edit staged through the store's queue whenever the merge reports no collision. Mail is immutable-content and reaches none of this. The remote body comes from the store, the engine's upgrade pass having fetched it into the conflict object, so a conflict whose body has not landed yet is visible and left alone rather than merged against a body nobody holds.

  The merge is built in rather than configured: it is a pure function over bodies the store already holds, there is no taste in it, and the format vocabulary is closed. Because it cannot be swapped it is strictly conservative, resolving on an empty report and on nothing else. Both sides setting the same field differently is a genuine disagreement and still parks for a person.

- Added exit code 2 for a run that reconciled its collections and left conflicts behind, and the outstanding count beside it in both output modes.

  A conflict is one item wide and halts nothing: failing the run would stop the other ten thousand items over one duplicated phone number, and under a supervisor restarting on failure it would loop over a state no supervisor can fix. The count is read from the store rather than from the run's own tally, the engine emitting nothing for a placement it already parked, so it is the number of decisions waiting rather than the number this run discovered.

- Added `conflict.notify`, a desktop notification shown when an item enters conflict, behind the `notify` cargo feature (on by default).

  Opt-in and unset by default, which leaves the warning in the log and the entry in the report: a run under cron never shells out unasked. A conflict an earlier run already parked is announced by no later one, so a five-minute schedule over one unresolved card raises one notification rather than nearly three hundred a day.

- Added the `conflict` command, which is the only place a content collision is decided.

  `conflict list` names every divergence the account's store is holding, `conflict show <id>` prints the three bodies a decision is made from, and `conflict resolve <id>` settles one. An item is addressed by the public id every other command already shows, narrowed by `--source` when it diverged on more than one. A conflict whose diverging remote body no run has fetched yet is listed and is not resolvable until one has.

  `--prefer-local` and `--prefer-remote` discard a side, which is acceptable because a person asked for it by name and is exactly what a background run must never do on its own. Deciding is never reached from a sync, whatever is attached to its terminal: a run has one when a wrapper script drives it, when a pane nobody is sitting at watches it and when a person is waiting, and the three cannot be told apart from inside.

- Added `conflict.merger` and `conflict resolve --interactive`, which hand a collision to a program of your own.

  Following git mergetool, the three bodies are handed over as filesystem paths appended positionally, base first, then the divergent sides, then the path to write, which is tcal's own argument order and makes `conflict.merger = "tcal merge"` the whole configuration. A command carrying any of the placeholders `{base}`, `{local}`, `{remote}` and `{output}` is substituted instead, for a tool with an argument shape of its own: tcard takes its output as a flag. The result is taken only on a zero exit with the output written, compared by content rather than by timestamp, since an editor exits zero on a bare quit and reading that as a choice would discard a side by accident.

- Added the staleness guard, which refuses a decision the store moved out from under.

  `conflict resolve` records the revision the divergence was recorded at and re-reads the store before staging anything. An unresolved conflict tracks the newest remote revision on every run, so a decision left in an editor for an hour can be a decision about a version nobody holds any more, and pushing it would overwrite everything that arrived meanwhile. A revision that moved is reported as moved; under `--interactive` the fresh bodies are exported again and the merger asked once more. The store lock is deliberately not held across the merger, so a sync is never blocked behind a person sitting in an editor.

- Added the `json-schema` command, aliased `json-schemas`, describing what each data command prints under `--json`.

  One schema per command path (`neverest-sync`, `neverest-check`, `neverest-init`, `neverest-conflict-list`, `neverest-conflict-show`, `neverest-conflict-resolve`), printed to the standard output for a single command or written as one file per command with `--dir`. The sync payload is the substantial one: a consumer reading `conflicts` or `outstanding_conflicts` out of it now has the shape written down rather than inferred from a sample run.

### Changed

- `check` and `init` now print one payload instead of a run of prose lines.

  Both said their piece in two or three separate messages, which under `--json` meant two or three JSON documents on the standard output and nothing a parser could read. `check` now reports the account's mode with one entry per endpoint it opened and how many collections it listed, and `init` the store it created with the endpoints it opened; the text output is unchanged in substance.

- A run that could not deliver a write now exits 2 rather than 0, and so does a run holding a duplicate `UID` a side refuses.

  Exit 2 means the run reconciled its collections and left something waiting for a person, which until now was only a parked conflict. A write the other side would not take is the same class of outcome: item-wide, unresolved, re-reported every run and unchanged by a rerun. A wrapper distinguishing "everything delivered" from "something is waiting" keeps working; one reading 2 as "conflicts, specifically" now also sees it for a refusal, and the report says which.

- Resolved every configured secret once per run instead of once per opened connection.

  A `password.command` is a command until something runs it, and that used to happen inside the connection layer: opening a connection took the configuration and spawned the command, so an IMAP source at the default `-j 4` ran it four times, concurrently, before its first request. An account naming one `pass` entry from its `imap`, `smtp`, `carddav` and `caldav` tables paid six `gpg` invocations per sync, each one a key unlock. A run now resolves them all up front into a runtime account holding the values themselves, memoizing identical commands, so that same account resolves once. Opening a second connection to a side costs a handshake and nothing else.

  A credential that fails to resolve fails its own endpoint rather than the account, so a stale entry for calendars no longer leaves mail unsynced. The wait is also visible now: the resolution has a spinner of its own, and each spawned command is logged at `debug` with the time it took. Neither the value nor the command arguments are ever logged.

- **BREAKING**: the sync engine runs on the [io-replica](https://github.com/pimalaya/io-replica) replica engine instead of a hand-rolled three-way diff.

  An account's sources are the sources of one shared collection in the store, so cross-source propagation of items, flags and deletions falls out of the shared hub rather than a hand-rolled cross-merge.

- **BREAKING**: the sync vocabulary is kind-neutral, turning neverest from a mail sync into a generic PIM sync.

  Everything above the backend seam speaks collections and items rather than mailboxes and messages; each protocol adapter keeps its own nouns behind the seam. The per-account `mailbox` and `message` tables became `collection` and `item`, and so did the per-source permission tables. Both old spellings keep working as serde aliases for one release. The `--include-mailbox`, `--exclude-mailbox` and `--all-mailboxes` flags became `--include-collection`, `--exclude-collection` and `--all-collections`, keeping the old long names as aliases and the `-m`, `-x` and `-A` short flags. The `--json` report's `mailbox` and `email` patch sections are now `collection` and `item`, with no alias.

- **BREAKING**: the reserved `Outbox` collection is gone, a queued submission being a `submit` action now.

  Neverest no longer creates an `Outbox` collection, no longer matches that name case-insensitively, and no longer hides it from listings, so a remote folder called `Outbox` syncs like any other. Anything that wrote a message into a local `Outbox` to have it sent must enqueue a `submit` action instead. The `--json` report renames the `outbox` section to `submitted` and the text report renames it to `Submissions`.

- **BREAKING**: the SMTP submission channel belongs to the source it completes.

  The channel is a property of one provider, not of the account, and whether a source needs one at all depends on its backend. It is written `sources.<name>.smtp.*`, or `smtp.*` under an account whose mail backend is the direct-backend sugar. At most one source per account may declare one, and two are refused at load rather than silently resolved by configuration order.

- **BREAKING**: `collection.filter` moved from the account onto the source it filters.

  An account may hold sources of several kinds, and an `include = ["INBOX"]` means nothing to a contacts source. Filters are consequently asymmetric: a collection may be synced on one source and skipped on another. An account-level `collection` table is refused, naming its replacement.

- **BREAKING**: `store.retention` and `store.hydration` are removed; whether the store keeps bodies is the account's `retain`.

  They encoded three states in two settings, one combination of which meant nothing, and a three-point scale described a store holding only the bodies that happened to cross, which is neither a replica nor a relay and which nothing asked for. A configuration carrying either key is refused rather than mapped onto `retain` by guesswork. Dropping `retain` from true to false never removes what is already stored: bodies stay, unreferenced, until an explicit `pimdir gc` or `sync --reset`.

- **BREAKING**: `collection.namespace` is removed, and with it the per-run store report.

  The namespace decided which sources met, by coincidence rather than by declaration, and could not say which way anything flowed; both jobs are now the account's arity and `one-way`. It is refused by name on whichever endpoint carries it. The hub still keys collections by `(kind, namespace, name)` so an address book and a mailbox called `Default` stay apart, but the namespace is internal and defaulted to the source name.

  The store report goes with it. It existed because what the store kept was derived and therefore unreadable from the configuration; `retain` is written down, so a run that wrote nothing now says nothing. `check` states the account's mode in plain language instead, and the persisted value serves the mode guard rather than a per-run line.

- **BREAKING**: per-source permissions are enforced per operation.

  They map onto io-replica's per-kind push rights one to one, and a forbidden kind is kept pending by the engine while the others still propagate. If you relied on a tightened permission block, it takes effect now where it previously did not.

- **BREAKING**: bodies are named by the hash the store records (pimdir SPEC §5) rather than by neverest's own digest.

  Every consumer of one store now names the same body identically, which is what makes the object store dedup at all. An existing store keeps its old object names and is not rewritten; `sync --reset` re-lands its bodies under the recorded algorithm.

- **BREAKING**: renamed `completions` and `manuals` to `completion` and `manual`, the plural staying as a hidden alias.

- The sync seam carries content revisions end to end and implements conditional in-place updates.

  An edit is written `If-Match` the last-synced revision, and a remote that moved since is rejected rather than overwritten, so the engine re-merges and records a conflict instead of destroying someone's edit. Conflicts are reported in both output modes and re-reported every run until resolved. What the run's own three-way merge cannot settle, neverest never decides by itself.

- Link ids, dates and summaries are the pimdir format's, not neverest's.

  A message links by its bare `Message-ID` and a card by its bare `UID`, as pimdir SPEC Annex A and the format's own vectors give them; the `mid:` and `uid:` prefixes are gone, and only a kind's own fallback stays marked (`alt:`, `hash:`), that being the one case a prefix is for. `meta.date` is the UTC instant rather than the sender's offset, so two writers of one store record a message the same way. The summary types come from `io_pimdir::conventions`, so the schema cannot drift by a field, which also gives a card the `emails` list it was missing. **An existing store re-links on the next sync: run `neverest sync --reset -a <account>`.**

  The per-kind readers stay neverest's for now. io-pimdir's read headers raw, so an RFC 2047 subject reaches a reader as `=?utf-8?q?…?=`, and its vCard scanner cuts the value of a legal quoted parameter holding a colon and leaves RFC 6350 escaping in place. Each gap is held by a test here until io-pimdir closes it.

- Every summarised item carries a sort key (pimdir SPEC §9.3), so a reader pages a collection in its natural order.

  The `Date:` header in RFC 3339 UTC at seconds precision for mail, derived identically at both tiers so hydrating a message never re-sorts it, and the casefolded display name for a card. Content with nothing to derive from keeps the empty key, which the store reads as unknown.

- The driver no longer knows what a UIDVALIDITY is: the rebuild guard reads an opaque epoch through the backend, and the IMAP checkpoint codec moved into the IMAP adapter.

- A store an earlier draft of the pimdir format wrote is refused naming the command that fixes it, the store being a derived cache rather than something to migrate.

  The refusal reads a store directory holding a database with no `neverest.json` beside it, so whatever creates the database writes that file in the same act: `init` stamps the store it creates and `sync --reset` stamps the store it recreates. Without the stamp a fresh account was refused on its first sync, and refused again after the reset the refusal asked for.

- The configuration wizard asks for a single input, an email address, and derives the account name from its domain.

  Discovery runs every mechanism in parallel (fixed provider rules, PACC, Thunderbird Autoconfiguration, RFC 6186 SRV, RFC 6764 DAV) under a deadline rather than trying them in series, and every reachable service is proposed. Only backends compiled into the running build are offered, and only the SASL mechanisms both the server advertises and the configuration can express. Every configured connection is tested before the file is written. A bare `neverest` prints the help, and offers the wizard only when it finds no configuration, the way a bare `himalaya` does; the generated configuration is offered for saving, never overwritten without confirmation, and printed on stdout when you decline; `--json` or a redirected stdout skips the prompts, so `neverest > config.toml` writes the file itself. A command that finds no configuration offers the wizard too, then fails naming the path it looked at when nothing landed; scripts and JSON callers skip the offer and get that failure straight away. `neverest configure` runs the same flow over an existing account, preserving everything the wizard did not decide.

- The IMAP and SMTP `alpn` fields are optional rather than defaulted in place, so io-imap and io-smtp own their own default.

  The SMTP channel therefore offers the `smtp` ALPN token (RFC 7595) where it previously offered none; set `smtp.alpn = []` to restore the old behaviour.

- Every remote is a cargo feature: `imap`, `msgraph`, `dav` (CardDAV and CalDAV together), plus `smtp` for the submission channel.

  All of them ship in the default set. Every source config parses in every build, and opening a source whose backend was not compiled in reports it at runtime.

- Relicensed from `AGPL-3.0-only` to `MIT OR Apache-2.0`, aligning with the rest of the Pimalaya ecosystem.

- Bumped the Pimalaya libraries: io-replica 0.4, io-pimdir 0.3, io-imap 0.6, io-smtp 0.3, io-webdav 0.2, io-http 0.5, io-pim-discovery 0.7, io-msgraph 0.3, pimalaya-stream 0.3, pimalaya-cli 0.2 and pimalaya-config 0.1. SASL moved out of pimalaya-stream into the new io-sasl crate, so the SCRAM-SHA-256 the configuration has always offered is now runnable. The minimum supported Rust version is 1.89.

### Fixed

- `conflict resolve --interactive` stored whatever the merger wrote.

  A decision was read as "the output file is no longer the empty one it was seeded with", and nothing looked at those bytes afterwards: they went into the blob tree and onto the queue, the kind being asked only for the summary derived on the way past. A merger writing `this is not a card at all` and exiting zero therefore replaced a contact with something that is not one, keeping the item's link id and losing every field its identity came from. A tool that crashes after a partial write, a template saved half-finished and a program that writes its error message to the output path all produce it. The body is now read before it is staged and refused unless it opens and closes with the kind's delimiters and states the `UID` the item is bound by, which is what makes it a resolution *of that item*; either refusal leaves the divergence exactly as an aborted merger leaves it.

- An account syncing a source to a target never reported a conflict at all.

  Conflicts reached the report from one place, the path a source takes against the local store alone. The two-endpoint reconcile ran each side's sync and asked its reports only whether anything had moved, throwing the per-item events away, so an account mirroring two servers showed no conflict in the text report, none in `--json`, and could raise no notification, the announcement being made from a list nothing ever filled. It did still say how many decisions were waiting, that count being read from the store, and never which. Both topologies now report a parked divergence the same way, and a collection reconciled over several passes names one divergence once, as it does a refused duplicate and a refused write.

- A run never named the conflict it had just parked, so the conflict notification could not fire.

  Collections are reconciled across a worker pool and their reports merged at a barrier, and the merge carried the item patch alone: what a collection parked, what a side refused and what it skipped were dropped before the account's report was printed. The run showed only the store-wide count, how many items are waiting and never which, and the desktop notification, which is raised from the parked list, returned early on a list that was always empty. Every arm of a collection's report now travels.

- A write the server refused was counted as an applied hunk and the run exited 0.

  The item patch is the plan derived before anything is pushed, and nothing revisited it afterwards, so a `PUT` the server answered with `403` was reported as `update item …`, counted in the hunk total, and exited successfully, then reported identically on every run after it. At the default log level there was no warning either, so the only signal a cron job had said the run had succeeded over a change that never left the store. A refused write is now reported as one, with its reason, and the hunk it was derived from is taken back.

- No sync could run while a person was in the interactive merger, which made the staleness guard unreachable.

  `conflict resolve` took neverest's run lock only around the apply, so that a sync stayed free while a merger was open, and then held io-pimdir's store *owner* lock for the whole command anyway. A sync of that store was refused outright, and since a sync is the only thing that moves a placement's conflict revision, the guard that refuses a decision computed against a revision the store has moved past could never fire: the refusal, the retry and the re-export were dead code. Every conflict command now reads through a handle that owns nothing and takes no lock, and the resolution re-reads the divergence per attempt, releasing the store before the merger runs.

- Every source drained every collection, so the first one alphabetically destroyed what a frontend queued for the others.

  The pre-sync drain listed the store's queued collections and drained all of them through the running source's handle, and that listing is store-wide: the queue records no source. Staging an existing item's action resolves that item's binding for the *draining* source, so a contacts source reaching a mail collection could not place the action, and io-pimdir parked it. A parked row is terminal, skipped by every later drain and cleared by no verb, so the action was destroyed before the source that held the item ever looked. Sources run in name order, which made this a rule rather than a race: on an account declaring `caldav`, `carddav` and `imap`, every flag change himalaya queued against `imap/INBOX` was parked by `caldav`, with `seq … projects no placement` as its epitaph. A source now drains only the collections its own namespace owns, and the drain reports what it skipped beside what it applied and parked.

  io-pimdir is fixed in the same breath, so that a missing binding leaves the row pending for the source that can apply it rather than parking it. Rows already parked stay parked: `pimdir queue cancel <id>` drops one, and the action it carried has to be redone in the frontend.

- A parked queue action was reported once per source instead of once per run.

  A queue action the drain cannot apply parks, and every run re-reports it until it is repaired. The parked rows were read where the drain runs, which is once per source, while the read itself is of the whole store: the queue records no source. An account syncing mail, contacts and calendar therefore showed one parked row as three identical warnings, the shared `#1` being the only hint that they were one problem. They are now read once, after every source has drained.

- A dry run copied the account's whole store before it started, and left the copy behind when it failed.

  A dry run works on a throwaway replica so that no checkpoint advances and nothing reaches a server, and that replica was a deep copy of the store into the temporary directory, taken before the first spinner and logged at no level. For a mail account that is the blob tree: 2.5 GB over 9511 files for a real account, of which 13 MB is the index and the rest is bodies. Every dry run read and wrote all of it, several silent seconds before anything appeared on screen, and spent that as memory rather than disk wherever `/tmp` is a tmpfs. The replica is now built beside the real store, so the two share a filesystem and the bodies are hardlinked rather than copied: what is copied drops to the index, and the preparation is logged with the time it took. Bodies are content-addressed and a dry run neither rewrites nor purges one, so sharing them is sound; everything the run writes to is still its own, and a file that rule misjudges is copied rather than shared, so being wrong costs speed and never the real store.

  Removing the replica was the last line of the run, so any earlier failure, a credential that would not resolve, a refused mode change, a store that would not open, returned before it and left the copy behind. It is now removed however the run ends, and a run clears what an earlier one left, a release build aborting on a panic without running destructors.

- A contacts run that downloaded an address book reported itself already in sync.

  The pull plan is the set of items carrying no body yet, and it was read after the probe that resolves link ids. A card's link id is its vCard `UID` and cards have no cheap summary tier, so the probe downloads the whole card to resolve it: by the time the plan was read every card was hydrated and the plan was empty. Mail resolves its link id from an `ENVELOPE`, leaves its bodies for the hydration phase, and reported them, which is why the two kinds disagreed — and `--dry-run`, which does not hydrate during the probe, reported what the real run stayed silent about. The plan is now read before the probe, so both kinds and both run modes name the same bodies.

- A run whose bodies never arrived reported itself in sync, forever.

  A batched fetch that answered for fewer members than it was asked about counted as a success: 2 cards came back for 64 handles, the engine recorded those 2, heard nothing about the other 62, and could not tell them from handles it had never asked about. So it asked again on the next run, and the next. Every run enumerated the collection, fetched it, stored nothing and printed "already in sync", while a dry run beside it counted the same hunks each time. A shortfall now falls back to per-item fetches, as a batch error already did, and names the count.

- An empty body poisoned the store.

  A zero-byte body hashes to the digest of nothing, so every empty item a server returned resolved to the same link id; the second collided with the first, the duplicate-link-id floor froze that identity, and the collection stayed frozen on every later run. A zero-length body is now refused with the item named. No kind neverest syncs has an empty body: a message carries headers and a card carries at least its `BEGIN` and `END` lines.

- An address book on a server without RFC 6578 never synced.

  Enumeration is `REPORT sync-collection`, and a server may implement none of it: its `supported-report-set` then holds `addressbook-multiget` and `addressbook-query` alone, and the REPORT comes back with the RFC 3253 §3.6 `DAV:supported-report` precondition. The client recovered from a rejected sync *token* and had nothing for a server that never had the report, so every address book on such a deployment was unsyncable. Such an address book is now listed with a `PROPFIND` at Depth 1, which yields the same ids and ETags; that carries no token, so those collections enumerate in full on every run. A `PROPFIND` rather than the `addressbook-query` the same server does advertise: a query carries a filter the server evaluates by parsing every card, so one card it cannot parse fails the whole enumeration, where a `PROPFIND` reads names and ETags out of the store and lists the collection past it. The choice is made from the advertised report set, which a sync run already pays for when it lists the address books, and from the precondition otherwise, never from the HTTP status the server chooses, so a permission refusal or a server fault still surfaces as the failure it is. A listing the server truncates is reconciled as a delta rather than as a snapshot, an incomplete member set read as the whole address book being a mass deletion.

- A run that failed to scan a collection reported itself in sync, and swallowed the reason.

  A collection whose spine failed was logged as a warning and left out of the report, so the run printed "already in sync" about a collection it had never managed to look at. It is now recorded as a failed hunk, so the run reports it and exits non-zero. The engine wrappers also rendered their errors with `Display`, which prints the outermost context and drops every source beneath it, so a backend's HTTP status and response body, kept verbatim precisely so a caller can read them, never reached the operator. Enumerate, fetch and push now render the full chain.

- A dry run over a contacts account reported it already in sync, and downloaded every card to do it.

  First-time discovery reaches the report through the fetch itemizer alone, and it read the hub projection, which drops the residual. The residual is where a freshly probed item sits until its link id is known, and a card has none before its body arrives, a `sync-collection` REPORT returning hrefs and ETags but no `UID`. Every item of a first contacts sync was therefore invisible to the plan, so an account that had never synced was told it was in sync while a mail source beside it reported thousands of hunks; mail escaped only because its probe resolves at `Meta`, which moves it into the hub. The plan now reads the side, projection plus residual. It also keys on the stored object rather than the detail level, so an item whose body a remote content change dropped is named rather than read as complete.

  The probe upgrade also ran before the dry run stopped, so previewing a DAV account fetched its whole address book to print a plan. It is now skipped where a dry run would have to reach `Full`. A cheaper tier still resolves, so a mail preview keeps naming messages by their `Message-ID`; a card stays probed and is named by its href.

- Every `server` accepts a bare authority, port included, not just the portless spelling.

  A bare authority is not a relative URL: `url` reads `posteo.de:8843` as the scheme `posteo.de` with the path `8843`, which parses cleanly and carries no host. Resolution decided when to prepend a scheme by matching on the parse error, so `imap.example.com` worked and `imap.example.com:143`, which config.sample.toml documents, reached io-imap hostless; a CardDAV `server` required a full URL outright and reported `posteo.de:8843 has no host` from inside io-webdav. All three backends now resolve through one function that splits on `://`, so an authority takes the backend's default scheme (`imaps`, `smtps`, `https`) and a value carrying a scheme is used verbatim.

- Queued submissions no longer fail against a server that checks the greeting.

  The SMTP session greeted with `EHLO localhost`, which RFC 5321 §4.1.4 entitles a server to reject, and one that does answers `550 5.5.0 Invalid EHLO domain`, so the session died before `MAIL FROM`. The failure being transient, the intent stayed pending behind a warning: the queue filled and no mail ever left. The greeting is now the loopback address literal RFC 5321 §4.1.3 reserves for a client with no resolvable name.

- A source that may not delete no longer resurrects the item on every source.

  A refused delete was undone rather than held, which states that the source still holds the member; the shared store reads that as the item being alive and clears the deletion everywhere, so removing an item once brought it back. The tombstone now stays until a run that may push delivers it.

- A store's disk is now reclaimed.

  The store deliberately collects nothing by itself, leaving reclamation to its owner, which is neverest. Nothing ran it, so every dereferenced body stayed on disk for ever while `store.purge-after` reported bytes it had merely released. The sweep now runs the collector after a purge that took something, and a store that has been running for a while reclaims its backlog on the next sweep. Orphan blobs left by a crash are still `pimdir gc`'s to take.

- An identity a collection holds twice no longer costs the other source its copy.

  Two messages with the same `Message-ID` used to pair arbitrarily: deleting the paired copy propagated a delete that removed the only copy on the other source, and a later checkpoint loss revived the retained row and re-appended it while reporting `already in sync`. The engine now freezes such an identity and derives nothing for it in either direction.

- A relayed copy is now reported.

  A relayed body never reaches the projection the report is built from, so every relayed append was invisible and a run that appended messages could print `already in sync`. Each relay is itemized where it happens.

- A handle-space rebuild no longer freezes the collection.

  A `UIDVALIDITY` bump renumbers every message, and the store read the rebuild as one server reporting each identity under a second handle: it kept the voided handles, marked every item ambiguous and stopped syncing in both directions. Fixed in io-pimdir.

- A CardDAV source no longer dies after its first request.

  A server that closes the connection between requests left every later exchange writing into a socket the peer had hung up on, so discovery itself failed. The connection is reopened and the exchange run again, carrying the discovered principal and home-set URLs over. Only an end-of-stream or reset failure is retried, so a create or a delete is never replayed against a server that acted on it.

- A DAV collection no longer fails its scan.

  Freshly probed items were raised to the `Meta` tier whatever the kind, which asks a CardDAV source for a summary tier it has none of. The tier is now the kind's.

- A body is hydrated by the absence of a stored body rather than by the item's detail level, so an item whose stale body was dropped is picked up again.

- `tls.cert = "~/ca.pem"` now reads the certificate in your home directory rather than a literal `./~/ca.pem`.

  The key was the one path in the schema declared bare, so it reached the TLS layer exactly as written while `store.root` beside it was expanded at deserialize. Every path key now expands in the same place, which is where a call site cannot forget it.

### Removed

- Removed the `notify` cargo feature and the `conflict.notify` key; neverest raises no desktop notification of its own.

  The once-only rule it existed for is already in the report: `conflicts` is what a run marked and `outstanding_conflicts` is what the store holds waiting, so a caller reading `--json` notifies on entry, once, with no state to keep, and can name the item, which a fixed summary and body could not. The exit code is not that signal, being wider: it means the run left something waiting, a refused duplicate or a rejected write included. `dbus` leaves the devshell and the package with it, along with an rpath workaround and an aarch64 atomics override. `conflict.merger` is unaffected. The README and config.sample.toml carry the replacement recipe.

- Removed the `merge` cargo feature; the three-way merge now rides on `dav`.

  The sync spec requires without condition that a run merges a marked conflict, and that the merge is "built in rather than configured". A feature gating it made that false in the builds that omitted it. It could not earn its keep either: it was declared `merge = ["dav", ...]`, so it enabled another feature, and every mutable-content kind is already `dav`-gated, so it was co-extensive with `dav` across everything it could act on. `dep:ical-rs` and `dep:vcard-rs` moved into `dav`, and no build anyone would have made changes behaviour: `default` carried `merge`, and `--features merge` pulled `dav` in regardless.

- **BREAKING**: removed local file backends as sync sources (Maildir, then m2dir).

  A source is a remote, and the pimdir store is the local replica, so a local file store beside it would be a second local copy. An existing on-disk tree is brought in through io-pimdir's conversion tooling instead.

- **BREAKING**: removed the **Notmuch** backend, with no replacement in the `io-*` ecosystem yet.

- **BREAKING**: removed collection aliases (`collection.alias`, the v0.1 `folder.aliases`).

  The table parsed and nothing ever read it: substituting a friendly name for a backend id is display work, and neverest renders nothing. The store already carries per-collection display metadata for the frontend that does. A configuration still declaring the table is refused rather than silently ignored.

- **BREAKING**: removed the built-in keyring and OAuth support.

  Secrets come from a command instead, so any secret manager works, and [ortie](https://github.com/pimalaya/ortie) issues and refreshes OAuth access tokens.

- **BREAKING**: removed `envelope.filter`, the per-source folder aliases, the `-o` output flag and the `--color` flag. Color now follows the terminal, and `--json` replaces `-o json`.

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
