---
cairn: delta
change: the-store-is-shared
---

# Delta

## ADDED Requirements

### Requirement: Accounts share one store
Every account naming no `store.root` SHALL sync into one store, at the default root, and accounts naming one `store.root` SHALL share that store: one database and one blob directory, a body two accounts hold stored once. An account SHALL be one hub within the store, its collections grouped under it (see "A collection records the account that syncs it") and its sources never meeting another account's. `sync --source <name>` SHALL narrow which sources of each account run.

Sources in one account never meet, whatever command invoked them: an item one holds crosses to the account's targets, never to another source. Two genuinely independent replicas SHALL be two accounts.

A source whose sync fails SHALL be reported and SHALL NOT stop the others: they share nothing but the file the store lives in.

#### Scenario: Two accounts, one store
- **GIVEN** two accounts naming no `store.root`
- **WHEN** both are initialized and synced
- **THEN** one pimdir.db holds both, each collection grouped under its account

### Requirement: A store laid out by an earlier neverest is refused
When the default root holds no pimdir.db yet but one of its immediate subdirectories does, a store an earlier neverest kept per account, neverest SHALL refuse before creating anything, as `store-layout`, naming each such store and the order to follow: sync it with the neverest that wrote it until nothing is queued, move it out of the neverest directory, then run again. It SHALL NOT say to delete it, the store holding what no server has (its queued writes, its retained items). Once the default root holds a pimdir.db, its subdirectories are not looked at.

A store whose sidecar is of an earlier shape, or a store directory holding a database and no sidecar, SHALL be refused the same way, as `store-layout`, naming the same order for that store. Neither is migrated: pimdir stores are reinstalled (pimdir STORAGE §6), and so is neverest's state beside them.

#### Scenario: Stores kept per account
- **GIVEN** a neverest directory holding `work/pimdir.db` and `home/pimdir.db` and no pimdir.db of its own
- **WHEN** any command runs over an account naming no `store.root`
- **THEN** it exits 1 with `code` `store-layout`, naming both stores, the sync with the previous neverest until nothing is queued and the move out of the directory, no delete, and the directory holds nothing new

### Requirement: The sidecar holds each account
neverest.json SHALL record the collection-id layout once for the store, and, under each account it initialized, the mode it last synced under, whether a mode change was accepted, the roles a listing does not carry and the calendars a listing states take no tasks. An account SHALL be initialized when it has an entry: a run over an account with none SHALL fail that account, naming `neverest init -a <account>`.

`init` SHALL add the account's entry, creating the store and its sidecar in one act when there is none, and SHALL leave an account that has one as it is, reporting it initialized already rather than failing: its store may hold work no server has. `sync --reset` SHALL replace the entry; a run SHALL save its account's entry and leave every other as it read it.

#### Scenario: Two accounts, two modes
- **GIVEN** a store shared by a two-way account and a one-way one
- **WHEN** the two-way account turns `one-way = true` and is synced
- **THEN** that run is refused naming `--accept-mode`, and the one-way account still syncs

### Requirement: `-a` filters the accounts a command runs over
`-a/--account` SHALL be repeatable and SHALL filter the configured accounts. Without it, `sync`, `check`, `drain` and `init` SHALL run over every configured account, store by store and in name order within a store, one after the other inside one run holding each store's lock once; with it, over the named ones. The configuration is merged into a sorted table before neverest reads it, so name order is the only order it can keep. A name the configuration does not declare SHALL fail the command before anything runs, naming the configured accounts, and so SHALL an account whose configuration does not validate, the configuration being one document.

Each account's failure SHALL be its own: the run SHALL go on with the next account and SHALL end with one result listing every account's outcome, a failed one carrying its error and its `code`. The command SHALL exit 0 when every account succeeded, otherwise with the partial-outcome code its own outcomes give (2 for a sync that left work waiting for a person), a failed or a busy account making it 3, the code of a run that could not do all its work. A failure common to the run (no configuration, a store refused as a whole) SHALL fail the command with the failure object, exit 1.

`index` is store-wide: it SHALL build the stores of the accounts the filter keeps, each once and whole, every account of a store included. `conflict show`, `conflict resolve` and `sync --reset` address one item or start one account over, so they SHALL need exactly one account: a filter leaving none or several SHALL fail, naming the accounts. `conflict list` SHALL list the conflicts of every account it runs over, each row naming its account. `configure` SHALL take no account.

#### Scenario: Two accounts, one of them failing
- **GIVEN** two accounts, the first initialized and the second not
- **WHEN** `neverest sync --json` runs
- **THEN** the first is synced, the result lists both, the second with its error naming `neverest init -a <account>`, and the run exits 3

#### Scenario: An unknown account
- **GIVEN** a configuration declaring `work` and `home`
- **WHEN** `neverest sync -a perso` runs
- **THEN** it exits 1 before anything runs, naming `home` and `work`

#### Scenario: A reset needs one account
- **GIVEN** a configuration declaring `work` and `home`
- **WHEN** `neverest sync --reset` runs with no `-a`
- **THEN** it exits 1, naming both accounts, and resets nothing

### Requirement: `default` is accepted and ignored
An account's `default` key SHALL be read and ignored, with one warning a run naming the key, so a configuration an earlier neverest wrote still loads: a command runs over every account unless `-a` names some, so no account is the default.

### Requirement: A check names the accounts a store holds that no configuration names
`neverest check` SHALL report, under `orphans`, every account a store the run reads holds collections of (pimdir `list_accounts`) and no configured account sharing that store names, with the store: renaming an account's TOML key leaves its collections under the old name. The report SHALL say the key must be renamed back to reach them, and SHALL NOT delete them nor change the exit code.

#### Scenario: A renamed account
- **GIVEN** a store holding the collections of an account `old`, and a configuration naming the same store under `new`
- **WHEN** `neverest check --json` runs
- **THEN** `orphans` names `old` with its store, and the collections of `old` are still there

## MODIFIED Requirements

### Requirement: An account holds named sources
An account SHALL hold a map of named sources (`sources.<name>`), each declaring exactly one remote backend, and SHALL require at least one. It MAY additionally hold a map of named targets (`targets.<name>`) on the same terms. Both map keys SHALL be the names its pimdir source ids are made from, so a name is what every binding it owns in the store is recorded against, and a rename SHALL be treated as a new source. One name SHALL NOT be both a source and a target.

An account SHALL NOT constrain its sources to one kind: mail, contacts and calendar sources may sit under one account, and their collections never meet (see the collection key requirement).

`left` and `right` SHALL NOT survive in any form, as keys, as aliases, or as source ids. A configuration carrying them SHALL be refused at load, naming `sources` and `targets` and the `one-way` flag that declares the direction they never could.

`collection.namespace` SHALL likewise be refused by name, naming `targets` and `one-way`. It expressed which sources met, which is now the arity, and never expressed which way, which is now `one-way`.

A store written before collection ids carried their namespace SHALL NOT be read. Neverest keeps its own state beside the store (neverest.json, see "The sidecar holds each account"), and a store directory holding a database but no such file SHALL be refused as "A store laid out by an earlier neverest is refused" says.

Whatever materializes the database SHALL write the sidecar in the same act, so that pair only ever describes a store an older neverest wrote. `init` SHALL stamp the account it initializes, and `sync --reset` SHALL stamp the account it starts over.

#### Scenario: Mail and contacts under one account
- GIVEN an account declaring an IMAP source and a CardDAV source
- WHEN it is synced
- THEN both run against the same store, and neither is refused for disagreeing on its kind

#### Scenario: A two-side config is refused with its replacement
- GIVEN a configuration written with `left` and `right`
- WHEN it is loaded
- THEN it is refused, naming `sources`, `targets` and `one-way`

#### Scenario: A fresh account syncs
- GIVEN an account `init` has just initialized
- WHEN `sync` runs, with or without `--dry-run`
- THEN the store is read rather than refused as the unnamespaced ancestor

#### Scenario: A database with no sidecar
- GIVEN a store directory holding a database and no sidecar
- WHEN a command opens it
- THEN it is refused as `store-layout`, naming the sync with the neverest that wrote it and the move of the directory, and nothing is written

### Requirement: A mode that would discard data is refused, not warned
The store's state file SHALL record the mode triple (arity, `one-way`, `retain`) under each account when the account is initialized, and every run of the account SHALL compare its configuration against it.

A run whose `one-way` moved from false to true SHALL be refused, for that account alone. The previous mode preserved changes on the side the new one discards, so the first run after the edit is the one that loses them. The refusal SHALL name a one-time acknowledgement (`sync --accept-mode`), which SHALL let that run through and record the new mode, and SHALL NOT name `init` or `--reset`, which drop the account's collections and are a heavier remedy than the situation calls for.

A `retain` that moved from true to false, and a change of arity that does not turn `one-way` on, SHALL be reported and SHALL NOT block: bodies already stored stay, unreferenced, until an explicit `pimdir gc`.

The comparison SHALL gate on those transitions and not on configuration change in general. A rotated credential, a new filter or an added source in a no-targets account threatens nothing, and forcing a resync for one would cost a mailbox.

A first run with `one-way = true` against a non-empty target has no recorded mode to compare against and SHALL NOT be gated. `init` SHALL instead state the account's behaviour in words.

#### Scenario: Turning on one-way stops the run that would discard
- GIVEN a two-way account synced at least once
- WHEN `one-way = true` is added and the account is synced
- THEN the account is refused before any write, naming the acknowledgement that records the change, and the run goes on with the next account

#### Scenario: The acknowledgement lets the run through
- GIVEN that refused account
- WHEN it is synced with `--accept-mode`, then synced again without it
- THEN both runs sync it, the first recording the new mode

#### Scenario: A rotated password is not a mode change
- GIVEN an account whose secret command changed
- WHEN it is synced
- THEN the run proceeds, the recorded mode being unchanged

### Requirement: A collection is keyed by kind, namespace and name
A hub collection SHALL be keyed by the triple `(kind, namespace, name)`: the source's media type, its namespace, and the collection name the backend enumerates. The namespace SHALL be internal, derived from the source name, and SHALL NOT be configurable: it exists so that a CardDAV address book and a mailbox carrying the same name key apart, not so that a user can decide which sources meet. A target binds the namespace of the source it is paired with, which is what makes the two meet.

The id is spelled `<account>/<namespace>/<name>` with the kind on the collection row, the account keeping two accounts' collections apart in a shared store, and the account and namespace prefix SHALL be stripped back off before any call reaches a server, at one seam, so a backend only ever sees the name it gave. A report SHALL name a collection the way its server does, not the way the store keys it.

Every wire call SHALL pass through that seam, including the ones a hydration pool makes on its own connections rather than through the remote, and including a collection named as an argument rather than as the target: a move destination is a hub id like the collection it leaves. A cache keyed by collection SHALL keep the hub id as its key, the seam being the wire call and not the plan.

The source's `collection` table SHALL keep `create` and `delete` optional, defaulting to granting, unlike the `item` table which requires its pair to be declared in full. The table also carries `filter`, so it is declared for reasons that have nothing to do with permissions, and demanding a permission pair from someone writing a filter would be a trap.

### Requirement: The pimdir store is the sole local copy
A message body SHALL be held locally exactly once: content-addressed in the pimdir blob store (where the derivation keeps bodies), deduped across sources, collections and accounts, and Neverest SHALL keep no parallel local copy in another format. Sync sources are remote backends only; an existing on-disk store (maildir/m2dir) is brought in through io-pimdir's conversion tooling, not synced as a source. The store is pimdir.db plus an `objects/` blob directory, shared by every account naming its root.

### Requirement: The default store root resolves on every supported platform
An account whose configuration names no `store.root` SHALL resolve its store under a per-platform base: the XDG state directory on Linux and the BSDs, `~/Library/Application Support` on macOS and iOS, `%LOCALAPPDATA%` on Windows. A platform SHALL NOT be left resolving through a mechanism it does not implement, since every account-touching verb reaches this and the wizard does not, so the failure lands after a configuration was successfully written.

Only the base SHALL vary. The store SHALL be `neverest/` under it on every platform, a directory rather than a file and one for every account naming no root, so one relative path addresses a store whoever wrote it. `store.root` SHALL override the whole resolution, for that account.

A roaming or cache location SHALL NOT be chosen where the platform distinguishes one: the store holds message bodies and a SQLite index, is the only local copy when the account retains bodies, and costs a full resync when dropped.

#### Scenario: A store resolves without a hand-written root
- GIVEN an account naming no `store.root`
- WHEN it is initialized on Linux, on macOS or on Windows
- THEN the store resolves under that platform's own base, at `neverest/`, and no directory is made for the account

### Requirement: A collection records the account that syncs it
Every store handle SHALL be opened for the account being synced, so each collection it writes is grouped under that account (pimdir STORAGE §9.2), and a reader reads one account through it (`pimdir.account` in himalaya, calendula and cardamum). Within the account, a collection is further keyed by its source's namespace, which is its name, so two sources of one kind are told apart without inferring it from the collection naming.

The store-wide ids SHALL carry the account (pimdir STORAGE §4.3, §9.2): a pimdir source id is `<account>/<source>` and a collection id `<account>/<source>/<name>`, so two accounts holding a source of one name, the wizard's `imap` among them, neither write into one collection nor replace each other's declarations. An account name holding `/` SHALL be refused, naming it: `a` with `b/c` and `a/b` with `c` would spell one id. A report and an event SHALL name an endpoint as the configuration does, the account being named beside it.

#### Scenario: Two accounts with one source name
- **GIVEN** two accounts `work` and `home` sharing the store, each with an `imap` source
- **WHEN** both are synced
- **THEN** the store holds `work/imap/INBOX` under `work` and `home/imap/INBOX` under `home`, and declaring one leaves the other's capabilities in place

### Requirement: A store the format outgrew is refused with its remedy
A store opens under its own format version alone (pimdir STORAGE §4.2, §6): one any earlier neverest wrote, naming no `store_meta.format_version` or an earlier draft, is refused as `store-version` and never migrated. The refusal is the store's, so it SHALL fail the command before any account runs, naming the reinstall in the order that keeps its queued writes, which live only in that store: sync it with the neverest that wrote it until nothing is queued, move the store directory away, then run again (`neverest init`, then a sync), rather than surfacing the raw refusal alone. The message SHALL never say to delete the store. What the old store held that no server holds (retained items, whatever was still queued) stays in the store moved away. MIGRATION.md SHALL give the same order.

#### Scenario: A store an earlier neverest wrote
- **GIVEN** a store whose `store_meta.format_version` names `storage-draft-08`
- **WHEN** neverest runs `--json drain` on the account
- **THEN** it exits 1 with `code` `store-version`, the message naming the sync with the previous neverest until nothing is queued and the move of the store directory, and no delete

### Requirement: A run holds the store lock, waiting bounded
A run SHALL hold an advisory sync.lock in the **actual** store directory (honouring `store.root`) for its whole length, whatever accounts it covers, taken once for each store it covers, in path order, before any account runs. A second run SHALL wait for the holder up to a bounded timeout (60 s) and then report every account of that store `busy` and exit 3, so cron ticks and connector-triggered scoped runs serialize instead of failing or corrupting.

The lock is the store's, as pimdir's owner lock is (STORAGE §8: one owner process per store), so two accounts of one store never sync at once. Accounts needing independent schedules SHALL give themselves their own `store.root`.

#### Scenario: Another account of a busy store
- **GIVEN** a run holding the store's lock
- **WHEN** `neverest sync --json -a <another account of it>` runs
- **THEN** it waits the bounded time, then prints that account `busy` and exits 3, its store untouched

### Requirement: Neverest is the store's sole owner and drains the queue first
Neverest SHALL be the only process writing a pimdir store; frontends read it and enqueue mutations through io-pimdir's producer queue. A sync run SHALL drain the store's queue once, at its start, after declaring the sources of every account it runs over and before reading any credential or opening any endpoint, so a run that cannot reach its servers still applies what was queued: exactly-once apply-and-delete per action, permanently bad actions parked, transient failures left queued in order. The store applies each action as the source syncing its collection (pimdir STORAGE §15.2), so the drain is the store's and not a source's, every account's queue included. `--declare-only` SHALL NOT drain; `--intents-only` SHALL; a dry run drains each account's replica. The applied counts SHALL be logged (info when nonzero) and reported under the account owning the collection; a row on a collection of an account the run does not cover is applied and reported by no account, its producer reading its receipt.

Every parked action of an account's collections SHALL surface in that account's report until repaired, and SHALL surface **once per run**, read after every source of the account has run, in a dry run as much as in a real one.

The subsequent sync of a drained collection pushes the resulting dirty state. An action kind the drain cannot apply itself (a capability-bound intent such as `submit`) SHALL be left pending for the phase that can, never parked.

#### Scenario: A sync that cannot connect
- **GIVEN** an event queued on a calendar whose server is unreachable
- **WHEN** the account syncs and exits 3
- **THEN** the event is in the store, with its `seq`

### Requirement: A drain applies the queue without syncing
`neverest drain` SHALL apply the queued actions as a sync's drain does, after declaring the sources of every account it runs over as a sync does before its drain (the configuration and the sidecar), reading no credential and opening no endpoint. The drain is the store's: it applies every account's queue, once a store, and each account's entry SHALL report the rows of its own collections it applied, with the `seq` of the item an `add` created, the rows it parked, and how many it left for a sync. It SHALL NOT wait for the store: while a run holds `sync.lock` it SHALL report the accounts of that store `busy` and exit 3, the queue untouched.

### Requirement: The wizard generates an account and never edits one
`neverest configure` SHALL generate a new account. It SHALL NOT read an account back, seed the prompts with its values, or write it out again: editing an account, adding a second by hand, and everything the prompts do not cover belong to the file and the user's editor, against the documented sample.

`configure` SHALL take no account: `-a` names an account to run against, and there is nothing to name when the wizard generates. The dispatcher SHALL NOT hand it one.

The account name SHALL be derived and never prompted, being only the table key, and it SHALL be free: the wizard SHALL suffix the name discovery proposes (`posteo`, `posteo-2`, …) until the configuration does not already hold it. A second `[accounts.<name>]` table makes the whole document fail to parse, taking the accounts that used to work down with it.

The generated account SHALL NOT write `default`: every command runs over every account unless `-a` names some.

A configuration file that fails to parse SHALL be an error rather than read as absent: appending to a broken document buries the real problem under a second one.

### Requirement: A bare invocation offers the wizard only on a first run
Running `neverest` with no subcommand SHALL print the help, except on a machine with no configuration, where it SHALL offer the wizard first, as a bare `himalaya` does. The wizard targets the first `--config` path when given, else the default one. A configuration that fails to parse counts as present, so the offer never proposes to write over a broken file; the parse error surfaces when a real command reads it. A declined offer SHALL fall back to the help, a bare invocation having nothing else to run.

The offer SHALL be skipped, and the help printed, in JSON mode and when stdin is not a terminal, neither being able to answer a prompt. It SHALL also be skipped when `--account` names an account: with no subcommand that is a half-typed command rather than a first run, and the help is what points at the commands.

`neverest configure` itself SHALL refuse to run when stdin is not a terminal, naming the documented sample as the way out: a wizard cannot prompt a cron job.

The wizard SHALL NOT write a configuration file unconditionally: it SHALL ask for confirmation before saving to a path holding no file, SHALL ask before appending to one that does ("Append account `<name>` to `<path>`?"), and SHALL print the generated TOML document on stdout when either confirmation is declined, so a generated account is never lost. In JSON mode or when stdout is not a terminal, the wizard SHALL emit the document on stdout and touch no file, so `neverest configure > config.toml` and scripted runs keep working.

Appending SHALL be a plain text append of `"\n<document>"` to the file opened in append mode. The wizard SHALL NOT parse a configuration file and serialize it back: comments, ordering and hand-written formatting are not in the parsed model, and re-serializing destroys every one of them.

A saved account SHALL be reported on stderr, naming the file it landed in and the name it landed under, since that name was never asked for, and SHALL point at `neverest init` and `neverest sync`, which run over every account.

A command that finds no configuration file SHALL propose the wizard, under the same two guards, and SHALL then read the configuration again rather than trust the wizard's result, which is only printed when the save is declined. A command still finding none SHALL fail naming the path it looked at and the documented sample; it SHALL NOT exit reporting success.

### Requirement: The generated configuration is a dotted document
A configuration neverest writes or prints SHALL render as Himalaya's does: one `[accounts.<name>]` table header per account, the only header in the document, with every field below it written as a dotted key. An empty table SHALL write nothing. The saved file and the document printed on stdout SHALL be identical.

A rendered account SHALL be readable in that order rather than the serializer's: the groups SHALL be ordered with the backend the wizard wrote before the sync options it never writes, each group SHALL be separated by a blank line, and the key naming what a group points at (`server`, `user-id`) SHALL be lifted to the top of its own, ahead of the credential authenticating against it.

An account naming several sources SHALL render under that same single header, its sources being dotted keys like every other field, so appending a table after it never opens a header a later account would fall into.

The document SHALL hold only what was actually decided: every field equal to its default SHALL be omitted (`default`, which the wizard never sets, the per-source collection / flag / item permissions, the per-source pool size, the collection filter, the HTTP-backend ALPN list, `starttls`). Omitting a field SHALL be lossless: every skipped field keeps a deserialization default equal to the value that was skipped.

### Requirement: A run reclaims retained items on a schedule
The store retains an item rather than deleting it when its last binding vanishes, so reclaiming is the client's schedule. An account SHALL configure it as `store.purge-after`, a human duration (one integer plus `s`/`m`/`h`/`d`/`w`). **Unset SHALL mean never purge**; `"0"` SHALL purge immediately, reproducing a terminal delete. There SHALL be no boolean beside it: the delay is the switch.

A sync run SHALL sweep **after** the sync and before the report is finalised, on both the pair and the solo paths, never in a dry run, purging every retained item of the account's own collections whose `retained_at` precedes `now - purge-after` (RFC 3339, the shape the store stamps), one item at a time (`purge`), never another account's: the store's own sweep (`purge_retained_before`) is store-wide, and one account's delay would decide for every other. Sweeping after the sync means an item this run retired starts its delay now rather than being reclaimed by the run that retired it. The sweep SHALL warn rather than fail the run, as the send channel does, and `sync --no-purge` SHALL skip it. The report SHALL carry what was reclaimed (items and bytes) in both the text and `--json` output.

A read-only source (`<protocol>.item.delete = false`, `collection.delete = false`) with no purge delay is therefore a backup: a remote expunge retires the local row without losing the item or its body.

#### Scenario: One account's delay
- **GIVEN** a store shared by an account with `store.purge-after = "0"` and one with none, each holding a retained item
- **WHEN** the first account's retention sweep runs
- **THEN** its retained item is purged, and the other account's stays

### Requirement: A reset never loses work no server holds
`sync --reset` SHALL act on exactly one account and SHALL start it over inside the store it shares: it SHALL drop the account's collections (pimdir GUIDE "Removal": `delete_collection`, the refcounts recomputed in its transaction, then the collector), with what the store records under them (items, bindings, checkpoints, coverage, capability rows), the account's source-wide capability rows and its sidecar entry, stamp the entry again and sync the account anew. Every other account SHALL be left as it was.

It SHALL read the account's collections before dropping any, and SHALL refuse while one holds a queued or parked action, which io-pimdir refuses to drop with its collection: the refusal SHALL name each row and the way out, the queued ones applied by `neverest drain` or a sync, the parked ones cancelled from the frontend that queued them. Without `--force` it SHALL also refuse while they hold other work no server has: a conflict, an item whose flags or body no binding's base holds (a staged edit or create), a staged delete, or a retained item, bound to no server, listing that work per collection and naming `--force`, which drops that work with the collections.

`--reset` SHALL be refused beside `--dry-run`, which would drop the real collections before planning, and beside a collection or source filter, the reset being the whole account's.

#### Scenario: A queued send survives a reset
- **GIVEN** an account whose collection holds a queued action
- **WHEN** it is synced with `--reset`, forced or not
- **THEN** the run refuses, naming the row and `neverest drain`, and the store is untouched

#### Scenario: A reset leaves the other accounts alone
- **GIVEN** two accounts sharing a store, each holding a collection
- **WHEN** one is reset
- **THEN** its collections and their checkpoints are gone and its sidecar entry starts over, and the other's collections, items and entry are as they were

#### Scenario: A forced reset drops staged work
- **GIVEN** an account holding a retained item
- **WHEN** it is synced with `--reset`
- **THEN** the run refuses naming `--force`, and with `--reset --force` the account's collections are dropped

### Requirement: A store is its user's alone
A store another user can read or write is refused by every profile as `store-exposed` (pimdir STORAGE §18). Every store directory neverest makes SHALL be its user's alone, so its own readers and frontends open it: the store `init` creates, which io-pimdir restricts at creation whatever the umask, and the replica a dry run copies the store into, created `0700` before the copy, its files keeping the store's modes.

#### Scenario: A store made under umask 022
- **GIVEN** a shell whose umask is 022
- **WHEN** `neverest init` creates an account's store
- **THEN** its directory and pimdir.db grant nothing to group or others, and a reader and a producer open it

#### Scenario: A dry run under umask 022
- **GIVEN** a store and a shell whose umask is 022
- **WHEN** `sync --dry-run` runs
- **THEN** its replica opens as a store

### Requirement: The store's owner builds its search index
A build with the `search` feature (on by default) SHALL refresh the store's search index (index.db, pimdir SEARCH §3, §4) after each account a sync runs over, last, for at most the account's `store.index-timeout` (60 seconds unless set, `"0"` turning it off), and after a drain, a run performing the queued intents alone and a run declaring collections, for at most 2 seconds. A dry run SHALL NOT refresh. A refresh cut by its deadline SHALL leave the index behind the store and the next refresh SHALL go on from there.

The run SHALL report the refresh under `index`: the changes folded, the bodies newly indexed, the stamps still `behind`, whether another indexer held it (`busy`), and the error of a failed one. An index failure SHALL be a warning and SHALL NOT fail the run or change its exit code: the index is derived from the store.

`neverest index` SHALL build the index of each store the accounts it runs over use, once a store and whole, every account of the store included, until it holds every change the store has, reading no credential and opening no endpoint, and SHALL exit 3 answering `busy` while another indexer holds one.

A reset SHALL leave the index to its next refresh, which folds the removal of every item the dropped collections held, the store counting each (pimdir STORAGE §4.5).

An index of the canonical shape at an earlier search draft SHALL be rebuilt by the next refresh; an index of another shape, or at a later or unknown version, SHALL be refused as `index-version` and left as it is, never rebuilt, written nor deleted (pimdir SEARCH §2), the refusal reported under `index` like any index failure. Every refresh SHALL state the user's zone as the index's floating zone (pimdir SEARCH §7, §8.1): the IANA name of the zone `TZ` names, else of the system's; a zone with no IANA name (a POSIX `TZ` rule) or none found SHALL read as `UTC`, with one warning a run. The first refresh in another zone than the index records re-expands every calendar item.

#### Scenario: A message a sync landed is found by a frontend
- **GIVEN** a sync landing a message whose subject says "Quarterly report"
- **WHEN** a frontend opens the index for queries alone and searches `quarterly`
- **THEN** it finds the message

#### Scenario: A first build longer than the deadline
- **GIVEN** a store whose index a refresh's deadline cut
- **WHEN** the next run refreshes it
- **THEN** it goes on from where the cut left it until the index is current

#### Scenario: A floating meeting in Paris
- **GIVEN** `TZ=Europe/Paris` and a calendar holding a meeting at a floating 09:00 on 7 January
- **WHEN** a run refreshes the index
- **THEN** the index records `Europe/Paris` as its floating zone and places the meeting at 08:00 UTC, 09:00 in Paris

### Requirement: A store error carries its code
A store failure that ends a command SHALL carry, in its JSON failure object, the code io-pimdir gives it (`PimdirError::code`), spelled the same: `store-version`, `store-uncreated`, `store-foreign`, `store-corrupt`, `store-not-current`, `store-hash-algo`, `sync-version`, `store-busy`, `store-owned`, `store-staging`, `store-in-flight`, `store-exposed`, `action-malformed`, `flag-invalid`, `unsupported`, `index-fts5-missing`, `index-version`. A store neverest's own layout refuses SHALL carry `store-layout`. A failure a caller cannot act on (an SQL or I/O error) SHALL carry none, nor SHALL a code neverest cannot meet (`index-busy`, which `index` answers as `busy`; `index-missing`, `index-outdated` and `index-query-only`, a query client's; `index-zone-unknown`, neverest stating only a zone its time zone database named; `cutoff-invalid`, neverest writing its cutoff as the store stamps; `conflict-unread`, which the one-way overwrite answers by fetching the unread body). A failure folded into one account's outcome SHALL keep its code. No message SHALL advise deleting the store: the remedies are an upgrade, the reinstall of "A store the format outgrew is refused with its remedy", which moves the store directory away, and `sync --reset`, which drops one account's collections.

#### Scenario: A downgrade under `--json`
- **GIVEN** a store whose `store_meta.format_version` names a later draft
- **WHEN** neverest runs `--json drain` on the account
- **THEN** it exits 1 printing a failure object whose `code` is `store-version`, the message asking for an upgrade and naming no reset

#### Scenario: A store root naming another program's database
- **GIVEN** a pimdir.db holding another program's tables
- **WHEN** neverest runs `--json drain` on the account
- **THEN** it exits 1 printing a failure object whose `code` is `store-foreign`, the database untouched

### Requirement: A sync prints its progress as events
Under `--json`, `neverest sync` SHALL print its progress as events before its result, one line each, each an object carrying `event`: `account-started` (`account`, `position`, `total`) before each account the run covers, the events after it being that account's; `opened` (`endpoint`) once an endpoint's first connection is open, `listed` (`endpoint`, `collections`) once it listed its collections; for a pair, `collection-started` (`source`, `target`, `collection`, `position`, `total`), `collection-progressed` (`source`, `target`, `collection`, `done`, `total`) and `collection-finished` (`source`, `target`, `collection`, `error`, `null` unless the collection stopped short); for a source with no target, `scan-progressed` and `download-progressed` (`source`, `done`, `total`), each printed as its phase starts. An endpoint SHALL be named as its account names it. A progress count SHALL be printed at most once per percent of its total, a new count of a collection's next phase starting again. Events SHALL report progress only: everything the run did or left SHALL stay in the result, the last line. The events SHALL be registered as `neverest-sync.events`.

Text mode SHALL print no event, the spinner on stderr being their text form; under `--json` no spinner SHALL be drawn.

#### Scenario: A frontend follows a long download
- GIVEN an account with no target whose store owes 2 bodies
- WHEN `neverest sync --json` downloads them
- THEN it prints `account-started`, then `download-progressed` with `done` 0, 1 and 2 of `total` 2, then the result

#### Scenario: A script reads the result alone
- GIVEN a sync piped to a script
- WHEN the run succeeds
- THEN the last line is the result, listing each account's report, without an `event` key, and every line before it carries one

### Requirement: A send that may have been delivered is never sent again
A `submit` that fails after the whole message was handed over and before a verdict came back SHALL be uncertain: over SMTP once the data's final `.` was written and flushed and no reply read; over Graph `sendMail` once the request was written whole and flushed and no answer read whole (none, one cut short, one that does not parse), or when 500, 502, 503 or 504 answered. A `sendMail` SHALL never be sent again on a dead connection; only a 429, which Graph answers before acting, SHALL send it again. An uncertain `submit` SHALL be parked with an error beginning `uncertain: `, SHALL be reported in `submitted` with `uncertain: true`, SHALL make the run that parked it exit 2, and SHALL never be sent again by any run.

Before the first byte of a message leaves, the run SHALL count the attempt in the store and SHALL write the intent's id to `submitting` beside `pimdir.db`, durably, clearing it once the outcome is recorded; a run finding the mark with that intent still pending SHALL park it as uncertain before sending anything, whichever account it belongs to. A store created SHALL start with no mark; a reset of one account SHALL leave it, the mark naming a row of the store.

A run that cannot record an outcome SHALL keep the mark and send nothing more.

#### Scenario: A server hanging up after the data
- **GIVEN** a queued `submit` and an SMTP server that reads the data and closes the connection unanswered
- **WHEN** the account syncs
- **THEN** the run exits 2, the row is parked under `uncertain: `, and the next run sends nothing

#### Scenario: A run killed while the server holds its verdict
- **GIVEN** a run handing over a queued `submit`
- **WHEN** it is killed before the server answers
- **THEN** the row reads pending with one attempt, and the next run parks it under `uncertain: ` without sending it

### Requirement: A check reports every endpoint
`neverest check` SHALL check every endpoint of every account it runs over whatever the others answer, and SHALL print, under each account, one row per endpoint, sorted by name: `source`, `backend` (its protocol table), `ok`, `code` and `error` (`null` when it answered), then `collections` (empty when it did not answer), `capabilities` and `smtp`. A credential that does not resolve SHALL fail its endpoint's row alone. When an endpoint or its send channel did not answer, the command SHALL print the report and exit 2, never the failure object.

A frontend adding an account shows what works and what does not in one run, rather than one failure per attempt.

#### Scenario: Two endpoints that do not answer
- GIVEN an account whose source and target both point at a closed port
- WHEN `neverest check --json` runs
- THEN it prints both rows under the account, each with `ok: false`, its `error` and no collection, and exits 2

### Requirement: A dry run works on a replica that shares the bodies
A dry run SHALL work on a throwaway replica of the pimdir store, so that no checkpoint advances and nothing reaches a server.

The replica SHALL be built beside the real store rather than under the temporary directory, so the two share a filesystem, and its bodies SHALL be hardlinked rather than copied. Bodies are content-addressed and therefore immutable, and a dry run neither rewrites nor purges one, so the replica needs the same bytes and not its own: a store whose blob tree is gigabytes SHALL cost a dry run some directory entries, never a read and a write of the whole tree, and never that much memory on a machine whose temporary directory is a tmpfs.

Everything the run writes to, the index above all, SHALL be copied. A file this rule misjudges SHALL be copied rather than shared, so being wrong costs a slower dry run and never a write reaching the real store. A link the filesystem refuses SHALL fall back to a copy.

The replica SHALL be removed however the run ends, an early return included, and a run SHALL clear what an earlier one left behind: a release build aborts on a panic and runs no destructor, so a leftover is a state to meet rather than one to rule out. Two runs of one store cannot race for it, the store lock being held for the whole run, and a run over several accounts makes each its own replica of the whole store, one after the other.

The preparation SHALL be logged with the time it took, and a blob tree that could not be shared SHALL say so, that being the slow case the sharing exists to avoid.

#### Scenario: A dry run over a mail account's store
- GIVEN an account whose store holds gigabytes of bodies
- WHEN `sync --dry-run` runs
- THEN the bodies are shared rather than copied, and the run starts without
reading and writing the whole tree

#### Scenario: A dry run that fails leaves nothing behind
- GIVEN a dry run whose credentials fail to resolve
- WHEN it returns
- THEN its replica is gone

### Requirement: A data command describes what it prints
A command returning data SHALL hand the printer one named `*Output` type deriving `Display` for the terminal, `Serialize` for `--json` and `JsonSchema` for the registry, and SHALL print it once. It SHALL NOT report data as a message: a message serialises as one prose string, so `--json` over one yields nothing a consumer can read, and several messages in one run yield several documents where a consumer expects one value.

`configure` returns data: the account it generated, under the name it derived, claiming no default (`default` stays in the shape every pimalaya wizard prints, always `false`). Its `Display` is the TOML document alone, which is what makes a redirected stdout a usable configuration file.

Every such output type SHALL be registered in the schema registry under its invocation path, the command path joined with hyphens and prefixed `neverest-`, as printed (the serialize contract, so a key always printed is required, `null` or not), and `neverest json-schema` SHALL print one schema to the standard output or write one file per command into a directory. A test SHALL run pimalaya-cli's registry check over the command tree: every runnable command described, bare `neverest` aside (it shows the help or runs the wizard `configure` describes), and every key naming a command, `neverest-error` aside, so a renamed subcommand cannot leave a schema nobody can ask for.

#### Scenario: A notifier reads the sync payload from its schema
- GIVEN a build of neverest
- WHEN `neverest json-schema neverest-sync` runs
- THEN it prints the schema of the sync result, its accounts' reports naming `conflicts` and `outstandingConflicts` among their fields

#### Scenario: A checked account is one JSON document
- GIVEN a configuration of two accounts
- WHEN `neverest check --json` runs
- THEN one document is printed, naming each account, its mode and every endpoint, answered or not, and the orphan accounts

## REMOVED Requirements

### Requirement: One account is one hub and one database
Replaced by "Accounts share one store": an account is one hub within a store it may share, `sync` runs over every account, and `-a` filters them.
