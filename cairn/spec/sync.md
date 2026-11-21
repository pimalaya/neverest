---
cairn: spec
capability: sync
status: current
---

# Sync

`neverest sync` reconciles the named sources of every account, or of those `-a` names, through the io-pimdir engine over the pimdir store the accounts share. It is sync-on-demand: one reconcile per invocation, no daemon.

The store and the engine follow pimdir storage-draft-15, sync-draft-15 and search-draft-15 (io-pimdir 0.19, vendoring the pimdir working tree of 2026-11-20; a store or a sync state of draft-14 is refused, never migrated, and an index of draft-14 rebuilt): "pimdir STORAGE", "pimdir SYNC" and "pimdir SEARCH" below cite those drafts.

### Requirement: An account holds named sources
An account SHALL hold a map of named sources (`sources.<name>`), each declaring exactly one remote backend, and SHALL require at least one. It MAY additionally hold a map of named targets (`targets.<name>`) on the same terms. Both map keys SHALL be the names its pimdir source ids are made from, so a name is what every binding it owns in the store is recorded against, and a rename SHALL be treated as a new source. One name SHALL NOT be both a source and a target.

An account SHALL NOT constrain its sources to one kind: mail, contacts and calendar sources may sit under one account, and their collections never meet (see the collection key requirement).

`left` and `right` SHALL NOT survive in any form, as keys, as aliases, or as source ids. A configuration carrying them SHALL be refused at load, naming `sources` and `targets` and the `one-way` flag that declares the direction they never could.

`collection.namespace` SHALL likewise be refused by name, naming `targets` and `one-way`. It expressed which sources met, which is now the arity, and never expressed which way, which is now `one-way`.

A store written before collection ids carried their namespace SHALL NOT be read. Neverest keeps its own state under the store's local/ (neverest.json, see "The sidecar holds each account" and "The owner's files live under local/"), and a store directory holding a database but no such file SHALL be refused as "A store laid out by an earlier neverest is refused" says.

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

### Requirement: The mode is the arity and two flags
An account SHALL declare `sources`, optionally `targets`, and the flags `one-way` and `retain`. Its mode SHALL be those, and SHALL NOT be inferred from any other property:

| sources | targets | `one-way` | behaviour |
|---|---|---|---|
| 1 | 1 | false | two-way mirror between the two remotes |
| 1 | 1 | true | source overwrites target |
| 1 | N | true | source overwrites each target |
| N | 0 | false | each source merges two-way with the local store |
| N | 0 | true | sources overwrite the local store, local edits discarded |

Every other combination SHALL be refused at load, naming the cell reached and the nearest legal one. One source with several targets and `one-way = false` SHALL be refused specifically: bidirectional propagation across more than two endpoints has no resolution order, and syncing them pairwise in configuration order would be a tiebreak the user never wrote.

In the no-targets cases the sources SHALL stay isolated from one another. An item held by one is never pushed to another, and a frontend unions them at display time.

#### Scenario: A plural typo cannot change the direction
- GIVEN an account declaring `sources` and `targets`
- WHEN a key is misspelled
- THEN the load fails naming the unknown field, rather than matching a different mode

#### Scenario: An illegal cell names its neighbour
- GIVEN an account declaring one source, two targets and no `one-way`
- WHEN it is loaded
- THEN it is refused, naming the missing `one-way = true` rather than picking an order

### Requirement: `one-way` declares authority, not silence
`one-way = true` SHALL make the `sources` side authoritative: a difference is resolved in its favour and the other side's change is discarded rather than merged, so no conflict is recorded and no divergence is reported. Each target SHALL sync under io-pimdir's `KeepLocal` (pimdir SYNC §5, sync-draft-15), the store being authoritative over it: a change made on the target alone is a divergence settled for the store, never pulled, so it never reaches the store's item, the source or another target, and the store's body is pushed back over it. The source SHALL sync under `PreferRemote`.

It SHALL NOT mean the other side goes unread. Every run SHALL still enumerate the target, or every item would be re-pushed on every run; the target's state decides what the run has left to do and never who wins. The documentation SHALL say "overwritten, not merged" in those words, because a user arriving from the two-way mode will otherwise expect a conflict report that cannot come.

#### Scenario: A one-way run copies the difference, not the collection
- GIVEN a one-way account whose previous run completed
- WHEN it runs again with nothing changed on the source
- THEN the target is enumerated, nothing is written, and the run is quiescent

### Requirement: pimdir is the ledger, and `retain` makes it a replica
The pimdir store SHALL NOT be nameable as a source or a target. It is the ledger in every mode, holding the item spine and the per-collection checkpoints that make enumeration incremental, and it SHALL be required even where no body is kept: a body-less IMAP to IMAP copy still needs to know what it has already copied and where each source's enumeration resumed.

`retain` SHALL declare whether the store additionally holds bodies and is readable by a frontend, independently of which endpoints the account names. With no targets, the store is the destination, so `retain` SHALL be true and an explicit `retain = false` SHALL be refused as syncing to nowhere. With targets declared it SHALL default to false, a configuration naming sources and targets having asked to copy between them rather than to fill a disk, and `retain = true` alongside targets SHALL be honoured rather than refused: migrating while keeping a local copy is a thing to want.

Whether a crossing is streamed from its holding source to the target or staged in the store and released SHALL be an internal choice, taken where the pairing allows, and SHALL NOT be visible in the configuration or in any report. A store that holds only the bodies that happened to cross SHALL NOT be a state a user can be in.

#### Scenario: A DAV pair keeps no more than an IMAP pair does
- GIVEN two CardDAV endpoints in a one-way account with `retain = false`
- WHEN the account is synced
- THEN the crossing is staged and released, and the store holds no body afterwards

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

### Requirement: A backend under the account is a source named after its protocol
A backend table written directly under the account (`imap`, `carddav`, `caldav`, `jmap`, `gmail`, `msgraph`, `gpeople`, `gcal`) SHALL be sugar for `sources.<key>.<key>`, the source taking the key as its name. The `msgraph` table SHALL be sugar for one source a domain it names, each named after the table and the domain: `msgraph-mail`, `msgraph-contact` and `msgraph-calendar`, each holding the table's session and that domain's table alone. The sugar SHALL produce a configuration indistinguishable from the expanded form, source id included, so expanding it by hand is a no-op on the store.

A key names a protocol, and where a protocol carries several domains a table under it names the domain: `carddav` and `caldav` are two keys over one adapter, `msgraph.mail`, `msgraph.contact` and `msgraph.calendar` three tables over one session. The domain is what the kind falls out of, so it SHALL be distinct per kind even when the transport is shared.

Declaring the same key both directly and under `sources` SHALL be a configuration error rather than a merge.

#### Scenario: Expanding the sugar changes nothing
- GIVEN an account written as `imap.server = "..."`
- WHEN it is rewritten as `sources.imap.imap.server = "..."`
- THEN the sync opens the same source id and reuses every existing binding

#### Scenario: Expanding the Graph table changes nothing
- GIVEN an account written as `msgraph.auth.token.raw = "..."` with `msgraph.mail = true` and `msgraph.contact = true`
- WHEN it is rewritten as `sources.msgraph-mail.msgraph.mail = true` and `sources.msgraph-contact.msgraph.contact = true`, each with the token
- THEN the sync opens the same two source ids and reuses every existing binding

### Requirement: The configuration refuses every key it does not read
The configuration SHALL refuse at load, naming it, every key it does not read, at every level: the document root, which holds `accounts` alone; an account; a `sources.<name>` or `targets.<name>` table; a backend table, written under a source or directly under the account; and every table nested in one (`tls`, `sasl`, `auth`, `collection`, `item`, `flag`, `smtp`, `store`, `mail`, `contact`, `calendar`, an `msgraph` domain table). A source table naming no backend table, or more than one, SHALL be refused at load, naming the backends it found.

A key an earlier neverest read and this one does not, `default` included, SHALL be refused as any unknown key is, by serde's `deny_unknown_fields`, with no sentence of its own and no pointer to a migration guide: v1 reads no 0.x configuration, a person writes a new one from the sample. No key SHALL be read under an old spelling: an alias either loads a configuration the person thinks is refused or refuses it under a name the person never wrote.

#### Scenario: A typo beside a source's backend
- GIVEN `sources.work.imap.server = "…"` and `sources.work.imap-typo = 1`
- WHEN any command loads the configuration
- THEN it fails naming the unknown field `imap-typo`, where the key used to be ignored

#### Scenario: Two backends in one source
- GIVEN a source naming an `imap` and a `jmap` table
- WHEN the configuration is loaded
- THEN it fails naming `imap` and `jmap`, where the second used to be dropped

#### Scenario: A key an earlier neverest read
- GIVEN an account carrying `folder.filter = "all"`, `imap.mailbox.create = false` or `default = true`
- WHEN the configuration is loaded
- THEN it fails naming the unknown field `folder`, `mailbox` or `default`

### Requirement: A collection is keyed by kind, namespace and name
A hub collection SHALL be keyed by the triple `(kind, namespace, name)`: the source's media type, its namespace, and the collection name the backend enumerates. The namespace SHALL be internal, derived from the source name, and SHALL NOT be configurable: it exists so that a CardDAV address book and a mailbox carrying the same name key apart, not so that a user can decide which sources meet. A target binds the namespace of the source it is paired with, which is what makes the two meet.

The id is spelled `<account>/<namespace>/<name>` with the kind on the collection row, the account keeping two accounts' collections apart in a shared store, and the account and namespace prefix SHALL be stripped back off before any call reaches a server, at one seam, so a backend only ever sees the name it gave. A report SHALL name a collection the way its server does, not the way the store keys it.

Every wire call SHALL pass through that seam, including the ones a hydration pool makes on its own connections rather than through the remote, and including a collection named as an argument rather than as the target: a move destination is a hub id like the collection it leaves. A cache keyed by collection SHALL keep the hub id as its key, the seam being the wire call and not the plan.

The source's `collection` table SHALL keep `create` and `delete` optional, defaulting to granting, unlike the `item` table which requires its pair to be declared in full. The table also carries `filter`, so it is declared for reasons that have nothing to do with permissions, and demanding a permission pair from someone writing a filter would be a trap.

### Requirement: Accounts share one store
Every account naming no `store.root` SHALL sync into one store, at the default root, and accounts naming one `store.root` SHALL share that store: one database and one blob directory, a body two accounts hold stored once. An account SHALL be one hub within the store, its collections grouped under it (see "A collection records the account that syncs it") and its sources never meeting another account's. `sync --source <name>` SHALL narrow which sources of each account run.

Sources in one account never meet, whatever command invoked them: an item one holds crosses to the account's targets, never to another source. Two genuinely independent replicas SHALL be two accounts.

A source whose sync fails SHALL be reported and SHALL NOT stop the others: they share nothing but the file the store lives in.

#### Scenario: Two accounts, one store
- **GIVEN** two accounts naming no `store.root`
- **WHEN** both are initialized and synced
- **THEN** one pimdir.db holds both, each collection grouped under its account

### Requirement: N sources over one store
Every configured source SHALL be one source handle of the account's pimdir store, keyed by its name. Cross-source propagation of items, flags and deletions falls out of each source's project/absorb against the shared hub, with no hand-rolled cross-merge and no special case for the two-source shape.

### Requirement: A flag on a kind other than mail stays in the store
A source syncing a kind other than `message/rfc822` SHALL be given no flag push right, whatever its `flag.update`: its backend carries no flags, so a flag the store alone holds on a card or a calendar item (`pimdir tag`, which reports it kept in the store only) is never pushed, never rejected, and kept local across runs. A run SHALL NOT report it, neither as a hunk of `itemPatch` nor as a forbidden write, nothing pushing it on any run. A mail source keeps `flag.update` as configured.

#### Scenario: A store-only flag on a card
- src/sync/driver/tests.rs `a_store_only_flag_on_a_card_stays_local`: two runs push nothing and the flag stays.
- src/sync/driver/tests.rs `a_store_only_flag_on_a_card_is_not_itemized`: the run reports no hunk and no forbidden write for it.

### Requirement: A source retains every body when the store is a replica
Where `retain` is true the sync SHALL hydrate every synced item to `Full`, the store being the app's offline copy. It SHALL pull before pushing so an edit the app staged locally stays pending and is reported rather than swallowed, and it SHALL open the store as the source it syncs so an app writing under the same id stages edits the sync pushes.

The item a hydration pass picks up SHALL be selected by the absence of a stored body, or of its base body under a local edit (pimdir SYNC §5, the edit pushing only once that base is read), not by its detail level. A remote content change drops the stale body while the hub keeps the level the item had reached, so a pass keyed on the level would leave an edited item bodiless for good.

### Requirement: Sync narrows by source
`sync --source <name>` SHALL narrow a run to the named sources. Narrowing no longer picks namespaces, there being none: an account is one mode, and a source is addressable on its own.

### Requirement: A send channel belongs to at most one source
At most one source per account SHALL declare `smtp`. Two or more SHALL be a configuration error, reported at load, rather than a silent tiebreak on source order. A source that sends by itself (Microsoft Graph mail through `sendMail`, Gmail through `messages.send`) needs none. Gmail MAY still take an `smtp` table, written under the account or the source, which then sends in its place; Graph mail sends through `sendMail` alone. A target SHALL NOT declare `smtp`, refused at load naming it: an account sends through a source, so a target's channel was checked and never used. A queued send naming a target as its sender SHALL park, saying a target sends nothing, rather than wait for ever.

Sending natively is a property of the source's **kind**, not of its transport: a Graph source of any other kind SHALL NOT be offered as the account's sender, and SHALL refuse an `smtp` table like any other non-mail source, so an account carrying two Graph sources still has at most one channel.

The account root MAY carry the `smtp` table when its mail backend is the direct-backend sugar, in which case it completes that one source; with no direct mail backend, or several, it SHALL be refused.

### Requirement: The send channel authenticates like the sync side
A source's `smtp` table SHALL be spelled as its `imap` one: a `server` that is either a bare authority, read as `smtps://<authority>`, or a full `smtp://` or `smtps://` URL; the same `tls` block and `starttls` switch; an optional `alpn` list; and an optional `sasl` table naming exactly one mechanism out of ANONYMOUS, LOGIN, PLAIN, OAUTHBEARER, XOAUTH2 and SCRAM-SHA-256.

The mechanism SHALL resolve through the same conversion the IMAP side uses, the GS2 host and port coming from the resolved submission URL. An omitted `sasl` table SHALL open an unauthenticated session, stopping after `EHLO` and sending no `AUTH`. The retired flat `login` and `password` keys SHALL be refused by name, never ignored, naming the `sasl` table that replaces them ("The configuration refuses every key it does not read"): a dropped credential would authenticate nothing against a server that requires it.

A build declaring the `smtp` feature SHALL enable io-smtp's `scram` feature, so a configured SCRAM-SHA-256 reaches the wire instead of being reported as an unsupported mechanism.

### Requirement: A collection filter belongs to the source
`collection.filter` SHALL be declared per source rather than per account, because an account may hold sources of several kinds and a mailbox include list means nothing to a contacts source. An account-level `collection` table SHALL be refused, naming its replacement.

Filters are consequently asymmetric: a collection may be synced on one source and skipped on another, which the documentation SHALL state, since the previous account-level filter guaranteed symmetry. A source and its target SHALL apply the source's filter (or the run's) to both, since they bind one set of hub collections and filtering them apart would read as a delete on the next pass; a target's own `collection.filter` SHALL narrow the pair further, applied to both sides alike, so a collection it leaves out is neither created on the target nor synced, and one source can feed different collections to different targets.

### Requirement: Bodies are content-addressed and deduped
An item body SHALL be stored once per content hash; an item present on both sources or in several collections is stored once and copied by reference. The link id SHALL be the identity pimdir STORAGE Annex A gives, with nothing prepended: the bare `Message-ID` for mail, the bare `UID` for a card. Only a kind's own fallback is marked (`alt:` over subject, date and sender; `hash:` over a card's body), that being the one case a prefix is for, a name no server has heard of; a real id cannot be mistaken for one, RFC 5322 `atext` admitting no colon before the `@`.

Where a kind resolves its link id at more than one tier (`message/rfc822`, from the header fields its source serves at `Meta` and from the parsed body at `Full`), the two derivations MUST produce the byte-identical string for the same item. In particular the date component SHALL be formatted the one canonical way, the **UTC instant** in RFC 3339 at seconds precision, so a message with no `Message-ID` does not link one way at `Meta` and another at `Full`. Kinds resolving at a single tier (the DAV kinds) cannot hit this class of bug.

### Requirement: Bodies are named by the store's own hash
The content hash naming an object SHALL come from the store handle (`PimdirStore::blobs`), which computes the algorithm `store_meta.hash_algo` records (pimdir STORAGE §5), never from a digest neverest defines. A consumer picking its own names bodies where no other reader of the same store looks, and it fails silently, as a dedup that never dedups.

### Requirement: Every item carries a per-kind sort key
The sync SHALL write a `sort_key` beside the summary of every item it summarises (pimdir STORAGE §9.3), derived by the same io-pimdir derivation and never parsed back out of the summary by the store. `message/rfc822` SHALL carry the `Date:` header normalised to RFC 3339 in UTC at seconds precision, so byte order is chronological order whatever offset the sender wrote; `text/vcard` SHALL carry the display name (`FN`) casefolded and trimmed. A kind resolving at two tiers SHALL derive the byte-identical key at both, on the same terms as its link id: a key that moved when the body arrived would re-sort the item on hydration. Content carrying nothing to derive from SHALL keep the empty key, which the store reads as unknown.

### Requirement: Sources are remote backends only
A sync source SHALL be a remote backend: IMAP, Microsoft Graph and Gmail for `message/rfc822`, CardDAV, Microsoft Graph and Google People for `text/vcard`, CalDAV, Microsoft Graph and Google Calendar for `text/calendar` (JMAP as its backend lands). Local file backends (m2dir, maildir, vdir) SHALL NOT be sync sources: the pimdir store is the local replica, so a local file store is redundant as a source and belongs on the import/export path, which neverest documents rather than syncing directly.

### Requirement: A first run meets the family's welcome
A command finding no configuration file, a bare `neverest` included, SHALL fail with pimalaya-config's missing-configuration error, exit 1: "No configuration found at <path>, start from the sample: <url>", the path being the first `-c` path when given, else the default one, and the URL config.sample.toml's. When the output is text and stderr is a terminal, a person SHALL meet pimalaya-cli's first-run welcome on stderr in its place, still exiting 1: the commands creating the file from the sample a package ships, or downloading it when none ships, then `neverest account check`. A script or a `--json` caller SHALL get the error.

With a configuration, a bare `neverest` SHALL print the help and exit 0, a configuration that fails to parse counting as one, its error left to the first command reading it. Under `--json` a bare `neverest` SHALL be a refused command line, through pimalaya-cli's `clap::meet_bare_invocation`: the failure object coded `usage-invalid`, exit 2, the configuration left unread, so a JSON reader never gets text. A command reading a configuration that fails to parse SHALL fail with that error rather than read it as absent. Neverest writes no configuration.

A configuration holding no account SHALL fail pointing at the sample the same way.

#### Scenario: A first run
- GIVEN no configuration file
- WHEN `neverest sync` runs from a script
- THEN it exits 1 naming the path it looked at and the sample, and writes nothing

#### Scenario: A first run at a terminal
- GIVEN no configuration file
- WHEN a person runs `neverest`
- THEN the welcome prints on stderr, giving the commands that create the file and `neverest account check`, and the run exits 1

### Requirement: The command line is the family's
`-c/--config` SHALL be pimalaya-cli's shared flag: one path per flag, never split, repeatable, the first file the base and the next ones deep-merged on top in order. No environment variable SHALL name the configuration (`NEVEREST_CONFIG` is not read). `-a/--account` SHALL stay repeatable, unlike the other Pimalaya CLIs, where it names the one account a command runs on: a neverest run covers several accounts.

`completion`, `manual` and `json-schema` SHALL come in that order, with pimalaya-cli's abouts, and SHALL require their names or `--dir`, as pimalaya-cli declares them: clap refuses a run with neither as a command line, exit 2, coded `usage-invalid` under `--json`. The help SHALL end with pimalaya-cli's footer. clap SHALL be built with the feature set the family uses (`derive`, `error-context`, `help`, `std`, `suggestions`, `usage`), so the help and errors are plain text read from no environment.

Neverest SHALL have no `repl` and no `mcp`: its commands are batch operations over every account and its store (sync, drain, check, init, index), which a session or an agent gains nothing from composing; reading and writing items is the frontends' job (himalaya, cardamum, calendula), each with its own `repl` and `mcp` over the store neverest keeps.

#### Scenario: Two configuration files
- GIVEN `-c public.toml -c private.toml`, each declaring one account
- WHEN `neverest sync --queue-only` runs
- THEN it runs over both accounts, the second file merged on top of the first

#### Scenario: A path holding a colon
- GIVEN `-c 'a:b.toml'`, a file that does not exist
- WHEN `neverest check` runs
- THEN it fails naming `a:b.toml` whole

### Requirement: The account group is the family's
Neverest SHALL carry pimalaya-cli's `account` group (pimalaya-cli cairn/spec/commands.md), over the accounts `-a` names, every one when it names none, as every neverest command:

- `account check` SHALL be the check the *A check …* requirements describe, and `check`, its name before the group, SHALL run it too, hidden from the help, its JSON schema kept under `neverest-check` beside `neverest-account-check`. It SHALL print pimalaya-cli's account check (pimalaya-cli cairn/spec/terminal.md, *The accounts and their checks print alike*), a list of accounts, each with its keys of its own after the shared ones (pimalaya-cli cairn/spec/output.md, *Shared nouns keep one shape*): `{accounts: [{account, backends, mode}], orphans}`, each row of `backends` `{backend, ok, code, error, tls, source, collections, capabilities, smtp}`, its shared keys pimalaya-cli's `BackendCheck`, the send channel's row being one. pimalaya-cli's `AccountCheckOutput` holds no key of a binary's own, so neverest prints its own types in its shape. A row's `backend` SHALL be the backend table the source writes, `msgraph` for every Graph domain, `source` the endpoint as the account names it. Its text SHALL print one block of fields per account, a blank line apart: `Account`, `Mode` (the mode's sentence), then one field per endpoint named as the account names it, `OK` or `FAIL (<error>)`, `, TLS` and the parts its table sets when it connects over TLS, then its collection count and its SMTP channel's outcome; the orphans after the blocks.
- `account list`, `ls` its visible alias as in every family binary, SHALL print pimalaya-cli's `AccountListOutput`, one `{name, default, backends, notBuilt}` row an account, sorted by name, reaching nothing. `backends` SHALL hold the backend tables the account writes, its sources' and its targets', sugar and named alike, each once, in the order `imap`, `carddav`, `caldav`, `jmap`, `gmail`, `msgraph`, `gpeople`, `gcal`, `smtp` (a send channel), those this build serves; `notBuilt` those it leaves out, JMAP always (it parses and no build opens it). A source's own name is the account's, not a backend, and reads in `account check`. `default` SHALL be `true` on every account: a command naming no `-a` runs over them all, no account being the default.

Neverest has no `account capability`: what each source can do reads in `account check`, from the configuration.

#### Scenario: Listing a mail account and a JMAP one
- GIVEN an account `a` with an `imap` table and an `smtp` table, and an account `b` with a `jmap` table
- WHEN `neverest --json account list` runs
- THEN it prints `a` with `backends: ["imap", "smtp"]` and `b` with `notBuilt: ["jmap"]`, both `default: true`, and exits 0 without opening anything

#### Scenario: A script calling `check`
- GIVEN a script running `neverest --json check`
- WHEN the account group lands
- THEN it prints the report `account check` prints, and `check` is not listed in the help

#### Scenario: Listing with `ls`
- GIVEN a configuration holding an account
- WHEN `neverest account ls` runs
- THEN it prints what `account list` prints and exits 0, and `account -h` lists `list` with `[alias: ls]`

### Requirement: Every remote backend is a cargo feature
Each remote SHALL be gated by a cargo feature named after the configuration table it builds: `imap` for the IMAP backend, `msgraph` for the Microsoft Graph backends, `carddav` for the CardDAV backend, `caldav` for the CalDAV backend, `gpeople` for the Google People backend, `gcal` for the Google Calendar backend, `gmail` for the Gmail backend, `smtp` for the SMTP submission channel.

Backends read from one table SHALL share its feature: `msgraph` covers Graph mail, contacts and calendars, all under the `msgraph` table. CardDAV and CalDAV share one adapter but are two tables, so they take one feature each, as cardamum's `carddav` and calendula's `caldav`: code serving both kinds SHALL build under either feature, and opening a source of one kind only under that kind's. A feature that merely aliases another is not introduced for an older spelling (no `dav`).

All of them SHALL ship in the default feature set, `msgraph` included again: it was held out under `msgraph-waits-for-its-domains` because a Graph account meant mail and nothing said so, and declaring the domain is what lifts that.

A missing backend SHALL surface at runtime, never at build time: every feature combination compiles, the configuration surface stays whole (every source config still parses), and an unavailable backend fails when the source is *opened*, coded `backend-not-built` and naming the cargo feature that builds it; a JMAP source, which no feature opens yet, fails saying so. A build with neither `smtp` nor `msgraph` has no send channel and SHALL warn rather than perform a submit intent. Each optional backend crate SHALL take its TLS provider from neverest's own `native-tls` / `rustls-aws` / `rustls-ring` / `vendored` features rather than pinning one.

#### Scenario: A CalDAV source in a CardDAV-only build
- **GIVEN** a build with `carddav` and without `caldav`, and an account with a `caldav` table
- **WHEN** the source is opened
- **THEN** it fails coded `backend-not-built`, naming the `caldav` backend and `--features caldav`

### Requirement: A backend owns its ALPN default
The `alpn` field of a source or channel config that has a backend crate SHALL be optional, and unset SHALL mean that crate's own default (io-imap's `["imap"]`, io-smtp's `["smtp"]`), resolved where the connection is opened. An explicit `[]` SHALL skip ALPN. Neverest SHALL NOT restate a backend's default in the config schema, so the default lives in exactly one place.

### Requirement: The pimdir store is the sole local copy
A message body SHALL be held locally exactly once: content-addressed in the pimdir blob store (where the derivation keeps bodies), deduped across sources, collections and accounts, and Neverest SHALL keep no parallel local copy in another format. Sync sources are remote backends only; an existing on-disk store (maildir/m2dir) is brought in through io-pimdir's conversion tooling, not synced as a source. The store is pimdir.db plus an `objects/` blob directory, shared by every account naming its root.

### Requirement: The default store root resolves on every supported platform
An account whose configuration names no `store.root` SHALL resolve its store under a per-platform base: the XDG state directory on Linux and the BSDs, `~/Library/Application Support` on macOS and iOS, `%LOCALAPPDATA%` on Windows. A platform SHALL NOT be left resolving through a mechanism it does not implement, since every account-touching verb reaches this.

Only the base SHALL vary. The store SHALL be `neverest/` under it on every platform, a directory rather than a file and one for every account naming no root, so one relative path addresses a store whoever wrote it. `store.root` SHALL override the whole resolution, for that account.

A roaming or cache location SHALL NOT be chosen where the platform distinguishes one: the store holds message bodies and a SQLite index, is the only local copy when the account retains bodies, and costs a full resync when dropped.

#### Scenario: A store resolves without a hand-written root
- GIVEN an account naming no `store.root`
- WHEN it is initialized on Linux, on macOS or on Windows
- THEN the store resolves under that platform's own base, at `neverest/`, and no directory is made for the account

### Requirement: The collection kind is declared
Each synced collection's media type SHALL be declared on the store from the backend (`Client::media_type`; `message/rfc822` for the mail backends), so the store is self-describing and ready to carry other item kinds.

### Requirement: A collection records the account that syncs it
Every store handle SHALL be opened for the account being synced, so each collection it writes is grouped under that account (pimdir STORAGE §9.2), and a reader reads one account through it (`pimdir.account` in himalaya, calendula and cardamum). Within the account, a collection is further keyed by its source's namespace, which is its name, so two sources of one kind are told apart without inferring it from the collection naming.

The store-wide ids SHALL carry the account (pimdir STORAGE §4.3, §9.2): a pimdir source id is `<account>/<source>` and a collection id `<account>/<source>/<name>`, so two accounts holding a source of one name, such as the `imap` sugar, neither write into one collection nor replace each other's declarations. An account name holding `/` SHALL be refused, naming it: `a` with `b/c` and `a/b` with `c` would spell one id. A report and an event SHALL name an endpoint as the configuration does, the account being named beside it.

#### Scenario: Two accounts with one source name
- **GIVEN** two accounts `work` and `home` sharing the store, each with an `imap` source
- **WHEN** both are synced
- **THEN** the store holds `work/imap/INBOX` under `work` and `home/imap/INBOX` under `home`, and declaring one leaves the other's capabilities in place

### Requirement: A store the format outgrew is refused with its remedy
A store opens under its own format version alone (pimdir STORAGE §4.2, §6): one any earlier neverest wrote, naming no `store_meta.format_version` or an earlier draft, is refused as `store-version` and never migrated. The refusal is the store's, so it SHALL fail the command before any account runs, naming the reinstall in the order that keeps its queued writes, which live only in that store: sync it with the neverest that wrote it until nothing is queued, move the store directory away, then rebuild it (`neverest init`, then a sync), rather than surfacing the raw refusal alone. The message SHALL never say to delete the store. What the old store held that no server holds (retained items, whatever was still queued) stays in the store moved away.

#### Scenario: A store an earlier neverest wrote
- **GIVEN** a store whose `store_meta.format_version` names `storage-draft-11`, as io-pimdir 0.13 wrote it
- **WHEN** neverest runs `--json sync --queue-only` on the account
- **THEN** it exits 1 with `code` `store-version`, the message naming the sync with the previous neverest until nothing is queued, the move of the store directory and the rebuild with `neverest init`, and no delete

### Requirement: The report shows remote-originated changes a pull applied
A sync SHALL report the remote-originated changes a pull applied to already-synced items (flag changes and removals), not only the local→remote pushes and the download plan. Because the pull applies them silently (the item reads `Clean` afterwards), they SHALL be recovered from the sync's per-item `events`: a `FlagsChanged` diffed against a pre-pull `handle → flags` snapshot into precise add/remove flag hunks, and a `Vanished` into a delete hunk. Each such hunk SHALL carry `direction: "pull"` and read `pull <flags> added to item <id> in <collection> from <side>`, `pull <flags> removed from ...` or `pull the deletion of item <id> in <collection> from <side>`, so it is not read as a change pushed to `side`; the `add-flags`, `remove-flags` and `delete` hunks a run pushes carry `direction: "push"`. A newly-pulled message (`Added`) is already reported by the pull plan (a `Fetch` hunk) and is not re-itemized.

### Requirement: A create is named by what delivers it
A `Created` placement SHALL be itemized by how the push delivers it (pimdir SYNC §5): an `Add` hunk, `add item <link id> in <collection> on <side>`, for a create with no origin on a side syncing alone, which is a compose or an import a frontend queued and the store uploads; a `Copy` hunk carrying `origin`, `copy item <link id> from <origin> to <collection> on <side>`, for a create whose origin names the collection the server copies it from, a move or a copy a frontend staged; and the cross-side `Copy` hunk, `copy item <link id> in <collection> from <other> to <side>`, for a body the other side holds. Reading every create as a crossing named a single-source account's own compose "from imap to imap".

A create a relocation delivers (a move whose remove carries its destination, pimdir SYNC §3, §5) SHALL be itemized beside that remove, in the collection the item leaves, while it is still pending there: collections push in turn, so the remove pushed first delivers the create before the collection it enters is itemized. A dry run pushes nothing, so there each half SHALL be itemized by its own collection. Either way a move reads as its two rows, once each.

#### Scenario: A card moved between address books
- src/sync/driver/tests.rs `a_moved_card_is_itemized_as_both_its_halves`: the address book it leaves itemizes the delete and the add of a real run, the delete alone in a dry run.

### Requirement: A dry run works on a replica that shares the bodies
A dry run SHALL work on a throwaway replica of the pimdir store, so that no checkpoint advances and nothing reaches a server.

The replica SHALL be built beside the real store rather than under the temporary directory, so the two share a filesystem, and its bodies SHALL be hardlinked rather than copied. Bodies are content-addressed and therefore immutable, and a dry run neither rewrites nor purges one, so the replica needs the same bytes and not its own: a store whose blob tree is gigabytes SHALL cost a dry run some directory entries, never a read and a write of the whole tree, and never that much memory on a machine whose temporary directory is a tmpfs.

Everything the run writes to, the index above all, SHALL be copied. A file this rule misjudges SHALL be copied rather than shared, so being wrong costs a slower dry run and never a write reaching the real store. A link the filesystem refuses SHALL fall back to a copy.

pimdir.db SHALL be copied as one consistent snapshot through `VACUUM INTO`, never as a file with its WAL: producers write the store under no lock neverest holds, so a file copy may tear between two of their commits (pimdir STORAGE §8). The staging lock SHALL be held shared from before the snapshot until the last body is linked, waiting out a collector, so no body the copied database names is unlinked under the copy. No lock file of the store root SHALL be copied: a lock file holds no state, and the replica's owner creates its own.

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

#### Scenario: A dry run while a collector runs
- GIVEN a store whose staging lock another process holds exclusively
- WHEN `sync --dry-run` prepares its replica
- THEN it waits for the lock, then takes the database as one snapshot, copying no lock file

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

### Requirement: Credentials are resolved once per run
A run SHALL resolve every configured secret once, up front, into a runtime account holding the values themselves, and SHALL open every connection from that account. Nothing below that seam may spawn a process to authenticate: a second connection to a side, whether opened eagerly for the connection budget or lazily for a concurrent fetch, SHALL cost a handshake and no credential read.

Resolution SHALL memoize the commands it spawns, keyed on the command as the configuration wrote it, so a configuration naming one password entry from several tables SHALL spawn it once per run rather than once per table.

The key SHALL be the configured shape itself, a shell line or a program with its arguments, compared as written and never across the two: a shell line and the argv spelling that runs it through the platform shell SHALL resolve on their own. Reading one as the other means guessing what a configuration meant, and handing a credential to a field that did not ask for it is the failure that guess would cause.

A credential that fails to resolve SHALL fail its endpoint, not the account: the error SHALL be raised when that endpoint is read, and reported where a source that could not be opened is already reported, so the account's other sources still sync.

The wait SHALL be visible. A credential store answers in seconds when its agent is locked, so the resolution SHALL be reported while it runs, and each spawned command SHALL be logged with the time it took. Neither the resolved value nor the command arguments SHALL be logged, a command line being free to carry the secret itself.

An account is resolved once and never re-read within a run. This is exact for a one-shot sync and SHALL NOT be relied on by a long-lived caller, which would resolve a new account rather than refresh this one.

#### Scenario: One entry named by four tables
- GIVEN an account whose `imap`, `smtp`, `carddav` and `caldav` tables name one
`password.command`
- WHEN a run resolves it
- THEN the command runs once, whatever connection budget the sources carry

### Requirement: Sync is one-shot
A `sync` run SHALL perform a bounded number of reconcile passes until quiescent and exit. Watch and real-time triggers are out of scope: watching belongs to carillon (carillon-core and its frontends), whose content-free ring kicks a sync run through its cmd consumer.

### Requirement: Neverest is the store's sole owner and drains the queue first
Neverest SHALL be the only process writing a pimdir store; frontends read it and enqueue mutations through io-pimdir's producer queue. A sync run SHALL drain the store's queue once, at its start, after declaring the sources of every account it runs over and before reading any credential or opening any endpoint, so a run that cannot reach its servers still applies what was queued: exactly-once apply-and-delete per action, permanently bad actions parked, transient failures left queued in order. The store applies each action as the source syncing its collection (pimdir STORAGE §15.2), so the drain is the store's and not a source's, every account's queue included. `--declare-only` SHALL NOT drain; `--intents-only` SHALL; a dry run drains each account's replica. The applied counts SHALL be logged (info when nonzero) and reported under the account owning the collection; a row on a collection of an account the run does not cover is applied and reported by no account, its producer reading its receipt. Two steps of a run stage rows of their own and drain again to apply them, the conflict merge and the copy of a sent message: that drain is the store's too, so a staged row SHALL be judged by its own status, never by the drain's totals, and the account's other rows it applies SHALL be reported as the start's drain reports them.

Every parked action of an account's collections SHALL surface in that account's report until repaired, and SHALL surface **once per run**, read after every source of the account has run, in a dry run as much as in a real one.

A queued `move` of an item in conflict, the item's or that of the binding it runs through, SHALL be parked, its error naming the diverging body, nothing staged in the collection it leaves nor in the one it names (pimdir SYNC §7, STORAGE §15.2, §15.3): a `move` states no resolution, and its tombstone would leave the decision standing where no listing shows it. The item stays listed where it stood and stays counted among the conflicts waiting. `neverest conflict resolve` settles the conflict first; the parked row then leaves by its producer, retried or cancelled, neverest retrying none. A `copy` meets no conflict.

The subsequent sync of a drained collection pushes the resulting dirty state. An action kind the drain cannot apply itself (a capability-bound intent such as `submit`) SHALL be left pending for the phase that can, never parked.

#### Scenario: A sync that cannot connect
- **GIVEN** an event queued on a calendar whose server is unreachable
- **WHEN** the account syncs and exits 4
- **THEN** the event is in the store, with its `seq`

#### Scenario: A move queued over a card waiting for a decision
- **GIVEN** a card in conflict and a `move` of it queued by a frontend
- **WHEN** the queue is drained
- **THEN** nothing is applied, the report lists the `move` parked with the diverging body's hash in its error, the card is still in its address book and the report still counts one item waiting for a decision (src/sync/driver/tests.rs `a_queued_move_over_a_conflict_parks_and_the_decision_still_waits`)

### Requirement: `sync` initializes, queues or indexes on a flag
`sync` SHALL take three flags, and `drain` and `index` SHALL be no commands (D6 of the-v1-review):

- `--init` SHALL initialize each account the run covers that is not yet, as `init` does (opening every endpoint, adding the account to the store, creating the store when there is none), then sync. The result SHALL list them under `initialized`, empty without the flag or when every account was initialized; an account whose initialization failed SHALL be reported with that failure in `accounts` and not synced. Without `--init`, an uninitialized account SHALL fail as before, the message naming both `neverest init -a <name>` and `neverest sync --init -a <name>`. Under `--dry-run` the accounts are still initialized, which moves no item, and the sync is only planned. `init` stays the command that initializes alone.
- `--queue-only` SHALL be the former `drain` (*A drain applies the queue without syncing*), its rows printed under the sync result's `queue`, one an account.
- `--index-only` SHALL be the former `index`, its stores printed under the sync result's `index`, one a store, in a build with `search`.

The two `-only` flags SHALL exclude each other, `--init` and every flag of a full sync, refused by clap as a command line (exit 2). The sync result SHALL print every key whichever flag ran: `initialized`, `accounts`, `queue` and `index`, empty when not filled.

#### Scenario: A first sync
- GIVEN an account no `init` ever ran over
- WHEN `neverest sync --init` runs
- THEN the account is initialized, then synced, and `initialized` names it

#### Scenario: Two only flags
- GIVEN any configuration
- WHEN `neverest sync --queue-only --index-only` runs
- THEN it exits 2 and touches nothing

### Requirement: A drain applies the queue without syncing
`neverest sync --queue-only` SHALL apply the queued actions as a sync's drain does, after declaring the sources of every account it runs over as a sync does before its drain (the configuration and the sidecar), reading no credential and opening no endpoint. The drain is the store's: it applies every account's queue, once a store, and each account's entry SHALL report the rows of its own collections it applied, with the `seq` of the item an `add` created, every row the store holds parked on its collections, this drain's or an earlier one's, as a sync reports them, and how many it left for a sync. It SHALL NOT wait for the store: while a run holds `sync.lock` it SHALL report the accounts of that store `busy` and exit 4, the queue untouched. A drain reporting a parked row SHALL exit 3, as a sync does; a store step failing, a queue row whose payload does not decode among them, SHALL fail every account of the store alone, its code kept (`action-malformed`), which exits 4, never the command.

### Requirement: A run holds the store lock, waiting bounded
A run SHALL hold an advisory sync.lock under local/ of the **actual** store directory (honouring `store.root`) for its whole length, whatever accounts it covers, taken once for each store it covers, in path order, before any account runs. A second run SHALL wait for the holder up to a bounded timeout (60 s) and then report every account of that store `busy` and exit 4, so cron ticks and connector-triggered scoped runs serialize instead of failing or corrupting.

The lock is the store's, as pimdir's owner lock is (STORAGE §8: one owner process per store), so two accounts of one store never sync at once. Accounts needing independent schedules SHALL give themselves their own `store.root`.

#### Scenario: Another account of a busy store
- **GIVEN** a run holding the store's lock
- **WHEN** `neverest sync --json -a <another account of it>` runs
- **THEN** it waits the bounded time, then prints that account `busy` and exits 4, its store untouched

### Requirement: An IMAP handle-space change rebuilds the collection and bumps its generation
An IMAP listing SHALL compare the `UIDVALIDITY` its `SELECT` answers with the one the store's handles were bound under: the stored checkpoint's, else, while no round has closed, the open round's. On a mismatch it SHALL fail the enumeration and name a rekey (pimdir SYNC §4, §8), whatever it was asked for (a delta, a head, a round's first page or a resumed page), never answering a round of the new UIDs: a sync reads such a round as every member gone and every new one arrived, merges flags onto whatever an old UID now names, and tombstones every message on another endpoint.

The driver SHALL then run io-pimdir's rekey on the collection before syncing it again, carrying cached bodies, summaries and pending state over by link id. The rebuild batch drops every old handle as `Rekeyed`, which the store reads as the rebuild and answers by bumping `collections.generation` in the transaction applying it (pimdir SYNC §8), so a frontend derives its epoch from the store alone. A member no old placement resolves to is reported `Added` by the run, as the sync naming it would (io-pimdir's `PimdirRekeyReport::events`). A rekey that fails leaves the stored checkpoint as it was, so the next run meets the moved `UIDVALIDITY` again and rekeys then. Ordinary syncs and full resyncs never bump. Graph sources never rebuild: Graph message ids survive a delta reset.

#### Scenario: A mailbox renumbered between two runs
- **GIVEN** a mailbox synced under one `UIDVALIDITY`, one of its messages carrying a flag staged in the store
- **WHEN** the server renumbers the mailbox and the account is synced
- **THEN** the message is bound under its new UID with the staged flag, nothing is dropped as deleted, and the flag is pushed to the new UID, and a message the store did not hold is reported added

### Requirement: Microsoft Graph is a first-class source
An `msgraph` source SHALL open protocol-direct over io-msgraph (never through a frozen aggregator). Graph carries several PIM domains behind one protocol, one host and one credential, so one `msgraph` table SHALL hold the session (`user-id`, `tls`, `alpn`, `auth`, `proxy`) and name the domains it syncs by their tables, as carillon, cardamum and calendula read one `msgraph` table: `msgraph.mail` the mail folders, `msgraph.contact` the address book, `msgraph.calendar` the calendars. A domain table SHALL hold that domain's source keys (`collection`, `flag`, `item`, `pool-size`), or be written `true` to take their defaults, `false` reading as absent. A table naming no domain SHALL be refused at load, naming `msgraph.mail = true`. Each domain SHALL be a source of its own: under the account as the sugar names them, under `sources.<name>` or `targets.<name>` exactly one, a named table naming two or more being refused at load. One adapter SHALL serve every domain, parameterised by which it speaks, as one adapter serves both DAV protocols. A value naming the kind (`kind = "…"`) SHALL NOT be introduced for this or any other backend.

For **calendars** (`msgraph.calendar`): the user's calendars keyed by id and named by name, events as `text/calendar` through io-msgraph's `ical` projection. Graph's event delta only runs over a calendar view, a time window, and an event leaving a sliding window would read as deleted and be deleted on every other source, so the enumeration SHALL list each calendar's lone events and series masters in full every run, with no window, the master's `changeKey` as the revision and removals reported by absence; an event Graph repeats across `@odata.nextLink` pages SHALL be enumerated once, and one id at two revisions SHALL fail the enumeration. A series SHALL be one item, read with its exceptions from the instances of its own date range, bounded by its range type (a `numbered` range sized from its occurrence count, an open-ended one five years either side of today, as "An open Graph series older than its window says so" reports), never by the `0001-01-01` `endDate` Graph fills the other types with, and listed in windows of at most five years, the longest Graph accepts, its cancelled occurrences as EXDATEs. The UID SHALL ride the projection's stash extended property, a create whose UID did not survive being deleted again and refused; an event with no stashed UID SHALL be keyed by Graph's `iCalUId`, unwrapped when it is a global object id wrapping the organizer's UID (an invitation Exchange took in by mail, the `vCal-Uid` form), so the event keeps the UID the organizer and the other attendees know; an update or delete SHALL be refused when the master's `changeKey` moved. A series SHALL be written whole, as "A series is pushed whole or not at all" says, and a floating time anchored, as "A floating time is anchored before Google or Graph takes it" says; an edit Graph makes to one occurrence alone is seen once the master's `changeKey` moves.

For **mail** (`msgraph.mail`): folders listed at every depth, each child folder walked in turn (`Parent/Child/Grandchild` naming, a `/` in a folder's own display name written `%2F` and a `%` written `%25`, so a top-level `A/B` and the child `B` of `A` stay two collections; the collection's name is the path with its escapes undone), enumeration through the messages delta query carrying the `@odata.deltaLink` as the engine's opaque checkpoint (HTTP 410 = expired link, restarting a fresh full round; any other failure surfaces), the `Meta` tier read by one summary get a message (`mid:`/`alt:` link ids, meta v1), the `Full` tier from the raw MIME content streamed into the blob store. Flags map to the IANA wire spellings (`isRead` = `\Seen`, a flagged follow-up = `\Flagged`, `isDraft` = `\Draft`). Push scope is honest: flag changes push through `message_update`, deletes through `message_delete`, moves and copies as the requirement "Graph mail moves, copies and adds drafts" says; an append lands in Drafts only, and content updates are rejected and documented.

Auth SHALL be a bearer access token only, resolved through the standard secret-command idiom (`auth.token.raw` / `auth.token.command`) once per run with every other credential; neverest SHALL NOT run any OAuth flow itself (no device sign-in, no client credentials, no token persistence): acquiring and refreshing the token is delegated to an external command, typically ortie. The token SHALL carry the scopes the declared domains need, which config.sample.toml names per domain so the first run does not fail on a bare 403. No token is ever logged.

### Requirement: A Graph contacts source syncs the address book
An `msgraph` table MAY name the `contact` domain, whose items are `text/vcard` cards and whose collections are the user's contact folders, listed two levels deep, keyed by folder id and named by display name. The default Contacts folder, which the folders endpoint does not list, SHALL be the collection `contacts`, addressed by omitting the folder segment.

It SHALL take the session of its `msgraph` table (`user-id`, `tls`, `alpn`, `auth`, `proxy`), and SHALL carry no send channel: submission is a mail capability, so an `smtp` table beside it is refused before any connection is made, and it SHALL NOT be picked as the account's native sender however the sources are ordered.

Enumeration SHALL be the contacts delta query, carrying the `@odata.deltaLink` as the engine's opaque checkpoint under the same rules mail follows: HTTP 410 restarts a fresh full round, any other failure surfaces, and handle identity survives a reset. The delta SHALL select only the handle and `changeKey`.

Contacts are **mutable content**: `change_key` SHALL be reported as the revision on every enumerate and fetch, an update SHALL be a conditional write against it, and a refused write SHALL be reported as rejected so the engine re-merges. Unlike mail, a Graph contacts source pushes creates and updates as well as deletes.

The body SHALL be read per contact with the `$expand` clause returning the stash (`MSGRAPH_CONTACT_STASH_EXPAND`), Graph omitting extended properties otherwise, and SHALL NOT be projected from a delta row: whether the contacts delta honours `$expand` is undocumented, and a row without the stash would read the card under a minted identity. The cost is one request per changed contact.

Graph has no conditional write for contacts, so an update or delete carrying an `If-Match` SHALL first read the contact and be refused when its `changeKey` moved, the next run enumerating the server edit.

Flags SHALL be reported known-empty, Graph contacts having no flag concept.

#### Scenario: An address book syncs, follows a server edit and pushes one back
- GIVEN a Graph tenant holding two contacts in the default Contacts folder
- WHEN the account is synced, one contact is edited on the server, another edited in the store, and it is synced again
- THEN the store holds both cards keyed by their vCard `UID`, follows the server edit, and the store edit reaches Graph as a conditional update

### Requirement: A Graph contact keeps the identity it arrived with
A Graph contact carries no vCard `UID` of its own, so the `UID` SHALL ride the stash extended property the projection already writes, and SHALL be read back from it: that value is the card's `UID` and therefore its link id. A card written by neverest keeps the identity it arrived with, so `One identity is one item across an account's endpoints` holds across a Graph endpoint and a DAV one.

A contact whose stash carries no `UID` is one Graph itself created, or one an earlier client wrote. Only then SHALL a `UID` be minted, from the Graph id; it is stored the next time the contact is written. The identity SHALL NOT fall back to the content hash, which would repoint it on every field edit. Graph reassigns a contact's id when the contact moves between folders, so such a contact, never written by neverest, reads as removed and re-added after a move.

A create SHALL read the contact back with its stash, and one whose `UID` the server did not keep SHALL be deleted again and the write refused, never completed under a minted identity.

#### Scenario: A card crosses from CardDAV to Graph and back unchanged
- GIVEN a card with `UID:urn:uuid:…` on a CardDAV endpoint and a Graph contacts endpoint in the same account
- WHEN the account is synced, then synced again
- THEN one item exists under that `UID`, the Graph contact carries it as its stored identity, and the second run finds nothing to do

### Requirement: The Graph projection belongs to the protocol crate
The mapping between a Graph contact and a vCard SHALL live in io-msgraph (its `vcard` feature), beside the resource it projects, and SHALL NOT be written a second time in a consumer. It projects both ways and produces a delta against a base card, and every line the vCard carries that Graph has no slot for SHALL survive on the server through the stash remainder rather than being dropped or folded into another slot.

Two copies of a projection drift silently and lose data at the seam between them, which is the failure this rules out rather than mitigates.

### Requirement: A queued submission is a `submit` queue intent
Neverest SHALL NOT reserve a collection for queued sends. Submission is a **mail** capability: a `submit` intent belongs to a `message/rfc822` account, and an `smtp` channel declared on a source of any other kind SHALL be refused before any connection is made, rather than silently ignored. A submission SHALL be a **queue action** whose kind (`submit`) pimdir defines (STORAGE Annex B.2) and io-pimdir types (`PimdirSubmit`), carrying a versioned JSON payload; which kinds an owner can perform is the owner's business. An owner that does not recognise a kind, or recognises it but lacks the capability, SHALL **skip** the row, leaving it pending, never parking it (parking means permanently unappliable) and never blocking later actions of that collection.

The intent's payload SHALL be `v: 1` JSON, read by io-pimdir's `PimdirSubmit` whatever the order of its keys, carrying `object` (the body hash, required, and the hash the row pins), `from` (empty means the null reverse path), `rcpts` (at least one), and an optional `subject`, `source` and `copy`. A payload `PimdirSubmit` refuses, or one naming another body than the row pins, SHALL park as undecodable, but one stating a version neverest does not know, which stays pending ("An intent stating an unknown version stays pending"). The body SHALL be written durably before the enqueue, so the queue row pins it (queued bodies are pinned, and no GC can sweep it between the enqueue and the send); it belongs to no collection. The intent SHALL anchor on whatever collection the producer chose: neverest scans every collection's pending actions, so there is no anchor rule.

Neverest SHALL perform each pending intent through the one source offering a send channel: its own `smtp` table, else its native send (the Graph `sendMail` action, which files the message in Sent itself, or Gmail's `messages.send`, which files it in SENT, replacing the body's `Message-ID` with its own where Gmail's SMTP server keeps it, over a connection opened for the sends so none goes out over one Gmail closed while it idled), sources walked in the order the account declares them. At most one source may declare an `smtp` table, so the only pick that order decides is between a declared channel and a natively-sending source. On success the row SHALL be acknowledged with its receipt, releasing the body's pin. A **transient** failure, which certainly delivered nothing (an SMTP 4xx, an SMTP 5xx refusing for a sending rate or quota as "A quota refusal keeps the send queued" says, a Graph 401, 408 or 429, a Gmail 401, 403, 408 or 429, a transport error before the message went whole), SHALL leave the row pending; a **permanent** one (any other SMTP 5xx, a Graph 400, 403, 404, 413, 422 or other 4xx, an undecodable payload, a missing body) SHALL park it with its error; an **unconfirmed** one SHALL park it as "A send that may have been delivered is never sent again" says. A refused recipient or a refused `DATA` command SHALL leave the SMTP transaction reset before the next message, one `RSET` sent by io-smtp; a refused `MAIL FROM` opens no transaction and a refused body ends its own, so neither is followed by a reset. A refusal for a sending rate or quota, a `421` reply, by which the server closes the session (RFC 5321 §4.2.2), or a failure with no reply or status, SHALL leave the rest of the run's intents unattempted, pending. An envelope address io-smtp refuses, one that is no mailbox or holds a line break, SHALL fail its intent as permanent before anything is sent. A build with no send channel (none of `smtp`, `msgraph` and `gmail`) SHALL skip submit intents and warn, never park them. Message content is never logged.

The SMTP channel SHALL remove the body's `Bcc` field before `DATA` (RFC 5322 §3.6.3): a producer may stage the body with it, the payload's `rcpts` alone carrying those recipients to the server. The Graph and Gmail channels hand the field to `sendMail` or `messages.send`, which derive the recipients from it and do not deliver it; a `rcpts` address no header names is not reached through them.

Submission is **at-most-once** on its own: a message that may have left is parked, never sent again by a run, and only a person, through the producer, sends it again.

An SMTP channel that does not open SHALL be reported: a server that cannot be reached, or a session cut before its credentials were answered, as an `unreached` error naming `<source>.smtp`, its intents pending and unattempted; credentials the server refused, by each queued `submit` failing as a `submitted` error with the reason, not parked, not attempted. Either makes the run incomplete.

### Requirement: A quota refusal keeps the send queued
An SMTP rejection SHALL be classified by its reply code and its reply text. A 5xx refusing for a sending rate or quota SHALL be transient: the row SHALL stay pending, reported as a `submitted` error, `parked: false`, and the run's remaining sends SHALL stay pending, unattempted, each reported as not attempted. Such a refusal is a 5xx whose text opens with an enhanced status code (RFC 3463) meaning nothing else, `5.4.5` (congestion, which RFC 3463 calls a persistent transient error), `5.1.90`, `5.7.232`, `5.7.233` or `5.7.236` (codes outside the registry naming a daily sending limit), or one whose text says so under a code of other meanings, matched without case: `SubmissionQuotaExceeded`, `submission quota`, `sending limit exceeded`, `rate limited`, `daily limit for message recipients`. Any other 5xx SHALL stay permanent, a sender blocked until a person releases it (`5.1.8`, `5.7.501` to `5.7.503`, `5.7.70x`) and a sender's own full mailbox (`5.2.2` with `QuotaExceededException`) among them.

A provider refusing a send for its daily limit has not refused the message: the next day's run sends it, and parking it would leave it to a person for nothing. The set stays narrow because a misread refusal holds the whole queue, each run reporting it (exit 4) until a person acts.

#### Scenario: A daily limit holds the send
- **GIVEN** two queued `submit` intents and an SMTP server answering the first one's data with `550 5.4.5 Daily user sending limit exceeded`
- **WHEN** the account syncs
- **THEN** both rows stay queued, `errors` names the first as a `submitted` error, `parked: false`, and the second as not attempted, and the second never reaches the server

#### Scenario: An unknown recipient still parks
- **GIVEN** a queued `submit` intent and an SMTP server answering its `RCPT TO` with `550 5.1.1 User unknown`
- **WHEN** the account syncs
- **THEN** the row is parked with its error

### Requirement: A run reclaims retained items on a schedule
The store retains an item rather than deleting it when its last binding vanishes, so reclaiming is the client's schedule. An account SHALL configure it as `store.purge-after`, a duration as pimalaya-config reads one: an integer of seconds, or a number plus `s`/`m`/`h`/`d`/`w` (`"90d"`), a day being 86400 seconds and months and years refused. **Unset SHALL mean never purge**; `0` or `"0"` SHALL purge immediately, reproducing a terminal delete. There SHALL be no boolean beside it: the delay is the switch.

A sync run SHALL sweep **after** the sync and before the report is finalised, on both the pair and the solo paths, never in a dry run, purging every retained item of the account's own collections whose `retained_at` precedes `now - purge-after` (RFC 3339, the shape the store stamps), every expired item of the account in one transaction (`purge_items`, never one transaction an item), never another account's: the store's own sweep (`purge_retained_before`) is store-wide, and one account's delay would decide for every other. Sweeping after the sync means an item this run retired starts its delay now rather than being reclaimed by the run that retired it. The sweep SHALL warn rather than fail the run, as the send channel does, and `sync --no-purge` SHALL skip it. The report SHALL carry what was reclaimed (items and bytes) in both the text and `--json` output.

A read-only source (`<protocol>.item.delete = false`, `collection.delete = false`) with no purge delay is therefore a backup: a remote expunge retires the local row without losing the item or its body.

#### Scenario: One account's delay
- **GIVEN** a store shared by an account with `store.purge-after = "0"` and one with none, each holding a retained item
- **WHEN** the first account's retention sweep runs
- **THEN** its retained item is purged, and the other account's stays

#### Scenario: One write a sweep
- **GIVEN** an account holding expired retained items in three collections
- **WHEN** its retention sweep runs
- **THEN** the three are purged in one committed transaction

### Requirement: The checkpoint is opaque to the shared client seam
The backend-neutral enumeration seam SHALL carry the incremental-sync cursor as opaque checkpoint bytes and string member handles: the IMAP adapter encodes its `(UIDVALIDITY, HIGHESTMODSEQ)` pair, the Graph adapter its delta link, and the engine stores whichever bytes the source produced. (Supersedes the IMAP-shaped `(u32, u64)` cursor on the shared seam; the QRESYNC behaviour itself is unchanged and stays specified under "IMAP enumeration is incremental".)

### Requirement: Text mode prints plain lines
In text mode a command SHALL print where it is as plain lines on stdout, one a step, never per item, each flushed as it comes, then its result: no colour, no `\r`, no cursor move, no spinner, and no dependency drawing one (D4 of the-v1-review). Text mode is read by a person, so the progress and the result share stdout, and stderr stays the logs'; a script reads `--json`, whose last line is the result alone (owner, 2026-11-17, closing N6). A full `sync`, the one long-lived command, SHALL print its lines to a pipe too, as pimalaya-cli's output spec asks of a long-lived command; `account check`, `init`, `sync --queue-only` and `sync --index-only` end on their own once their step is done, and SHALL print theirs only when stdout is a terminal, so their redirected output holds their result alone (owner, 2026-11-20, narrowing N6). Under `--json` no such line SHALL be printed, the events and the result saying the same. `check`, `init` and the credential resolution SHALL print one line per step (`Checked source imap (3 collections)`, `Checked source imap (1 collection)`, `Source imap failed (…)`, `Initialized imap of account work`, `Created the store`, `Resolved credentials`).

Every text a command prints, result, notice or failure, SHALL pass through pimalaya-cli's sanitizer line by line, its line breaks and tabs kept: it holds what servers and other clients wrote (collection names, ids, flags, bodies, refusal reasons), and a control or bidi character there could drive the terminal or reorder what a person reads. Under `--json` every string SHALL stay exact.

#### Scenario: A text-mode sync
- GIVEN an initialized account `work`
- WHEN `neverest sync` runs without `--json`
- THEN stdout holds `[1/1] Syncing account work`, then the result, and no escape byte, and stderr holds no progress line

#### Scenario: A check with a source down, its output in a file
- **GIVEN** an account whose IMAP source points at a closed port
- **WHEN** `neverest check >file` runs
- **THEN** the file holds the check's result alone, `imap:     FAIL (…)` under `Account:  <name>`, no progress line and no escape sequence

### Requirement: IMAP enumeration is incremental (QRESYNC)
Enumeration SHALL carry a per-mailbox checkpoint `(UIDVALIDITY, HIGHESTMODSEQ)`. On a server that enabled QRESYNC on connect, its `ENABLED` naming it (RFC 5161; advertising it is not enough, iCloud answering `ENABLE` a bare `OK` and then reporting no change to a QRESYNC `SELECT`), with a checkpoint whose UIDVALIDITY still matches, a delta SHALL be a QRESYNC `SELECT (QRESYNC (uidvalidity highestmodseq))`: only the messages changed since the modseq, the meta read for the ones the store does not bind, plus the vanished UIDs, issuing **no FETCH when nothing changed**. Without a usable checkpoint (first sync, malformed bytes) or on a server that did not enable QRESYNC it SHALL answer a round, page by page, so an arrival, a flag change and an expunge are never stepped over. A checkpoint whose UIDVALIDITY moved SHALL fail the delta and name a rekey, never answer a round.

### Requirement: A connection SELECTs a mailbox once per run of commands
An IMAP connection SHALL cache the mailbox it currently has `SELECT`ed and skip a redundant `SELECT` when the next command targets the same mailbox, so a run of commands on one mailbox, most importantly a batch of body fetches, pays a single `SELECT`, not one per command. Every select path SHALL record the selection so a cached skip is always correct. For a hydrate of N bodies across a W-connection pool this makes `SELECT`s ~W rather than ~N, halving the fetch path's round trips over a high-latency link without changing what is fetched. (Pipelining the body `FETCH`es themselves, mbsync-style, is a further io-imap change, not yet done.)

### Requirement: Bodies transfer with bounded memory
A body SHALL be fetched and appended by streaming, fetched straight into the blob store and appended straight from it, so no full message is held in memory; peak memory is bounded to a chunk regardless of message size, including in the batched fetch (each message opens its own streaming sink; the socket-to-blob copy uses a 128 KB buffer). The `Message-ID` link id and the summary SHALL be read from the streamed header prefix, so no extra pass over the body is needed. A card or a calendar resource SHALL be named and summarised from every octet it streams, never from a prefix: a `UID` may follow a blank line or an inline `PHOTO` of any size, and one read from a prefix keyed the same contact by two hashes on the two endpoints of a pair. Such a body is held whole while it streams, its kind bounding its size as a message's does not. (The local m2dir backend is not yet chunked; the IMAP path is.)

### Requirement: The one-source sync runs as two account-wide phases
The one-source (retain) sync SHALL run as two phases across the whole account, not a per-mailbox loop, so the connection pool never idles at a mailbox boundary:

- **Phase 1, spine (parallel over mailboxes).** A work-stealing pool of workers,
each on its own connection *and* its own store handle, reconciles each mailbox's spine (pull + meta + itemize + push), the mailboxes taken in the run's order (see "Collections are taken by role"), in one pass or, primed, in two (see "Every collection's newest mail comes first on request"). The network overlaps across mailboxes; store writes serialise on the store's single-writer lock (the seam sanctions process-level serialization), and every collection is pre-created serially first so no worker races lazy creation. A worker with no mailbox left downloads the bodies of the pages the others have landed (see "A landed page's bodies download while the next pages list"), the spine of a mailbox holding it at rest for each apply.
- **Phase 2, hydrate and apply (one global pool).** Once phase 1 has ended, the bodies still owed (`handle`, size and date from the local mail summary), but those the run found gone (see "A body gone from the server is owed nothing more"), are chunked into
per-mailbox batches queued in the download order the caller chose (largest first by default, see below), each mailbox's primed bodies first when the run primes, a handle of a source with account-wide handles planned once (see "A message under several labels is downloaded once"), and work-stolen across the connections through **one** queue: a worker finishing one mailbox's last batch immediately steals the next mailbox's (`select_cached` re-SELECTs across the boundary), so no connection idles at a mailbox edge. Each batch carries the sizes the store records, which the backend plans its reads with. Bodies stream into the blob store. The worker that fetched a batch SHALL raise its items to `Full` before taking the next one, through its own store handle, over a cache-backed remote serving that batch alone (a miss falls back to a real fetch on the worker's connection). The writes of the workers SHALL be serialised, the same body possibly belonging to two mailboxes.

A body SHALL be readable from the store as soon as its batch is applied, not once the account has downloaded. A run that stops during either phase SHALL keep every batch it applied, so the next run's pull plan holds only what is still without a body; nothing is held in memory beyond the batches in flight, one a connection. In phase 2 a fetch error SHALL stop the pool and fail the run; a batch that fails to apply SHALL be warned about and left for the next run.

A dry run stops after Phase 1 (reporting the pull plan, downloading nothing). Progress is the two phases: `Scanning <source> (k/M)`, then `Downloading <source> n% (done/total)`, once over the bodies phase 1 queued and had not settled when its last collection was listed, counted from there, then over those phase 2 owes, a count of its own.

### Requirement: A landed page's bodies download while the next pages list
In the one-source sync, each time a page of a collection's listing has landed, its members still without a body SHALL be queued for download, in batches of the hydration batch size ordered by the download order the run uses; before the first page of a round an earlier run left open, every member of the collection still without a body SHALL be queued (when the run primes, the first pass queues them instead). A connection with no collection left to spine SHALL take the queued batch that comes first in the download order across the account, ties going to the collection the run takes first, fetch it on its own connection and raise it to `Full` at once, so the first bodies of a large collection are readable before its last page is listed. This holds over every backend's connections, Microsoft Graph's as IMAP's.

A batch SHALL be applied to a collection only where its sync is at rest: while its listing waits on the server, or once its spine has ended, never between a page's load and its write, which would write the placement back without its body. An interrupted run SHALL keep every page and every body it applied. What phase 1 did not download (a run over one connection that does not prime, a batch that failed to download or to apply, which is warned about) SHALL be downloaded by phase 2, which reads the bodies still owed once phase 1 has ended, fetching none twice. A dry run SHALL queue nothing.

#### Scenario: The first body before the last page
- **GIVEN** a mailbox of 1,600 messages synced for the first time over four connections
- **WHEN** its first page of 500 has landed
- **THEN** bodies of that page are readable while the round is still open

#### Scenario: Interrupted with bodies
- **GIVEN** that run killed once a body was readable and the round open
- **WHEN** the next run syncs the mailbox
- **THEN** the pages and bodies the first run landed are kept, and the run fetches exactly the bodies the store lacks

### Requirement: The caller chooses the download order
`sync --download-order <largest|newest>` SHALL choose the order the one-source sync downloads bodies in, in both phases; `largest` is the default. Under `largest`, bodies are ordered as before: by size, largest first, within each collection (in phase 1, within each landed page) and, batch by batch, across the account. Under `newest`, bodies SHALL be ordered by their date (the mail summary's instant, from the store meta), newest first, then chunked into batches that stay within one collection, the batches taken across the account by their newest body among those queued. A body without a date SHALL come after every dated one, in plan order. Neither order adds a server request.

#### Scenario: Today's mail first
- **GIVEN** an Inbox holding a large mail from last year and a small one from today
- **WHEN** `sync --download-order newest` runs
- **THEN** today's mail is fetched and readable first

### Requirement: Hydration may run concurrently, largest-first
Full-tier hydration SHALL fetch bodies in **batches**: one `UID FETCH <set> (UID BODY.PEEK[])` streaming K bodies (`BATCH_SIZE`, default 64) in a single response, so N bodies cost ~N/K round trips per connection rather than one round trip per message. Each message is routed to its own streaming sink by the **UID on its own FETCH line**, so an out-of-order server response still lands correctly; a body line without a parseable UID SHALL fail the batch so the caller falls back to per-message fetches rather than misroute. In the one-source sync, hydration follows the listing page by page on the connections it leaves idle, then ends in one account-wide phase (see the two phases above), each batch applied as it lands: bodies are ordered **largest-first** globally by default (or newest first on request, see the download order) using each item's size from the store meta (no size probe), chunked into per-mailbox batches, biggest first, and work-stolen across the pool over one queue with no per-mailbox barrier; the cross-source copy path, lacking sizes, falls back to UID order. On any batch error the fetch SHALL fall back to per-message fetches; content-addressing makes the partial retry idempotent. The pool is **persistent**: connections are opened up front and kept for the run, so their auth is paid once, not per batch. The budget is per backend and SHALL stay under the backend's per-account cap: IMAP takes `connections`, 4 by default, configurable per account and overridable by a `sync --connections` flag; Microsoft Graph takes its `pool-size`, else 3, never more than `connections` (Graph serves four requests at once a mailbox, a `$batch`'s inner requests among them); `pool-size` SHALL be a key of a Graph domain table alone, any other backend refusing it as unknown, since none reads it; Gmail, Google Calendar, Google People, CalDAV and CardDAV keep one. Each connection is its own client: what one caches (Graph's folder ids, listed rows, Drafts id) serves it alone, and what they share (the throttle, the downloaded-bytes count) is shared across threads. Body bytes stream lock-free into the blob store; the engine serialises the index write on the single-writer store afterwards. The largest-first order takes its sizes from the store meta, never a server size probe.

> Initial seed spec (Cairn adopted 2026-07-31): captures the sync driver's core
> guarantees; the CLI surface, mailbox diff, and report are further capabilities
> to spell out as they are touched.

### Requirement: A source's backend declares the kind it syncs
Every sync backend SHALL declare the IANA media type of the items it syncs, and that type SHALL be recorded as the pimdir collection's `kind`. The kind SHALL be derived from the backend table the source writes and, on Graph, the domain table under it, never declared by a value in the configuration: one backend and one domain per source.

The media type SHALL also be knowable from the protocol alone, without opening a connection. A pair that disagrees SHALL still fail with a clear error before any store write, since a mailbox cannot reconcile against an address book. A store MAY hold collections of several kinds, which is what pimdir is built for, and an account MAY now feed it several.

### Requirement: Mutable-content backends carry a revision and push updates
A backend whose item bodies change in place SHALL report a content revision (an ETag) on every enumerate and fetch, and SHALL return the revision the server assigned from every accepted write. `PimdirChangeKind::Update` SHALL be pushed as a conditional write against the base revision. A write the server refuses because the revision moved SHALL be reported as rejected, so the engine re-merges and records the divergence as a conflict rather than overwriting the remote. Item bodies on an immutable-content backend (mail) keep no revision, and an update there is still rejected as impossible.

### Requirement: Conflicts are surfaced in the run report
A placement the engine marked `conflicted` SHALL appear in the sync report (text and `--json`), naming its collection and item, and SHALL keep appearing on every run until it is resolved. This SHALL hold whatever the account's topology is: a source reconciled against the store alone and a source reconciled against a target report a parked divergence the same way.

A run SHALL name a divergence once. A collection is reconciled until it is quiescent, so a pass runs several times over it and every endpoint reports into one report, a side no write can reach (its rights or its authority forbid every write) being listed by the opening pass alone, as later passes would only list it again; a divergence the run's own merge settles and a later pass marks again is one divergence, one line and one notification. A create a side refused and a write a side would not take SHALL likewise be named once per item, however many passes met the same answer, while two copies of one identity stay two refusals.

A run SHALL first merge the three bodies and resolve the conflict where the merge reports no collision, so only a genuine disagreement is surfaced. Neverest SHALL NOT decide a collision by itself; that decision is an edit, staged through the pimdir queue by whoever owns it.

#### Scenario: A mirrored pair names the endpoint that parked it
- GIVEN an account naming one source and one target
- WHEN a run parks a divergence on either of them
- THEN the report names the item, its collection and that endpoint, and the notification is raised

#### Scenario: A convergence loop does not multiply the report
- GIVEN a collection whose reconcile takes several passes
- WHEN a divergence the run settled is marked again by a later pass
- THEN it is reported once

### Requirement: A collection's report reaches the account's whole
A report filled while reconciling one collection SHALL be merged into the account's report entire. Every arm a collection fills SHALL travel: the item and collection patches, the divergences it parked, the duplicates a side refused, the writes a side would not take and the collisions it skipped. Only what a collection never had an opinion about SHALL stay behind, namely the account's own name and dry-run flag, the retention sweep, which runs once for the account, and the outstanding conflict count, which is read from the store rather than summed.

Collections are reconciled across a worker pool and their reports merged at a barrier, so this is the one place the account's report is assembled. A merge that carries the patch alone leaves the run saying how many items it touched and never what it left behind: no warning block, and no notification, the announcement being raised from the parked list and returning early when it is empty.

#### Scenario: A conflict parked in a worker reaches the printed report
- GIVEN a run whose worker parks a divergence while reconciling a collection
- WHEN the account's report is printed
- THEN the warnings block names the item, its collection and its source, and the notification is raised

### Requirement: A run merges what nobody disagreed about
A run SHALL three-way merge the base, local and diverging bodies of a marked conflict, and SHALL resolve it as an ordinary edit when the merge reports no collision. The merge SHALL be built in rather than configured, and SHALL be dispatched on the collection's kind: vcard-rs for contacts, ical-rs for calendars and tasks and journals. Mail is immutable-content and reaches none of this.

The merge SHALL NOT be gated by a cargo feature of its own, and SHALL ride on the feature that decides whether a mutable-content kind exists at all. Built in rather than configured is a statement about build time as much as about configuration: a feature that removes the merge makes this requirement false in the builds that omit it. Nothing else can reach a merge, so nothing is lost by tying them.

It SHALL resolve on an empty report and on nothing else. Being unswappable is what forces that: a merge nobody can replace has no business deciding anything a person might have decided differently, and the report distinguishes the two exactly.

Most divergence is not disagreement. Two sides editing different fields of one card have said nothing contradictory, and the stored base is what proves it, by naming which side touched which field. Reporting those to a person is a background tool asking to be switched off.

A calendar's stamps SHALL NOT collide. `DTSTAMP`, `LAST-MODIFIED`, `CREATED` and `SEQUENCE` say when and how often a side wrote, never what anybody decided, and a server rewrites them on its own: Graph moves `LAST-MODIFIED` when it sends a meeting's invitations and carries no `SEQUENCE` at all. They SHALL leave the base and the diverging side before the merge, the local side keeping its own, and each merged component SHALL carry the higher `SEQUENCE` of the two sides, a revision count only growing.

#### Scenario: Disjoint edits need no one
- GIVEN a conflicted contact whose sides changed different fields
- WHEN the run merges it
- THEN both changes survive, the conflict clears through the queue, and nothing is reported

#### Scenario: Restamped sides need no one
- GIVEN a conflicted event edited locally while the server only restamped its copy
- WHEN the run merges it
- THEN the local edit survives with the higher `SEQUENCE`, and nothing is reported

#### Scenario: A collision is not merged away
- GIVEN a conflicted contact whose sides set the same field differently
- WHEN the run merges it
- THEN the conflict stays parked and the run reports it

### Requirement: A conflicted run succeeds, with its own exit code
A run that reconciled its collections and left work behind SHALL exit with a code distinct from both success and failure, 3 (the first partial-outcome status of the Pimalaya exit contract, pimalaya-cli's `EXIT_PARTIAL`), and SHALL report the outstanding conflict count read from the store, the bindings' conflicts and the items' own (pimdir SYNC §9), rather than the count the run itself marked.

These states are that code, and they are one class: a divergence waiting for a decision, a duplicate `UID` a side refuses, a write a side would not take, a send or an intent this run parked (refused for good, or unconfirmed), a queued action the store holds parked, a listing that parked a mass absence, and a change withheld over a body the store cannot serve. Each leaves something a person has to settle, and a rerun on its own changes none of them. Conflicts, refusals and parked queue actions are re-reported on every run until a person acts; a parked send or intent is reported by the run that parked it; a parked absence and a withheld change are reported by every run meeting them.

A conflict is one item wide, and so is a refusal. Failing the run would stop every other item over one divergence, and under a supervisor restarting on failure it would loop over a state no supervisor can resolve. The distinct code says the same thing without pretending the run broke.

The two conflict counts differ and the difference matters: the engine emits nothing for a placement already parked, which is what keeps notifications quiet across repeated runs, and which is also why the run's own tally is not the number of decisions waiting.

#### Scenario: A parked conflict does not fail the run
- GIVEN a collection holding one parked conflict beside ordinary items
- WHEN it is synced
- THEN the ordinary items reconcile, the run exits with the conflict code, and the outstanding count is reported

#### Scenario: A run that could not deliver a write says so
- GIVEN a source that refuses the only write a run had to make
- WHEN the run ends
- THEN it exits with the same code, rather than reporting success over a change that stayed in the store

#### Scenario: A refused send is not a success
- GIVEN a queued send the server refuses for good
- WHEN the run ends
- THEN the send is parked and the run exits with the conflict code

### Requirement: An incomplete run has its own exit code
A run that could not do all its work SHALL exit with a code of its own, distinct from success, failure and the conflict code, 4: a source it could not scan, an endpoint or a send channel it could not open, a hunk it could not apply, a push its server answered nothing for, a send or an intent that failed without parking, a pair collection whose sync failed, a store step that failed, a source that gave up throttled, a round left open on a collection it synced (but one its listing parked over a mass absence), or convergence passes that ended still moving. It SHALL win over the conflict code, the report still counting what waits for a person. Its text output SHALL NOT say the account is in sync or synchronized without saying it is incomplete, and SHALL list the endpoints it could not reach and the sources that gave up throttled.

An unreachable server is the common case. The run stops nothing else over it, so it is no failure, yet a run with nothing to do and a run that could not reach its server both exited 0, and a caller told offline from idle by parsing error strings. A rerun clears this state on its own, which is what sets it apart from the conflict code: that one waits for a person, this one for the network.

A send or an intent that parked is not this: it waits for a person, and its row says why. Nor is a round a mass absence parked: a rerun meets the same listing.

#### Scenario: An unreachable source is not an idle run
- GIVEN an account whose only source refuses the connection
- WHEN it is synced
- THEN the scan error is reported and the run exits with the incomplete code

### Requirement: A refused command line exits 2
A command line the grammar refuses (an unknown flag or subcommand, a missing or malformed argument) SHALL exit 2, clap's own status, through pimalaya-cli's `clap::parse`. With a `--json` word before any `--`, it SHALL print on stdout the failure object coded `usage-invalid`, its `error` clap's message without the `error: ` prefix and without styles, `sources` empty and `backtrace` null; without it, clap's rendering SHALL go to stderr. Help and version SHALL print and exit 0 as clap does.

1 stays a failure and the partial outcomes start at 3, so a script tells a mistyped invocation from a run that failed or did part of its work. The JSON schema of `neverest-error` SHALL admit `usage-invalid` beside neverest's own codes.

#### Scenario: An unknown flag under JSON
- GIVEN `neverest --json sync --no-such-flag`
- WHEN it runs
- THEN it prints one failure object coded `usage-invalid`, its `error` naming `--no-such-flag`, on stdout and exits 2

### Requirement: An unreached endpoint is named in the report
A run SHALL list in its report, as an `unreached` error ("A run's failures are one list"), every endpoint whose connections it could not open, named under `endpoint`, its `id` `null`: a server it could not reach, or one refusing the connection or its credentials. The failure SHALL still read as a `collection-patch` error of a `*` scan and make the run incomplete (exit 4). A collection that failed to read on an endpoint that opened SHALL NOT be listed: that is a server answering, not an endpoint unreached.

A source's SMTP channel SHALL be listed as `<source>.smtp` when its server could not be reached, with no `*` scan, a send channel holding no collection; it SHALL still make the run incomplete. Credentials it refused SHALL NOT list it: the sends report the refusal.

An exit 4 says the run did not do everything, and the patch says which collection failed; neither said whether the server was reached, so a caller told "offline" from "one folder unreadable" by matching the scan of `*`. Reported by MOA, whose "provider unreachable" read the first scan error of an exit-4 report.

#### Scenario: A closed port
- **GIVEN** an account whose CalDAV source points at a port nothing listens on
- **WHEN** it is synced
- **THEN** the run exits 4 and an `unreached` error names `caldav` with an error beginning `Open connection`

#### Scenario: A closed submission port
- **GIVEN** an IMAP account whose `smtp` table points at a port nothing listens on, and a queued `submit`
- **WHEN** it runs `sync --intents-only`
- **THEN** the run exits 4, an `unreached` error names `imap.smtp`, and the row reads pending with no attempt

### Requirement: A Graph mail is dated by its `Date`
The Graph mail source SHALL fill a message's summary `date` from `sentDateTime`, Graph's reading of the `Date` header, which pimdir's mail `date` column and sort key hold (STORAGE Annex A.1). A message without one SHALL have no date, never its `receivedDateTime`.

It SHALL write that date as the store takes one, an instant in UTC at seconds precision under `Z` (`2026-08-01T10:00:00Z`, pimdir STORAGE §13), an offset brought to UTC, the text io-pimdir derives from the `Date` of the same message (src/msgraph/client.rs `a_row_and_its_body_derive_one_key`): the store holds a date of any other shape as none, such a message being in every scope. It is the one mail date neverest writes itself, IMAP's and Gmail's being io-pimdir's derivation of the `Date` field.

#### Scenario: A mail delivered late
- **GIVEN** a Graph message sent at 12:00 and received at 12:05
- **WHEN** its delta row is folded into a summary
- **THEN** its date is 12:00

### Requirement: An IMAP move respects the server's capabilities

An IMAP source SHALL use UID MOVE only when its authenticated capabilities advertise MOVE or IMAP4rev2. Otherwise, a single-message move with a searchable, non-minted identity SHALL search the destination before copying, copy only when absent, then mark the source UID Deleted and expunge that UID alone. The fallback SHALL require UIDPLUS and SHALL refuse a missing searchable identity before mutation. A failed search or copy SHALL never delete the source. A failed delete SHALL leave the move pending; a retry SHALL find the destination copy and finish the deletion without copying again. No fallback SHALL use unrestricted EXPUNGE.

#### Scenario: OVH offers UIDPLUS but no MOVE

- GIVEN an authenticated IMAP4rev1 server advertising UIDPLUS without MOVE
- WHEN a message with a searchable identity moves to another mailbox
- THEN the source searches the destination, copies the message if absent, marks the source UID Deleted and expunges only that UID

#### Scenario: Copy succeeded before deletion failed

- GIVEN a fallback copy in the destination and its source message still present
- WHEN the move is retried, including on a new connection
- THEN the destination is searched and the source is deleted without another copy

### Requirement: An IMAP delete expunges its own message alone
An IMAP delete SHALL mark the UID `\Deleted` and expunge it by `UID EXPUNGE` on that UID (RFC 4315 UIDPLUS, built into IMAP4rev2), never by a plain `EXPUNGE`, which removes every message any client marked `\Deleted` in the mailbox. On a server advertising neither, the delete SHALL fail before any flag is stored, the push staying in the store as rejected.

#### Scenario: Another client's deleted message survives
- **GIVEN** a mailbox holding two messages, one marked `\Deleted` by another client
- **WHEN** the store deletes the other one and the account is synced
- **THEN** the deleted one is gone from the server and the one the other client marked is still there

### Requirement: A throttled provider is waited out, then named
A request Microsoft Graph, Gmail, Google Calendar, Google People, CalDAV or CardDAV answers with 429, or with 502, 503 or 504 for a request that creates nothing, or Google with a 403 rate limit, SHALL be sent again after the wait the provider states, else after a bounded exponential back-off with jitter. Google's refusals SHALL be read by their reasons and wording: a burst (a concurrency 429, a per-second rate, a 502, 503 or 504) SHALL wait one to two seconds; a per-minute quota, whose answer names the minute (`Units per minute per user`) whatever its reason, two, three, four then five seconds, then the next minute; a daily quota SHALL give the source up until the next midnight in America/Los_Angeles. Past the bound, or on a single wait longer than `sync --max-wait <SECONDS>` (120 by default), the source SHALL give up: no connection opened from it sends anything until the wait is over, what the run already wrote stays, the report lists the source under `throttled` with `until` (RFC 3339 UTC), and the run is incomplete (exit 4). Graph's body batches and Gmail's batch pauses SHALL wait no longer than that bound.

#### Scenario: A server answering 503
- **GIVEN** an account whose CalDAV source answers every request 503
- **WHEN** it is synced
- **THEN** the run exits 4 and `throttled` names `caldav` with an `until`

#### Scenario: A gateway error on a listing
- **GIVEN** a Google People connections listing answered `502` "Error 502 (Server Error)!!1"
- **WHEN** it is synced
- **THEN** the listing is sent again a second or two later, rather than failing the run

#### Scenario: Gmail's per-minute quota
- **GIVEN** a Gmail read answered 403 `Quota exceeded for quota metric 'Total Query Cost' and limit 'Units per minute per user'`, reason `rateLimitExceeded`
- **WHEN** it is synced
- **THEN** the read is sent again two seconds later, rather than failing the run

### Requirement: Gmail is paced below its quota
The Gmail source SHALL hold itself to 200 quota units a second across all its connections, and to `gmail.units-per-minute` units over any sixty seconds (6,000 by default), each call, and each batch for all its reads, taking its cost before it is sent, a read costing the 5 units Gmail bills.

### Requirement: Gmail reads in paced batches
A `gmail` source SHALL read message metadata, sizes and raw bodies through Gmail's HTTP batch endpoint, at most `gmail.batch-size` reads a batch (10 by default, 1 to 100) and at most `gmail.batches-in-flight` batches at once across the source's connections (1 by default), each batch paced by the units of all its reads. The size SHALL adapt: a batch with a read refused for concurrency or rate, failed transiently or left unanswered SHALL cut the size to half the reads it carried, never raising it and never below one read, and a full batch sent at the size in force and answered with no refusal SHALL double it, never above `gmail.batch-size`; a per-minute quota refusal SHALL leave it as it is. The source's connections SHALL share the size for the run, a later read starting where the last one left it. A read refused inside a batch for concurrency, rate or a missing answer SHALL be sent again at the head of the next batch after one to two seconds, one refused a per-minute quota likewise after two to five seconds and then the next minute; a read answered 404 SHALL be a message gone, never an error, and a body read so SHALL be answered gone to the driver; a daily quota, or a read refused six times, SHALL give the source up. A raw batch SHALL hold about 8 MiB of messages by their sizes, a message over 5 MiB being read alone, and each body SHALL be stored as its batch lands. The sizes SHALL be the run's own reads, else the ones the store records for the bodies it owes, a minimal read asking Gmail only for the sizes neither knows.

A run SHALL read a message once: a message carrying several synced labels SHALL be named, under the second label listing it, from the metadata the first read, its flags from that label's own flag listings. The run SHALL read the mailbox history once, with no label, and project it on each label.

#### Scenario: Ten reads a batch, the refused one put back
- **GIVEN** a Gmail label of 45 unbound messages, one refused for concurrency once
- **WHEN** its round page is read
- **THEN** six batches are sent one after another, 10, 5, 10, 10, 10 and 1 reads, the refused message first in the second, halved by the refusal and doubled back after it

#### Scenario: Refusals shrink the batches until one lands
- **GIVEN** a Gmail label of 20 unbound messages, the first refused for concurrency three times
- **WHEN** its round page is read
- **THEN** the batches carry 10, 5, 2, 1, 2 and 3 reads, and the size left for the run is 4

#### Scenario: A minute refusal leaves the size alone
- **GIVEN** a Gmail label of 25 unbound messages, the first refused once for concurrency and the eleventh once for the per-minute quota
- **WHEN** its round page is read
- **THEN** the batches carry 10, 5, 5 and 7 reads

#### Scenario: Batches sent before a refusal land after it
- **GIVEN** two connections that each built a batch of 10 before either was answered
- **WHEN** both are refused
- **THEN** the size is 5, not 2, and a batch of 10 answered later leaves it at 5

#### Scenario: A message under two labels
- **GIVEN** a message filed under `INBOX` and a user label
- **WHEN** both are listed in one run
- **THEN** its metadata is read once

#### Scenario: A resumed hydration
- **GIVEN** bodies an interrupted run left owed, their sizes in the store
- **WHEN** a run downloads them
- **THEN** raw batches read them with no minimal read first

### Requirement: Entering a conflict is said once
A run SHALL warn once for each placement that entered conflict during it, and SHALL say nothing about one an earlier run already parked. Neverest SHALL raise no desktop notification of its own, and SHALL NOT link a notification daemon.

The report SHALL keep the two apart, and this is what makes notifying possible without building it in: the conflicts a run marked are listed item by item, and the count the store holds waiting is carried beside them. A caller reading the JSON report notifies on entry by testing the first, once, with no state of its own to keep, and can name the item, its collection and its side while doing so.

An unattended tool that repeats itself is one a user silences. A five-minute schedule and one unresolved conflict is otherwise nearly three hundred notifications a day, all naming the same card.

The exit code SHALL NOT be read as that signal. It answers a wider question, whether the run left anything waiting at all, which a parked conflict, a refused duplicate `UID` and a rejected write all satisfy.

#### Scenario: The second run is quiet
- GIVEN a conflict marked by one run and left unresolved
- WHEN a later run observes it again
- THEN it is not warned about again, and the report lists it as outstanding
rather than as newly marked

#### Scenario: A caller notifies on entry
- GIVEN a run that marked one conflict and a store holding three others from
earlier runs
- WHEN the JSON report is read
- THEN the newly marked one is listed and the outstanding count is four, so a
caller announces one item rather than four

### Requirement: Deciding is a command, never a run
Neverest SHALL NOT decide a content collision during a sync, and SHALL NOT open an editor or any interactive program from one, whatever is attached to its terminal. Deciding SHALL be `neverest conflict resolve` and nothing else. A resolution SHALL be judged by the queue row it staged alone: another producer's row parking in the same drain SHALL NOT fail it.

`--prefer-local` and `--prefer-remote` discard a side, which is what a person may ask for by name and what a background run may never do on its own. `--interactive` SHALL hand the bodies to the merger the conflict's kind names, `contact.merger` for a card and `calendar.merger` for an event, as filesystem paths, base first, then the divergent sides, then the path to write, and SHALL take the result only on a zero exit with that path modified. A non-zero exit, or an untouched output, SHALL leave the conflict exactly as it was: an editor exits zero on a bare quit, and reading that as a choice would discard a side by accident. The command SHALL then print its `aborted` outcome and exit 3, a decision a person still has to make, never 0.

No merger SHALL be handed another kind's bodies: a tool reading cards cannot read events, so one command for every kind fails on half of them. A kind naming no merger SHALL be refused naming its key, and a mail conflict SHALL be refused outright, mail bodies being immutable.

A command naming any of `{base}`, `{local}`, `{remote}` and `{output}` SHALL be substituted rather than appended, for a tool whose output is a flag rather than its last argument. The documentation SHALL show the form each named tool actually takes: tcard and tcal both take three positionals and their output as `--output`, so both are the placeholder form, and neither runs when handed four positionals.

A tty is not consent. A run has one when it is driven by a wrapper script, when it is watched from a pane nobody is sitting at, and when it is a person waiting, and the three are indistinguishable from inside. Escalating on that signal blocks every remaining collection behind a human who may not exist.

#### Scenario: A sync with a terminal still parks
- GIVEN a run attached to a tty that marks a collision
- WHEN the run continues
- THEN no program is spawned, the conflict parks, and the remaining collections reconcile

#### Scenario: An aborted merger changes nothing
- GIVEN an interactive resolution whose merger exits non-zero, or leaves its output untouched
- WHEN it returns
- THEN the conflict is unchanged and nothing is pushed, the command printing `aborted` and exiting 3

#### Scenario: A card never reaches the calendar's merger
- GIVEN an account naming `calendar.merger` and no `contact.merger`, and a parked card
- WHEN the card is resolved interactively
- THEN it is refused naming `contact.merger`, nothing runs, and the card stays parked

### Requirement: The merger is named once for the document
The interactive mergers SHALL be configurable at the document level as well as under an account, the account's value winning where it is set and the document's applying where it is not. Neither SHALL be required, and an account naming no merger with no document-level one SHALL leave `conflict resolve --interactive` with nothing to run, as before.

The merger is a property of the person rather than of the account: the same program settles a collision whichever account raised it, the way one `$EDITOR` serves every repository. Requiring it per account copies one long line, carrying four placeholders in a fixed order, once per account.

The two SHALL be reconciled where the account configuration is loaded, so every reader still sees one optional command and no call site learns there are two places it could have come from.

An `$EDITOR` or `$VISUAL` fallback SHALL NOT be inferred: a merger is handed four paths under a contract no text editor knows, so guessing one would open the base file and read a bare save as a decision.

#### Scenario: One merger serves every account
- GIVEN a document naming `contact.merger` and two accounts naming none
- WHEN a conflict is resolved interactively under either
- THEN both hand the bodies to that merger

#### Scenario: An account overrides the document
- GIVEN a document naming `calendar.merger` and an account naming another
- WHEN a conflict is resolved interactively under that account
- THEN the account's merger runs and the document's does not

### Requirement: A settled body is a body of its item
A body a resolution settles on SHALL be read before it is staged, and refused unless it is a body of the collection's kind and of that item. It SHALL open and close with the kind's component delimiters (`VCARD`, `VCALENDAR`), and it SHALL state the identity the item is bound by: a body stating another `UID`, or none where the item states one, SHALL be refused. Mail SHALL be refused outright, its bodies being immutable.

A settled body and a body a run merges SHALL be queued with every `X-PIMDIR-` property removed (pimdir STORAGE §18), through io-pimdir's `enqueue_with_body`: neither is the user asking a performer for an instruction.

A refusal SHALL leave the divergence exactly as it was, which is what an aborted merger already does.

The three bodies a run merges by itself are the store's own, and the merge refuses a side no parser reads. A settled body is the one body reaching the store that nothing derived: a merger that crashed after a partial write, a template saved half-finished and a tool writing its error message to the output path all produce bytes that are not a contact, and the item keeps its link id while losing every field that identity came from.

#### Scenario: A merger writes something that is not a card
- GIVEN an interactive resolution whose merger writes bytes no parser reads and exits zero
- WHEN the decision is applied
- THEN it is refused naming the delimiters, nothing is staged, and the conflict is still parked

#### Scenario: A resolution may not rename the item
- GIVEN a settled body that reads as a card but states another `UID`
- WHEN the decision is applied
- THEN it is refused naming both identities

#### Scenario: A settled body carrying an instruction
- GIVEN a settled card holding an `X-PIMDIR-` property
- WHEN the decision is applied
- THEN the card lands without that property, the rest of it as written

### Requirement: A resolution is refused when the remote moved under it
`neverest conflict resolve` SHALL record the revision the resolution was computed against and SHALL refuse to push when the store has since observed a newer one, reporting it rather than applying it.

An unresolved conflict tracks the newest remote revision on every run, so a decision made in an editor over an hour can be a decision about a version nobody holds any more. Pushing it would overwrite everything that arrived meanwhile, which is the loss the parking exists to prevent, arriving at the last step instead of the first.

#### Scenario: A stale decision is not applied
- GIVEN a resolution computed against one revision
- WHEN the store has observed a newer one before it is applied
- THEN the push is refused and the conflict is reported as moved

### Requirement: Deciding never owns the store
A conflict command SHALL read the store through a handle that owns nothing and takes no lock (pimdir STORAGE §8), and SHALL NOT hold the store's owner lock across a decision. A resolution SHALL re-read the divergence and its bodies for each attempt, release the store before the merger runs, and take the store again, under the run lock, only to apply what came back.

The store's owner lock lives on the handle, so a handle kept for a command's lifetime refuses every sync of that store for as long as a person sits in an editor. That is the window the staleness guard exists for, and holding the lock across it makes the guard unreachable: the only thing that moves a placement's conflict revision is a sync of that store, so the revision cannot move, and the refusal, the retry and the re-export never run.

#### Scenario: A sync writes the store while the merger is up
- GIVEN an interactive resolution whose merger is still running
- WHEN a sync of that store records a newer conflict revision
- THEN the write is not refused, and the decision the merger returns is exported again against what arrived

### Requirement: A new resource name never collides with a stored one
A resource name derived for an item being appended SHALL be unique within its collection. Where the item's link id was minted because its identity was already taken (pimdir STORAGE §9), the name SHALL carry the same distinguishing part, so two items sharing a `UID` are pushed to two hrefs.

The fallback that derives a name from the body is the trap: a duplicate's body carries the same `UID` as its twin, so a name derived from it collides by construction, and a colliding `PUT` is not refused by the server but applied to the resource already there. The copy that was already synced is overwritten by the copy being appended, which loses an event and reports success.

#### Scenario: Two copies are pushed to two names
- GIVEN two items of one collection sharing a `UID`, one keyed bare and one minted
- WHEN both are appended to a source that holds neither
- THEN they are created under two distinct resource names

### Requirement: A resource has one spelling
A DAV member SHALL be addressed by one spelling of its name, whichever way the server writes its href: an encoded unreserved octet decoded, a `:` encoded as `%3A`, every escape in upper case, and every other escape kept. A name neverest derives for a new resource SHALL be written in that spelling.

A server is free to percent-encode what a path segment may carry literally. Stalwart lists `urn:uuid:x.vcf`, created under that name, as `urn%3Auuid%3Ax.vcf`, and read as two names the one resource became two items, re-copied on every run. The colon is the one spelled encoded rather than literal: a relative reference whose first segment carries one reads as a scheme (RFC 3986 §4.2), and a read addressed `urn:uuid:x.vcf` answered 404.

#### Scenario: A card named after a urn UID is one item
- GIVEN a card whose `UID` is a `urn:uuid:`, on a server listing its href encoded
- WHEN it is copied to another server and synced again
- THEN the second run copies nothing, and the target holds one card

### Requirement: A create is refused when the server hands back a bound handle
A create whose assigned handle is already bound by that source in that collection SHALL be recorded as a rejected push, never as a binding. The engine binds one handle per item per source, and two items pointing at one handle make the next enumeration read one of them as vanished, which propagates a delete of a resource nobody removed.

A server answering a create by updating the resource that already holds the `UID`, rather than refusing it, is what produces the collision. That behaviour is out of spec (RFC 6352 §6.3.2) and cannot be prevented from here, so it is detected on the way back instead.

#### Scenario: A merging server is caught
- GIVEN an append of an item whose `UID` the target already holds
- WHEN the server answers with the href of the existing resource
- THEN the push is reported as rejected and no second binding is written

### Requirement: A refused duplicate names itself
A push refused with the CalDAV or CardDAV no-uid-conflict precondition SHALL be reported as a duplicate `UID` refusal, naming the source, the collection and the `UID`, in the text and `--json` reports alike. It SHALL keep appearing on every run until the source stops holding the identity twice.

The repetition is the point: the run wrote nothing, the state is unresolved, and the line carries the one action that resolves it. That is what separates it from the phantom fetch this change removes, which named work no run could ever complete.

#### Scenario: The refusal is actionable
- GIVEN a target that refuses a duplicate `UID`
- WHEN a run pushes the second copy
- THEN the report names the refusal, the `UID` and the collection, and the run reports having written nothing

### Requirement: A refused write is reported, never counted as applied
A write a remote would not take, and did not refuse as it stands ("A change refused as it stands is held and reported"), SHALL be answered `Rejected` and reported as a `rejected` error ("A run's failures are one list"), naming the source, the collection, the item, what was attempted and why it failed, coded where the server's refusal says so ("A server's refusal carries its code"), in the text and `--json` reports alike. It SHALL NOT be counted among the hunks the run applied: the hunk the run derived for that item SHALL be taken back, the item patch being the plan and not the outcome. It SHALL keep appearing on every run until the write lands or the reason is removed.

A body that never reached the wire, because the blob tree no longer holds it, SHALL be reported the same way and for the same reason: the change is still in the store and the next run will try again.

A create refused with the no-uid-conflict precondition SHALL keep its own entry and gain no second one, since that entry names the identity and the remedy, and one write is one line.

#### Scenario: A server refuses a body
- GIVEN a source that answers an update with a 412
- WHEN the run reports
- THEN the refusal is a `rejected` error with its message, its causes and its code, the update is not among the hunks applied, and a run with no other work does not read as having written anything

### Requirement: A change refused as it stands is held and reported
A connector SHALL answer `Refused { reason }` (pimdir SYNC §4) for a change the remote refuses as it stands, whatever its base: an HTTP status from 400 to 499 naming the request (on DAV, Graph mail, contacts and calendars, Google Calendar, People and Gmail), all but 401 and 407 (the credentials), 408, 423, 425 and 429 (a state that passes) and 409 and 412 (a stale base); an IMAP `NO` naming `CANNOT`, `PARSE`, `TOOBIG` or `NOPERM`; and a rule the change breaks that neverest states before any request, coded `unsupported` (Graph's append outside Drafts, an override Google Calendar or Graph takes through no instance). `reason` SHALL be the failure's messages joined by `: `. A failure naming a quota exceeded or a rate limit, whatever its status, SHALL NOT be a refusal: the account's state, not the change's, which the same change outlives once the state passes, where a refusal holds it for good. Every other failure (a bare IMAP `NO`, a `BAD`, a `5xx`, a transport failure) cannot tell, and SHALL stay `Rejected` as before.

The engine records a refusal on the item's binding and holds the change while it is derived under the same key, pushing nothing, and reports it every run (pimdir SYNC §5). The report SHALL list each as a `refusals` error (`side`, `collection`, the item's `id`, the reason as its `error`), the `id` of a create being its key with no control character, once a run however many passes meet it, counted as a warning, the run exiting 3, and SHALL take back the hunk the run derived for that item, as for a rejected write. A refusal SHALL NOT also be a `rejected` error. The text output SHALL print each among the warnings. Under `--json`, `sync` SHALL print a `change-refused` event for each, once the account's run ends. The change leaves the hold when the item changes (an edit, a flag, a move into a collection the source takes), which pushes it anew, or when it is removed, which withdraws a create whatever the source's rights (pimdir STORAGE §15.6).

#### Scenario: A message appended to a Graph folder other than Drafts
- **GIVEN** a message staged into a Graph `Archive` folder by another source of the account
- **WHEN** the account syncs twice
- **THEN** the first run answers the append `Refused` with "appends to Drafts only", sending nothing, the second pushes nothing, and both report it as a `refusals` error and exit 3

#### Scenario: An IMAP append refused as it stands, and one refused for a quota
- **GIVEN** a pending create the server answers `NO [CANNOT]`, and another it answers `NO [OVERQUOTA]`
- **WHEN** the account syncs twice
- **THEN** the first is appended once, held and reported as a `refusals` error by both runs, and the second is appended by both runs and reported as a `rejected` error

### Requirement: A duplicated identity is mirrored, not reported
One collection holding two resources under one identity SHALL be mirrored as two items, and SHALL produce no report entry of its own. The store holds what the source holds, and a report entry is for work a run could not do.

Two resources means two resources of one source. An identity a sibling endpoint of the same account also holds is one item bound twice, not two items: see "One identity is one item across an account's endpoints".

### Requirement: Two endpoints are reconciled with each other
An account whose endpoints are both authoritative SHALL reconcile them with each other as well as each with its own server. An item both endpoints changed since the store last agreed with them SHALL be three-way merged against the body they both came from, SHALL resolve as an ordinary edit where the merge reports no collision, and SHALL be parked and reported where it reports one. Neither endpoint's body SHALL be written over the other's on a collision.

Neither endpoint's own reconcile can see this. Each of them agrees with its own server, and only the pair disagrees, so nothing marks it and the run reads as quiet while the two servers hold different bodies.

The store keeps the body the endpoints agreed on until every binding reconciled (pimdir SYNC §9), and records the divergence itself when the second endpoint's pull brings another body than the first's: it is the collection's divergence, which the collection's `manual` policy records as the item's own conflict, the item's body (the first pull's) kept and the second endpoint's recorded as the diverging body, every binding of the item then projecting `Conflict`. The merge's ancestor SHALL be the shared body a binding last agreed on that neither side holds (its `shared_object`, else its sync base), which the diverging binding keeps pinned. Either endpoint's merge then settles it, by an edit naming the diverging body under `resolves`, or reports it. A run SHALL NOT fetch an endpoint's body again for a placement projecting the item's conflict whose diverging body the store holds: only a binding's own conflict, or an item's whose diverging body nobody has read, is fetched. neverest keeps no snapshot of its own: one read before the round and held in memory was lost to any run that stopped between the two pulls.

The endpoint whose pull landed first is the merge's left side, `ours`. That decides which body becomes the shared one and therefore what the other's divergence is recorded against; it decides nothing about a collision, which is a person's to settle through `neverest conflict resolve`.

Under `one-way` there is no divergence to reconcile: the source is the truth and the target follows it, which is what declaring an authority is for. A dry run stages nothing.

#### Scenario: Disjoint edits on two endpoints need no one
- GIVEN one card whose two endpoints each changed a field the other left alone
- WHEN the account is synced
- THEN both changes survive on both endpoints and nothing is reported as waiting

#### Scenario: A collision between two endpoints is parked, never overwritten
- GIVEN one card whose two endpoints set the same field differently
- WHEN the account is synced
- THEN the divergence is parked and counted, each endpoint still holds its own body, and rerunning changes neither

### Requirement: A collection deleted on one endpoint is deleted on the other
A collection one endpoint of a pair lists and the other does not SHALL be read three ways, the store being the base: where the store knows both endpoints held it (a binding or a checkpoint of each, pimdir SYNC §5), the other endpoint deleted it, and the delete SHALL cross to the one still holding it only when that endpoint's `collection.delete` allows it, which it does not unless configured; otherwise it is new on the endpoint listing it, and SHALL be created on the other, as that endpoint's `collection.create` allows. A delete the remaining endpoint forbids SHALL leave the collection where it is, and SHALL NOT create it again where it was deleted.

Absence is the only signal a collection deletion leaves on IMAP and DAV, and a listing may drop a collection nobody deleted, so a server collection is deleted from an absence only where the user asked for it (pimdir SYNC §5).

Once the delete has crossed, the collection SHALL be dropped from the store with what it holds, the operator having removed it from the account: a collection made again later under its name then reads as new rather than as deleted on the other side. A dry run reports the hunk and changes nothing.

Without a base, a collection deleted on one endpoint was created again from the other on every run, which no deletion could outlast.

#### Scenario: A deleted address book stays on the other endpoint by default
- **GIVEN** an address book both endpoints held, deleted on one of them, the other configured without `collection.delete`
- **WHEN** the account is synced
- **THEN** it stays on the other endpoint, and the next run does not create it again

#### Scenario: A deleted address book does not come back
- **GIVEN** an address book both endpoints held, deleted on one of them, the other configured with `collection.delete = true`
- **WHEN** the account is synced
- **THEN** it is deleted on the other, the report naming the delete, and the next run creates nothing

#### Scenario: An address book made again is new
- **GIVEN** an address book deleted on both endpoints by a crossed delete, then made again on one
- **WHEN** the account is synced
- **THEN** it is created on the other

### Requirement: A divergence with no common ancestor parks rather than resolving itself
Two endpoints of one account holding one identity under two different bodies, with no body they ever agreed on behind them, SHALL park the divergence and SHALL NOT write either body over the other. A three-way merge SHALL NOT be attempted: with no base it could only ever park, and merging against the target's own body as the base would read the target as having changed nothing and settle on the source's, which is the overwrite parking exists to refuse.

This is the same disagreement as "Two endpoints are reconciled with each other" without the ancestor that requirement merges against, and it is what a migration and a hand-built mirror start from rather than a rare edge.

The store records it (pimdir SYNC §6, §9): the second endpoint's pull joins its body to an item holding another, the item's own conflict, the item's body kept and the endpoint's recorded as the diverging body, before any round pushes, every binding projecting as a conflict and never as a dirty placement a round would push. No binding agreed on a body neither side holds, so no merge is attempted. `conflict list` and `conflict resolve` read it as the item's own conflict, naming no source. `--prefer-local` settles on the item's body and `--prefer-remote` on the parked endpoint's, and the run after either carries the decision to both endpoints. The store clears the item's conflict on that edit, so a settled divergence does not park again.

Under `one-way` nothing parks: the source decides by declaration. A dry run stages nothing.

#### Scenario: Two endpoints holding one identity under two bodies
- GIVEN the same identity under two different bodies on two endpoints, before the store has read either
- WHEN the account is synced
- THEN the divergence is parked and counted, the run exits 3, and each endpoint still holds its own body

#### Scenario: A parked divergence settles either way round
- GIVEN a parked divergence between two endpoints that never agreed
- WHEN it is resolved with `--prefer-local` or with `--prefer-remote`
- THEN the next run carries the settled body to both endpoints and neither body was lost on the way

### Requirement: One identity is one item across an account's endpoints
An identity two endpoints of one account already hold SHALL bind to a single shared item, whichever endpoint the store reads first, and SHALL NOT be minted a second key. The minting rule answers one collection holding one identity twice; a sibling endpoint holding it once is not that.

Identity is settled by the fetch that reads it, against the placements the store answers with. A source's whole-collection projection carries the copies its sibling holds and it does not, so that the merge can derive the append, and reading those as claims on the identity turns the second endpoint's own card into a duplicate of the first endpoint's. The store's load by key therefore answers with the rows that source binds and nothing else, which is io-pimdir's rule (its store capability, a load by key answers with the rows the source binds), and neverest reads the store through the plain seam with no narrowing of its own.

A mirror and a migration both start from two servers already holding the same items, so binding only when an item propagates from one side is binding in exactly the case that does not matter.

#### Scenario: Two servers already holding one card
- GIVEN the same card on both endpoints before the store has read either
- WHEN the account is synced
- THEN the store holds one item under the card's own key, neither endpoint is asked to take a copy it already holds, and a second run is quiescent

### Requirement: The report accounts for every write the run made
A run that wrote to a remote SHALL report it. `already in sync` SHALL mean the run wrote nothing, and an append performed by the sync SHALL appear as a hunk, so a report can be read as the record of what happened rather than a summary that may omit it. A relayed copy (streamed source to source, never reaching the projection the report is otherwise built from) SHALL therefore be itemized where it is performed.

### Requirement: A source may be denied item updates
The per-source permission set SHALL gate item updates (`item.update`, default true) beside the existing collection and item create/delete and flag gates. An update hunk a source's policy forbids SHALL be dropped from the patch and surfaced in the report, so a mutable-content source can be made read-only. So SHALL every hunk a source's rights forbid (a create, an update, a delete, a flag change): the engine holds it in the store and never pushes it, so it SHALL NOT be listed as applied, and the report SHALL count it under `forbidden`, one row a side, collection and kind (`kind`: `create`, `update`, `delete`, `set-flags`), with no warning and no exit code, the configuration having asked for it.

### Requirement: DAV collections enumerate by sync token and resolve at Full
A CardDAV or CalDAV source SHALL enumerate through `REPORT sync-collection`, storing the returned sync token verbatim as the collection's opaque checkpoint, and SHALL fall back to a tokenless report (the whole member set, reported complete) when the server rejects the stored token. Because that report returns hrefs and ETags but no `UID`, the bodies of the members it lists SHALL be fetched through `addressbook-multiget` / `calendar-multiget`, in batches, before the page lands, each member named by its body, so a DAV item's link id has exactly one derivation. A member the store holds at the listed ETag SHALL be named by its binding, its body not fetched again.

A body a multiget or a listing reads SHALL be stored as a GET of the member returns it, the whole text of `address-data` or `calendar-data` (io-webdav 0.9.1): iCloud and Fastmail escape each CR as `&#13;`, and a trimmed text cut the final CRLF of every card and object. A body stored short before SHALL stay as it is until its ETag moves, a member at the listed ETag being fetched again by no run.

### Requirement: A DAV id stays inside what it names
A collection key or a member id that would address the server root, the home set or a parent of either (empty, `/` alone, or holding an empty, `.` or `..` segment, percent-encoded or `\`-separated) SHALL be refused before any request is written: io-webdav's client checks every id it is handed (`check_collection_id`, `check_resource_id`), failing with an I/O error of kind `InvalidInput`, which no retry and no reconnect repeats. neverest takes no DAV id from its command line: the ids reaching a DAV server come from its listings, from the keys another source's collections have in the store, and from the member names it mints, each one segment (`resource_id`). A refused one SHALL fail its collection's or its item's row, io-webdav's reason among the row's sources; the row carries no code, neverest having none for a collection missing.

#### Scenario: A store key climbing out of the home set
- **GIVEN** a store collection keyed `..`, created by another source, on an account whose CalDAV endpoint may create collections
- **WHEN** a sync creates it on the CalDAV server
- **THEN** no request is sent, and the collection's row fails naming the id

### Requirement: A DAV key keeps the segment the server served
Every collection key and member id neverest takes from io-webdav SHALL keep the spelling the server served, the key io-webdav 0.9.1 gave. It SHALL be read from the href io-webdav carries beside each id (io-webdav 0.11), never from the id, which io-webdav decodes wherever encoding it gives the segment back (`My%20Cal` read as `My Cal`, as a raw `My Cal` is): a collection listed directly under the home set by the last segment of its href, query and fragment dropped, and the default calendar a scheduling inbox names by that of its URL's path, a collection outside the home set by io-webdav's path id, which keeps the server's spelling; a member a multiget answers, read or gone, by the id the enumeration reads from its href (A resource has one spelling); a created member by the last segment of the `Location` its server sent, else of the URL the create was sent to; a moved member by the last segment of the `Destination` io-webdav sent. A segment served escaped and one served raw (a space or a non-ASCII byte, which RFC 3986 keeps out of an href but a server may send) SHALL stay two keys, each spelled as served. The reports io-webdav caches while listing, under its own id, SHALL be found under the key, through the id io-webdav reads from that segment, so a collection advertising no `sync-collection` is listed with no REPORT first.

#### Scenario: A calendar named with a space
- **GIVEN** a calendar the server lists as `/dav/cal/My%20Cal/`, synced under io-webdav 0.9.1 as `My%20Cal`, holding a member listed as `r%C3%A9union%201.ics`
- **WHEN** a sync on io-webdav 0.11 lists the calendars and fetches the member's body
- **THEN** the calendar is keyed `My%20Cal`, and the multiget answers the body under `r%C3%A9union%201.ics`, nothing read alone

#### Scenario: A raw space beside an escaped one
- **GIVEN** a server listing both `/dav/cal/My%20Cal/` and `/dav/cal/My Cal/`, which io-webdav reads as the one id `My Cal`
- **WHEN** the calendars are listed
- **THEN** they are keyed `My%20Cal` and `My Cal`, two collections, as under io-webdav 0.9.1

### Requirement: A DAV server without `sync-collection` is listed instead
`sync-collection` is an extension, so a server MAY implement none of it, advertising a `supported-report-set` of `addressbook-multiget` and `addressbook-query` alone. Such a collection SHALL be enumerated through a `PROPFIND` at Depth 1 requesting the ETag, which yields the same member ids and revisions, rather than failing to enumerate at all.

The `PROPFIND` SHALL be preferred over the `addressbook-query` the same server advertises. A query carries a filter, a server evaluates a filter by parsing every member, and a collection holding one member the server cannot parse then fails to enumerate at all, which is the case this recovery exists for. A `PROPFIND` parses nothing.

The listing SHALL be chosen from the collection's advertised `supported-report-set` where a run has read it, a sync listing its collections before it enumerates them, and from the RFC 3253 §3.6 `DAV:supported-report` precondition otherwise, which is the server saying by name that it does not run the report. It SHALL NOT be chosen on the HTTP status alone, the status wrapping that precondition being the server's own choice, except for `405` and `501` which mean the request was never going to run. A permission refusal, a credential failure and a server fault SHALL all surface as failures, and a rejected sync token SHALL keep its own recovery, a fresh full report.

The listing carries no sync token, so its checkpoint SHALL be empty and an empty checkpoint SHALL read as no cursor: such a collection is enumerated in full on every run, which is what a server offering nothing incremental costs.

A listing the server truncates SHALL be reported as a delta rather than as a complete snapshot. A snapshot is read as "absence means removed", so a truncated one taken for a whole collection deletes every member the server left out.

#### Scenario: An address book on a server without the report syncs
- GIVEN a CardDAV server whose `supported-report-set` holds no `sync-collection`
- WHEN the account is synced
- THEN the address book is listed with a `PROPFIND` and its cards reach the store

#### Scenario: A card the server cannot parse costs only itself
- GIVEN such an address book holding one card the server fails to parse
- WHEN the account is synced
- THEN every other card is listed and stored, and only that card's body fetch fails

#### Scenario: A permission refusal is not mistaken for a missing report
- GIVEN a server refusing the REPORT for lack of privileges
- WHEN the account is synced
- THEN the run reports the refusal rather than retrying it as a listing

### Requirement: A DAV connection survives a server that closes it
A DAV source SHALL reopen its connection and run the exchange again when the server closed it between requests (an HTTP/1.0 answer, a `Connection: close`), carrying the discovery it already paid for over to the new connection. Only an end-of-stream or reset failure SHALL be retried, being the shape of a request the server never read, so a create or a delete is never replayed against a server that acted on it.

### Requirement: A backend without flags reports them known-empty
A backend with no flag concept (CardDAV, CalDAV) SHALL report an item's flags as *known-empty*, never as *unknown*. The distinction is normative (pimdir STORAGE §13): reporting unknown makes the engine treat the flag set as unfetched and re-probe the item on every run.

### Requirement: A refused delete is held, never reverted
Every source SHALL sync beside the other sources of its hub (io-pimdir `PimdirSync::beside_other_sources`), holding a refused delete rather than reverting it. Both refusals (`push` off, or `item.delete = false`) run through that one disposition, and each source here is bound to the store's hub, which fixes the answer: reverting a tombstone states that the source still holds the member, and a hub reads that as the item being alive (add-beats-delete across sources), so it clears the deletion for every source and mirrors the item back to the one it was deleted on.

A source configured to take no deletes would then resurrect on both what the user removed on one, which is the opposite of what that setting is for.

#### Scenario: A read-only source keeps the removal
- GIVEN a staged delete on a source whose `item.delete` is false
- WHEN the source is synced
- THEN nothing is pushed and the tombstone stays, rather than being undone into a clean row

### Requirement: A purge is followed by a collection
The store reclaims nothing by itself (pimdir STORAGE §5: an object at refcount zero is unreferenced, not deleted, because the batch that attaches a body may not be the one that indexed it), so the retention sweep SHALL run the collector after a purge that removed rows, and SHALL report the objects it dropped and the bytes it freed beside the items the purge removed. A purge releases a body; it does not reclaim one.

The collector SHALL NOT run after a sweep that took nothing: its cost is a walk of the whole blob tree, and a purge that removed no row released nothing. Orphan blobs a crash left are not this run's to find; they are what `pimdir gc` is for.

#### Scenario: The bytes a purge reports are bytes that left
- GIVEN a retained item past the purge cutoff, holding a body nothing else references
- WHEN the sweep runs
- THEN the item is purged, the object row is dropped, and the blob is gone from the tree

### Requirement: A verb counts as in flight while neverest drives it
neverest's own drive loop (src/sync/mod.rs `run_verb_paged`, which `run_verb` calls) SHALL hold io-pimdir's in-flight guard (`PimdirSourceStore::running`) from the coroutine's first resume to its completion, pause or failure, as io-pimdir's `run` does, so a collection of the process (`collect_garbage`, `evict_before`, `release_before`) refuses as `InFlight` rather than take a body one chunk streamed and the next attaches (pimdir STORAGE §5).

#### Scenario: The collector asked between two pages refuses
- GIVEN a paged mail round driven by `run_verb` and a second handle of the process on the store
- WHEN that handle collects while the verb waits on a listing
- THEN every collection refuses as `InFlight`, and the collector runs once the verb is done (src/sync/driver/tests.rs `a_running_verb_keeps_the_collector_out`)

### Requirement: A submission greets with an address literal
An SMTP submission session SHALL greet with the loopback address literal (`EHLO [127.0.0.1]`), the form RFC 5321 §4.1.3 reserves for a client with no resolvable domain name of its own, which a desktop client behind a NAT never has. It SHALL NOT greet with a bare `localhost`, which is not such a name either: RFC 5321 §4.1.4 entitles a server to check, and one that does (Stalwart) answers `550 5.5.0 Invalid EHLO domain`, failing the session before `MAIL FROM` and leaving every intent pending behind a warning.

### Requirement: A `server` is an authority or a URL, resolved at one seam
Every backend's `server` SHALL accept either a bare authority, with or without a port, or a full URL, and both SHALL resolve through one shared function rather than per backend. The scheme a bare authority takes is the backend's own: `imaps` for IMAP, `smtps` for SMTP, `https` for a DAV entry point.

The presence of `://` SHALL be what tells the two forms apart. A value carrying it SHALL be parsed verbatim, so an explicit cleartext scheme or a non-default port survives; a value without it SHALL take the default scheme. Resolution SHALL NOT be decided by a parse error: a bare authority carrying a port parses as a URL whose scheme is the hostname and whose path is the port, so it reports no error and carries no host, and a backend handed one rejects it for a reason that names neither the value the user wrote nor the field it came from.

### Requirement: A collection that failed to scan is reported, never only logged
A collection whose spine fails SHALL be recorded in the run's report, carrying its error, and SHALL NOT be reported through the log alone. The other collections SHALL still run: they share nothing but the file the store lives in.

A run that failed to scan a collection SHALL NOT report itself in sync. "In sync" is a claim about what the sync compared, and a collection it could not enumerate was never compared; a run that says it anyway hides a broken account for as long as nobody reads the log.

An error crossing an engine boundary SHALL be rendered with its full chain, not with its outermost context alone. A backend keeps a server's status and response body so a caller can read them, and a wrapper that renders only the top drops exactly the part naming what the server said.

### Requirement: A failed pair collection is reported
A pair collection whose sync fails SHALL be recorded as a `collection-patch` error whose `hunk` is a `sync` hunk naming `source`, `target` and `collection`, and the run SHALL exit 4. It SHALL NOT be a `scan` hunk: the failure may come from either endpoint, the store or the merge, and what the collection reported before failing stands.

#### Scenario: A pair whose target fails its listing
- GIVEN a CardDAV pair whose target answers 500 to its REPORT
- WHEN it is synced
- THEN the report's `errors` hold that `sync` hunk with its error, the text does not say the account is in sync, and the run exits 4

### Requirement: A store step that fails is reported
A step of a run that fails outside any hunk SHALL be recorded as a `failures` error, one a failure: `step`, one of `queue` (the queue could not be read or drained), `bodies` (downloaded bodies could not land in the store, or a pair's full hydration failed) and `conflicts` (the waiting conflicts could not be counted) and `listing` (the server cut a listing short); `collection`, `null` for a store-wide step; beside the item error's `code`, `error` and `sources`, its `id` `null`. A `failures` error SHALL make the run incomplete (exit 4) and SHALL be printed among the errors. A step that fails SHALL NOT read as having done nothing: rows a failed read could not classify SHALL NOT be counted as drained.

#### Scenario: A drain that fails
- GIVEN a store whose queue cannot be drained
- WHEN a sync runs
- THEN `errors` holds a `failures` row of the `queue` step with the error and the run exits 4

### Requirement: A body fetch answers for every handle it was asked about
A batched body fetch SHALL be treated as complete only when it answers for every handle it carried, a handle the server answered gone being answered (see "A body gone from the server is owed nothing more"). A batch answering for fewer SHALL fall back to a per-item fetch for the remainder, exactly as a batch that errors already does, and SHALL report the shortfall; a per-item fetch the server answers gone SHALL be answered gone, never fail the batch.

A CardDAV or CalDAV multiget row the server answers 404 or 410 SHALL be answered gone, with no per-item fetch. Any other row it could not read (a 403, a refused or empty data property) SHALL be left unanswered, so the per-item fetch reads it and says what it is.

A backend that answers for a subset is not a backend that failed, so nothing surfaces as an error; but the engine cannot tell an unanswered handle from an unasked one, so an unanswered handle is recorded nowhere and re-requested on every later run. That is a run that fetches a whole collection, stores nothing and reports itself in sync.

A batch that errors SHALL be read item by item on a connection fit for it. An IMAP batch the server refused with a tagged `NO` or `BAD` was answered whole, and its items SHALL be read on the connection it failed on. After any other failure (the connection lost, a body that could not be written, an answer the client stopped reading), what is left of the answer may still be on the connection, where the next command would read it as its own: the connection SHALL be dropped and opened again before any item is read, and one that does not open again SHALL fail the batch.

#### Scenario: A server answers for two cards out of sixty-four
- GIVEN a batched fetch of 64 handles that returns 2 bodies
- WHEN the run continues
- THEN the other 62 are fetched one by one and the shortfall is reported

#### Scenario: An IMAP batch failing in the middle of its answer
- GIVEN a batched fetch of two messages whose answer the client stops reading at its second row
- WHEN the run continues
- THEN a new connection is opened, the two messages are read one by one on it, and each handle holds the body of its own message

#### Scenario: An IMAP batch the server refuses
- GIVEN a batched fetch the server answers with a tagged `NO`
- WHEN the run continues
- THEN its messages are read one by one on the same connection

### Requirement: An empty body is refused, never stored
A body of zero bytes SHALL fail the fetch, naming the item and its collection. No kind neverest syncs has an empty body: a message carries headers and a card carries at least its `BEGIN` and `END` lines. An IMAP UID a lone fetch answers with no message at all is no empty body: the message is gone.

An empty body stored is worse than a fetch that fails. Its link id is the digest of nothing, so every empty body a server returns resolves to the same identity; the second one collides with the first, the duplicate-link-id floor freezes it, and the collection stays frozen for every later run.

#### Scenario: A server returns zero-length cards
- GIVEN a server answering card bodies with zero bytes
- WHEN the run fetches them
- THEN it fails naming the first such card, rather than storing an item whose identity is the digest of nothing

### Requirement: A run reports the bodies it pulls, whatever the tier
A run SHALL report the bodies it fetches, and SHALL report the same ones whether or not it is a dry run. The report SHALL NOT depend on the tier a kind resolves its identity at.

A kind whose meta is its body is fetched while its page is listed, so its pull plan SHALL be read off the pull's events (the items it added or changed holding a body) as well as off the placements carrying no body yet; a plan read off the store alone is empty for exactly the items the run pulled, and the run calls itself quiescent having downloaded a collection.

#### Scenario: A first contacts sync says what it did
- GIVEN an empty store and an address book holding one card
- WHEN `sync` runs without `--dry-run`
- THEN it reports fetching that card, as `--dry-run` said it would, rather than reporting itself already in sync

### Requirement: A CalDAV source syncs calendars
A source MAY declare a `caldav` backend, whose items are `text/calendar` calendar object resources and whose collections are the calendars under the principal's calendar home set (RFC 4791 §6.2.1), keyed by their path segment, or by their path for one outside the home set (`A collection is keyed by its backend id and named by its display name`). It SHALL accept the same `server`, `tls`, `alpn` and `auth` fields as a CardDAV source, and SHALL carry no send channel, submission being a mail capability.

The item SHALL be the calendar object **resource**, never the component: RFC 4791 §4.1 keeps every component sharing a `UID` in one resource, so a recurring series and its modified instances are one item under one link id, and an override is a body edit rather than an item of its own. A new resource SHALL be named `<UID>.ics`, the `UID` sanitised to one path segment, so the href stays derivable from the body, unless its key was minted, which the name then carries too.

A calendar SHALL be synced whole. Restricting it to a component type is an item-level filter, which no kind has.

#### Scenario: A calendar syncs, follows a server edit and retains a delete
- GIVEN a CalDAV server holding two events in one calendar
- WHEN the account is synced, one event is edited on the server and another deleted, and it is synced again
- THEN the store holds both events keyed by their `UID`, follows the edited body, and keeps the deleted one as retained

### Requirement: One adapter serves both DAV protocols
CardDAV and CalDAV SHALL be implemented by one client adapter, parameterised by which of the two a session speaks. The difference between them SHALL be confined to the home set it discovers, the collection listing it runs and the resource extension it names a new item with; enumeration, multiget, conditional writes, flag handling and the reconnect repair are RFC 4918 and RFC 6578 and SHALL NOT be written twice.

The adapter SHALL report its own media type to the client seam, so one backend variant still declares two kinds and the store records the right one per collection.

### Requirement: A path key expands at deserialize, never at a call site
Every path-valued configuration key (`store.root`, every `tls.cert`) SHALL be shell-expanded by pimalaya-config's own deserializer, a leading `~` and every `$VAR` or `${VAR}` alike, as in every Pimalaya CLI, so a value reaching any call site is already resolved. A key SHALL NOT be expanded at the point it is read: one reader forgetting is a lookup for a literal `./~/…` path, which fails naming a file the user never wrote. A variable that is not set SHALL leave the value as written.

An absent optional key SHALL stay absent rather than expanding an empty path, which is what `#[serde(default)]` beside the deserializer buys.

#### Scenario: A certificate under the home directory is found
- GIVEN `imap.tls.cert = "~/ca.pem"`
- WHEN the configuration is loaded
- THEN the certificate path is the one under the user's home directory

#### Scenario: A store under a variable
- GIVEN `store.root = "$HOME/store"`
- WHEN the configuration is loaded
- THEN the store is the one under the user's home directory

### Requirement: A data command describes what it prints
A command returning data SHALL hand the printer one named `*Output` type deriving `Display` for the terminal, `Serialize` for `--json` and `JsonSchema` for the registry, and SHALL print it once. It SHALL NOT report data as a message: a message serialises as one prose string, so `--json` over one yields nothing a consumer can read, and several messages in one run yield several documents where a consumer expects one value.

Every such output type SHALL be registered in the schema registry under its invocation path, the command path joined with hyphens and prefixed `neverest-`, as printed (the serialize contract, so a key always printed is required, `null` or not), and `neverest json-schema` SHALL print one schema to the standard output or write one file per command into a directory. A test SHALL run pimalaya-cli's registry check over the command tree: every runnable command described, bare `neverest` aside (it shows the help, or fails `usage-invalid` under `--json`), and every key naming a command, `neverest-error` aside, so a renamed subcommand cannot leave a schema nobody can ask for.

#### Scenario: A notifier reads the sync payload from its schema
- GIVEN a build of neverest
- WHEN `neverest json-schema neverest-sync` runs
- THEN it prints the schema of the sync result, its accounts' reports naming `conflicts` and `outstandingConflicts` among their fields

#### Scenario: A checked account is one JSON document
- GIVEN a configuration of two accounts
- WHEN `neverest check --json` runs
- THEN one document is printed, naming each account, its mode and every endpoint, answered or not, and the orphan accounts

### Requirement: The JSON contract is described and versioned
`neverest json-schema` SHALL describe, under the key `neverest-error`, the failure object every command prints under `--json`. Every generated schema SHALL carry an `$id` naming the contract version, which is the minor version while Neverest is below 1.0 (`v0.3`) and the major version after. Every registered payload SHALL be a JSON object, checked by a test. Piped `--json` output SHALL print one value per line.

#### Scenario: A caller reads a failure against its schema
- GIVEN `neverest json-schema neverest-error`
- WHEN it runs
- THEN the schema of the JSON failure object is printed, its `$id` being `https://pimalaya.org/schemas/neverest/v0.3/neverest-error.json`

### Requirement: A sync prints its progress as events
Under `--json`, `neverest sync` SHALL print its progress as events before its result, one line each, each an object carrying `event`: `account-started` (`account`, `position`, `total`) before each account the run covers, the events after it being that account's; `endpoint-opened` (`endpoint`) once an endpoint's first connection is open, `endpoint-listed` (`endpoint`, `collections`) once it listed its collections; for a pair, `collection-started` (`source`, `target`, `collection`, `position`, `total`), `collection-progressed` (`source`, `target`, `collection`, `done`, `total`) and `collection-finished` (`source`, `target`, `collection`, then `code`, `error` and `sources` as an item error carries them, `code` and `error` `null` and `sources` `[]` unless the collection stopped short); for a source with no target, `scan-progressed` and `download-progressed` (`source`, `done`, `total`), each printed as its phase starts, a download starting as the scan ends when bodies it queued are still downloading, and again for the bodies phase 2 owes; and `change-refused` (`source`, `collection`, `id`, `reason`), one for each row of the account's `refusals`, printed once the account's run ends. An endpoint SHALL be named as its account names it. A progress count SHALL be printed at most once per percent of its total, a new count of a collection's next phase starting again. Events SHALL report progress and the refusals: everything the run did or left SHALL stay in the result, the last line. The events SHALL be registered as `neverest-sync.events`.

Text mode SHALL print the same events, each as one plain line on stdout before the result, leaving out `collection-progressed` alone, so a pair's collection reads as its start and its end and a download as it goes, once per percent, its line naming the percent before the count. `sync` SHALL print through pimalaya-cli's streaming printer, as carillon's `watch` does, so an event it prints reaches a pipe as it comes.

#### Scenario: A frontend follows a long download
- GIVEN an account with no target whose store owes 2 bodies
- WHEN `neverest sync --json` downloads them
- THEN it prints `account-started`, then `download-progressed` with `done` 0, 1 and 2 of `total` 2, then the result

#### Scenario: A first run over several connections reports its download
- GIVEN a source with no target listing two large collections over four connections, the free ones downloading bodies beside the listing
- WHEN the last collection is listed with N bodies still to settle
- THEN text mode prints `Scanning <source> (2/2)`, then `Downloading <source> 0% (0/N)`, and later `Downloading <source> 100% (N/N)`

#### Scenario: A script reads the result alone
- GIVEN a sync piped to a script
- WHEN the run succeeds
- THEN the last line is the result, listing each account's report, without an `event` key, and every line before it carries one

### Requirement: A signal stops a sync cleanly
`neverest sync` SHALL install pimalaya-cli's `signal::Shutdown` before anything else it does, before `--init` initializes and before it waits for a store's lock. A first signal during the lock wait SHALL end the wait, the accounts of that store reported "Interrupted before the account ran", and the result printed. On a first SIGINT or SIGTERM the run SHALL start no new account, collection, spine or body batch, let what is in flight finish, and print its result: an account it did not reach as failed with "Interrupted before the account ran", an account it stopped in with its report marked `interrupted`. An interrupted report is incomplete, so the run exits 4. A second signal ends the process with 130. A wait for a throttled provider SHALL be cut short by the signal: the request is given up as the provider left it, never sent again, and the run stops within a fraction of a second rather than sitting out minutes.

#### Scenario: Ctrl-C during a long download
- GIVEN a sync downloading the bodies of a large mailbox
- WHEN the person presses Ctrl-C
- THEN the batches in flight land, no other starts, the result prints with `interrupted` true, and the exit code is 4

#### Scenario: SIGTERM while another run holds the store
- GIVEN a sync waiting for the lock of a store another run holds
- WHEN a service manager sends SIGTERM
- THEN the wait ends, the result prints the account failed "Interrupted before the account ran", and the exit code is 4 (tests/exit.rs `a_signal_during_the_lock_wait_prints_the_interrupted_result`)

### Requirement: A check opens the send channel too
`neverest check` SHALL open the SMTP channel an endpoint declares, upgrade it and authenticate, then quit without sending. Each endpoint's row SHALL carry the channel's own check row as `smtp`, pimalaya-cli's `{backend: "smtp", ok, code, error, tls}`, `null` when the endpoint declares no channel, when its credentials did not resolve, or in a build without SMTP. A channel that does not open SHALL fail its row and make the check exit 3, the other endpoints still checked.

A send channel holds its own server and often its own credential. Checking the sources alone left a wrong SMTP password to surface at the first send, a queued submission parked for a reason the check had said nothing about.

#### Scenario: A wrong submission server fails the check
- GIVEN an account whose IMAP source answers and whose SMTP channel does not
- WHEN `neverest check --json` runs
- THEN the source's row is `ok: true` with its collections, its `smtp` row `ok: false` naming why, and the check exits 3

### Requirement: A check lists collections with their roles
`neverest check` SHALL list, for each endpoint, every collection it answered with: its id, its name and its role among `inbox`, `sent`, `drafts`, `trash`, `junk`, `all`, `archive`, `flagged` and `important`, `null` when the server states none. A role SHALL come from the server alone, never from a collection's name: on IMAP the SPECIAL-USE attributes of the `LIST` rows (RFC 6154) and `INBOX` (RFC 3501 §5.1); on Graph mail the well-known folders, one lookup per role, a well-known folder the mailbox does not have being skipped; on the Gmail API the system labels. Other backends state no role. The check SHALL write nothing to the store. It is a doctor: what it prints is for a person, and a program reads roles from the store a sync fills.

Display names are localised (`[Gmail]/Bin`, "Éléments envoyés") and an IMAP server need not name its folders after their use: only the server's statement lets a caller propose the right folder for each use. On Graph, the Drafts folder is the one place a message can be created, so a caller has to know which listed folder it is.

#### Scenario: An IMAP check names the special-use folders
- GIVEN an IMAP account whose server advertises SPECIAL-USE
- WHEN `neverest check --json` runs
- THEN each source lists its collections, `INBOX` with the role `inbox` and the sent, drafts and trash folders with theirs

#### Scenario: A folder without a statement has no role
- GIVEN a folder the server marks with no SPECIAL-USE attribute
- WHEN `neverest check --json` runs
- THEN the folder is listed with `role: null`, whatever its name

### Requirement: A check names the default calendar and address book
`neverest check` SHALL mark with `default: true` the collection of an endpoint where a new item goes when none is named, as the server states it: on Google Calendar the calendar the calendar list marks `primary`; on Google People the one address book; on a Graph calendar source the calendar with `isDefaultCalendar`; on a Graph contacts source the default Contacts folder; on CalDAV the calendar the principal's scheduling inbox names as `schedule-default-calendar-URL` (RFC 6638 §9.2), when the server gives it. CardDAV states none, nor does mail, whose folders have roles. At most one collection per endpoint SHALL carry the mark, every other `default: false`. A failed CalDAV scheduling lookup SHALL be no default, never a failed check.

A caller choosing one calendar and one address book for an account takes the marked one, and the first listed when none is marked.

#### Scenario: A CalDAV server with scheduling
- GIVEN a CalDAV account whose scheduling inbox names its default calendar
- WHEN `neverest check --json` runs
- THEN that calendar is listed with `default: true`, and no other

### Requirement: A check reports what each source declares it can do
`neverest check` SHALL list, for each source, the capabilities a sync declares for it in the store (pimdir STORAGE §15.6) as `capabilities: [{ name, level, detail }]`, one row per capability, `level` being pimalaya-cli's capability level as every pimalaya binary prints it (`full`, `partial` or `none`, the store stating no `server`) and `detail` what a partial level leaves out or why it is refused, `null` otherwise, from its backend and configured rights alone. An intent a source performs on the collections it holds (`calendar.reply`, `calendar.cancel`, their `.occurrence` options, `collection.create`, `collection.rename`, `collection.delete`), which the store gets as a `none` source-wide row and one row per collection, SHALL read as it does on those collections: the store's collections are unknown to a check, so the `none` a rename or a delete gets on a collection stating a role is not listed. The source-wide `calendar.component.vtodo` row a sync declares from the configuration (a Google Calendar or Graph calendar source, or a source one of whose targets is one) SHALL be listed with them. What a CalDAV listing states of a calendar SHALL be listed on that collection as `takesTasks`, `false` for a component set without `VTODO`, `true` for one naming it, `null` where no set is stated, on a target's collections as on a source's: a sync declares it as `calendar.component.vtodo` `none` on the source's calendar of that name. A target and an undeclared (JMAP) source SHALL list no capability. A capability a source learns to declare is reported by the check with no change to it.

#### Scenario: A check names the calendars that take no tasks
- **GIVEN** a CalDAV source whose server lists `events` with `VEVENT` alone and a Google Calendar target
- **WHEN** `neverest check --json` runs
- **THEN** `events` is listed with `takesTasks: false`, and the source's capabilities hold `calendar.component.vtodo` at level `none` naming the target

### Requirement: A check reports every endpoint
`neverest check` SHALL check every endpoint of every account it runs over whatever the others answer, and SHALL print, under each account's `backends`, one row per endpoint, sorted by name: `backend` (its protocol table), `ok`, `code` and `error` (`null` when it answered) and `tls`, then `source`, `collections` (empty when it did not answer), `capabilities` and `smtp`. `tls` SHALL be what the endpoint's `tls` table, its account's inherited, configures (`{provider, crypto, cert, clientCert}`, each `null` where the build's default answers), `null` for a cleartext `imap://`, `smtp://` or `http://` URL that does not upgrade. A credential that does not resolve SHALL fail its endpoint's row alone. When an endpoint or its send channel did not answer, the command SHALL print the report and exit 3, never the failure object.

A frontend adding an account shows what works and what does not in one run, rather than one failure per attempt.

#### Scenario: Two endpoints that do not answer
- GIVEN an account whose source and target both point at a closed port
- WHEN `neverest check --json` runs
- THEN it prints both rows under the account's `backends`, each with `ok: false`, its `error`, `tls: null` over `http://` and no collection, and exits 3

### Requirement: JSON keys are camelCase
Every output type neverest prints SHALL serialize its keys as camelCase, matching the wire formats the endpoints speak (JMAP per RFC 8620, Microsoft Graph, the Google APIs) and keeping every key reachable by dot access in jq and JavaScript, which is how the README's notifier reads a report.

A field carrying a provider or format spelling verbatim SHALL keep its explicit rename, `@odata.nextLink`, `nextPageToken` and the vCard `fn` being the examples: those are the wire's names rather than derivable ones. A variant name travelling as a value, the hunk `kind` and the resolve `outcome`, is not a key and SHALL keep its own spelling.

Configuration types SHALL stay kebab-case, since what the loader reads from TOML is not what the printer emits.

#### Scenario: A notifier dot-accesses every key of a report
- GIVEN a run that parked a divergence
- WHEN `neverest sync --json` prints its report
- THEN every key of the document is camelCase, `dryRun` and `outstandingConflicts` among them, and none carries a hyphen or an underscore

### Requirement: Every key is printed
Every output type neverest prints SHALL print every key it declares: a list as `[]` when empty, an absent value as `null`, a bool as `false`, never leaving a key out. Its schema SHALL mark each such key required. State neverest writes for itself and reads back (the sidecar beside the store) is not output and MAY leave keys out.

#### Scenario: A quiet run still names its lists
- GIVEN a sync that parked, refused and conflicted nothing
- WHEN `neverest sync --json` runs
- THEN the report holds `conflicts`, `parked` and `errors`, each `[]`, and `purged` as `null` when no sweep ran

### Requirement: A conflict body is printed as stored
`conflict show` SHALL print each of `base`, `local` and `remote` as the family prints a vCard or an iCalendar body (pimalaya-cli cairn/spec/output.md, *Bytes are never raw under JSON*), as cardamum's card read does: `{contents, encoding}`, `contents` the text and `encoding` `utf-8` when its stored bytes are valid UTF-8, else their base64 (RFC 4648 standard alphabet, padded) and `encoding` `base64`, `null` when the store holds none. One field SHALL keep one shape whatever the bytes. It SHALL NOT decode a body lossily under `--json`: the bytes printed are the ones a decision picks between. The text form MAY show such a body with its invalid sequences replaced, saying so. The conflict itself SHALL print as `conflict list` prints its row, the item's handle on the source under `remoteId`.

#### Scenario: A Latin-1 card in conflict
- GIVEN a conflicted card whose local body is a vCard 2.1 in Latin-1
- WHEN `neverest conflict show <id> --json` runs
- THEN `local` is `{contents, encoding: "base64"}`, `contents` the exact bytes in base64, and a UTF-8 side reads `{contents, encoding: "utf-8"}`

### Requirement: The mode is data
`check` and `init` SHALL print the account's mode as an object, `{sources, targets, oneWay, retain}`: the source and target names sorted, whether the sources are authoritative, and whether the store keeps bodies. The sentence describing it SHALL be its text form only.

#### Scenario: A caller reads the direction of an account
- GIVEN an account with a CalDAV source and a target `backup`
- WHEN `neverest check --json` runs
- THEN `mode` is `{"sources": ["caldav"], "targets": ["backup"], "oneWay": false, "retain": false}`

### Requirement: A change one endpoint made crosses in the run that observed it
An item an account's two endpoints both hold, whose body exactly one of them changed, SHALL be delivered to the other in the run that observed the change. The changed body SHALL be hydrated before the pushing passes, so the run that reports the update is the run that makes it.

A run SHALL NOT report a write it has not attempted. An update itemized from the projection and left unsent by a hydration that never happened is such a write, and it reads as applied on every run while the two endpoints stay divergent.

Under a declared authority there is no divergence to reconcile, so an item both endpoints changed SHALL hydrate the deciding side's body and overwrite the other in that same run. Under shared authority the same item is a divergence, and the conflict path merges or parks it instead.

The far side's own permission decides what may be hydrated for it: `item.create` for a copy it does not hold, `item.update` for a body it does.

#### Scenario: A mirror delivers an edit in one run
- GIVEN one source and one target holding the same item
- WHEN the item's body changes on the source and the account is synced once
- THEN the target holds the new body, the update is reported once, and the next run reports nothing

#### Scenario: A one-way account overwrites in the run that saw the difference
- GIVEN a `one-way` account whose source and target hold one item under two bodies
- WHEN the account is synced once
- THEN the target carries the source's body, nothing is counted as a conflict, and the run exits 0

### Requirement: A refused write names the item as a person knows it
A write a remote refused SHALL be reported under the item's own identity, never under a handle the engine minted for its own bookkeeping. A create is staged under its provisional handle, U+0001 followed by the item's link id (pimdir SYNC §2), the item having no handle on the side it is being created on: the report SHALL name it by that link id, and the U+0001 SHALL NOT reach the report.

The hunk the refused write was derived from SHALL be taken back whatever its kind, a copy and a frontend's add included: both are named by the link id the provisional handle carries, so a run whose appends were all refused SHALL report no hunk applied.

#### Scenario: A read-only collection refuses every append
- GIVEN a target collection the server serves read-only
- WHEN a run tries to copy items into it
- THEN each refusal names its item plainly, the item patch is empty, and the run exits 3

#### Scenario: A frontend's add refused
- GIVEN an event a frontend queued into a Google calendar, which Google refuses
- WHEN a sync pushes it
- THEN the refusal names the event by its `UID`, never an empty `id`, and the add is not among the hunks applied

### Requirement: The summary is the format's typed row
The summary written for an item SHALL be io-pimdir's `PimdirSummary`, the row of the kind's summary table with the addresses it names (pimdir STORAGE Annex A): `mail_summary` for `message/rfc822`, `contact_summary` for `text/vcard`, `event_summary`, `task_summary` or `journal_summary` for `text/calendar`, plus `item_address`. A reader renders a list from those rows and never parses a blob. Flags are not in the summary. Both the listed (`Meta`) and the streamed (`Full`) paths of mail SHALL emit the same row, the streamed path carrying the message's known octet length as `size` rather than the header prefix it read.

### Requirement: The derivations are the format's
A link id, a summary and a sort key SHALL be what pimdir STORAGE Annex A gives, derived by io-pimdir, so the schema cannot drift from the format's by a field or a spelling: `summary::derive` at the `Full` tier from the streamed bytes, and at the `Meta` tier `summary::mail::derive_meta` over the header fields the source serves raw. This crate SHALL define no summary struct, and SHALL parse no `Message-ID`, `In-Reply-To`, `Subject`, `Date` or address field itself.

A message is named at the `Meta` tier as its body would name it (Annex A.1, pimdir SYNC §4), by the same read on a listing and on a `Meta` fetch:

- **IMAP** SHALL read `BODY.PEEK[HEADER.FIELDS (Date From To Cc Bcc Subject Message-ID In-Reply-To Content-Type)]` with `RFC822.SIZE`, and SHALL NOT read an `ENVELOPE`, whose addresses are the server's parse.
- **Gmail** SHALL lay the metadata headers out as the header block they came from, each field from its first entry, a folded value unfolded by the derivation.
- **Graph** SHALL select `internetMessageHeaders` on every read naming a member: the whole-folder delta, a band page, a listing by arrival, the summary batch of a delta or of the pass, and a `Meta` fetch. A row listing header fields SHALL be named off them, the fields a summary reads (`Message-ID`, `In-Reply-To`, `Subject`, `From`, `To`, `Cc`, `Bcc`, `Date`, `Content-Type`) laid out as the header block they came from, each from its first entry of that name. Its size and its attachment mark stay Graph's own (`PidTagMessageSize`, `hasAttachments`), the mark being the top-level `Content-Type`'s where Graph states none. The pass closing a round under a scope names no member, and SHALL select ids, dates and flags alone.

  A row listing no header field, its read having selected none (a delta link made before they were selected carries its own `$select`) or Graph holding none for the message, SHALL be named from `subject`, `sentDateTime` and the addresses as Graph states them, its `internetMessageId` read as a `Message-ID` field is, by io-pimdir's `message_id`: values of the server's own making, which stand until the body is read (Annex A.1). Such a member whose decoded summary differs from Annex A.1's reading is minted apart from its own message where another reading wrote its row, a missed dedup the run reports `disagreed` when the row is its own create.

  The scope of a Graph listing is still read off `sentDateTime`, the member Graph filters and orders by; the `date` a row is named by is its `Date` field's, which the engine's own scope check reads.

A hint already derived (the `Message-ID` an add is searched by) SHALL be compared as it stands, never parsed again.

The streamed tier reads a header prefix rather than the whole body, so it SHALL restate the message's octet length as the summary's `size`, and SHALL leave the attachment mark and the invitation unknown: neverest SHALL NOT walk a body's parts. io-pimdir's write walks each body entering the store once, from its blob, and states the mark and the invitation over the writer's summary; a body the account already holds takes those of a row holding it (Annex A.1, io-pimdir `ca4c589`). That walk reads the last part of a multipart no closing delimiter ends, so a message cut short is marked as the same message closed is, and each entity's `Content-Type`, `Content-Disposition`, `Content-Transfer-Encoding` and `Content-ID` from their first occurrence (Annex A.1 as storage-draft-11 states it). A body served from a stored blob SHALL be read up to its header block alone. The `Meta` tier SHALL carry every `From`, `To`, `Cc` and `Bcc` address the backend surfaces, so the address rows the two tiers write agree.

#### Scenario: A Message-ID wrapped in comments
- **GIVEN** the message of pimdir vectors/fixtures/mail-id-comment.eml, whose field reads `(via <relay@example.net>) <comment-1@example.org> (trailing)`
- **WHEN** it is listed over IMAP, over Gmail or over Graph
- **THEN** it is keyed `comment-1@example.org`, the key vectors/summaries.json states and the one its body derives

#### Scenario: A folded subject and no Message-ID
- **GIVEN** the message of vectors/fixtures/mail-folded-subject.eml
- **WHEN** it is listed over IMAP, over Gmail, or over Graph by a row listing its header fields
- **THEN** its `alt:` key holds the subject unfolded, its two encoded words joined and its padding trimmed, as the vectors state

#### Scenario: A Graph row listing its header fields
- **GIVEN** a Graph row whose `internetMessageHeaders` lists the fields of one of the ten fixtures under tests/vectors/fixtures, repeated names kept in their order, beside a `subject`, a `sentDateTime`, a `from` and an `internetMessageId` stating another message
- **WHEN** the row is named
- **THEN** its key, its `subject`, `date`, `sender`, `sender_name` and `in_reply_to` are those vectors/summaries.json states for the fixture, a field stated twice read from its first entry (src/msgraph/client.rs `a_row_listing_its_header_fields_is_named_as_the_vectors_state`)

#### Scenario: A Graph row listing no header field
- **GIVEN** a Graph row carrying no `internetMessageHeaders`
- **WHEN** the row is named
- **THEN** it is named from `internetMessageId`, `subject`, `sentDateTime` and `from` as Graph states them, the `Message-ID` of vectors/fixtures/mail-id-comment.eml still keyed `comment-1@example.org`

#### Scenario: A non-ASCII subject reaches a reader readable
- GIVEN a message whose `Subject:` is RFC 2047 encoded
- WHEN either tier summarises it
- THEN the summary's subject holds the decoded text, not the encoded word

#### Scenario: The mark of a downloaded body is its parts'
- **GIVEN** a `multipart/mixed` message holding only an inline image, and a `multipart/signed` one holding an attachment two levels down
- **WHEN** a sync downloads their bodies, or files them again in a second folder
- **THEN** the first is marked as attaching nothing and the second as attaching, the store's write having walked each body once (tests/attachments.rs); the streamed summary itself names them as the whole message does and states no mark (src/sync/remote.rs `scan_tests`)

### Requirement: Link id and summary are per-kind, resolved at one seam
The cross-collection link id and the typed summary SHALL be produced by io-pimdir's derivation for the media type, selected from the source's declared kind at a single dispatch point. `message/rfc822` keeps the bare `Message-ID` identity with its `(subject, date, sender)` (`alt:`) fallback. `text/vcard` and `text/calendar` use the bare vCard / iCalendar `UID`, falling back to the content hash (`hash:`) for a body carrying no `UID`; an iCalendar `RECURRENCE-ID` SHALL NOT enter the link id, so a recurrence override stays the same item.

The `text/calendar` sort key SHALL be the item's start resolved to RFC 3339 in UTC (`DUE` then `DTSTART` for a `VTODO`, `DTSTART` otherwise), read through the `VTIMEZONE` the resource itself carries, so an agenda reads chronologically without the store holding a time zone database.

### Requirement: A collection is keyed by its backend id and named by its display name
Every hub collection id, every wire call and the account's collection filter SHALL be built from the backend's own id: the mailbox name on IMAP and on Graph, the label name on Gmail, on DAV the path segment of a collection directly under the home set, as the server served it (A DAV key keeps the segment the server served), and the href's path, starting with a slash, of one outside it (io-webdav's id; Google lists every calendar as `/caldav/v2/<calendar>/events/`, hub id `caldav//caldav/v2/<calendar>/events`). A display name SHALL NOT address a collection, being optional, mutable and free to collide. The default calendar a scheduling inbox names SHALL be keyed as the listing keys it.

The store SHALL still carry what the collection is called: `set_collection_name` runs beside every `ensure_collection` with the name without the namespace the hub id carries. A DAV source SHALL take it from `DAV:displayname`, falling back to the id when it is blank or absent; every other source's id is already its name. Where two sources meet, the first in declared order with a non-blank name wins. Naming SHALL NOT fail a run: nothing keys on the column, so a refused write is logged and the sync continues.

### Requirement: Google contacts and calendars are native sources
A source MAY declare a `gpeople` backend, syncing Google contacts as `text/vcard` through the People API, or a `gcal` backend, syncing Google calendars as `text/calendar` through the Calendar API. Both SHALL take `tls`, `alpn` and an `auth.token` bearer only, neverest running no OAuth flow, and SHALL carry no send channel.

A `gpeople` source SHALL have one collection, `contacts`, holding every connection: a contact group is a label a person carries any number of, not a folder. A `gcal` source SHALL have one collection per calendar of the user's calendar list, keyed by calendar id and named by the user's own name for it.

Enumeration SHALL be the API's sync-token listing, the token as the engine's opaque checkpoint: a resumed listing reports what changed and names removals, a full one reports removals by absence, and an expired token (HTTP 410) restarts a full listing. A resumed listing SHALL ask what the listing that issued its token asked (`maxResults`, `showDeleted`, no `singleEvents` nor `timeMin`), the token aside. An expired token is a provider event the user cannot act on, some calendars (the public holiday ones) refusing every token: it SHALL be said at debug, the calendar listed whole again.

A `gpeople` item SHALL be one person, its `etag` the revision, projected through io-gpeople's `vcard` feature. Its vCard `UID` rides the projection's `clientData` stash, so it keeps the identity it arrived with; a create whose stash People did not keep SHALL be deleted again and refused. An update SHALL carry the etag, which People enforces, and SHALL merge back, in each field of its mask, what People holds there and a vCard cannot show (io-gpeople's `merge_unprojected`: an organization or occupation after the first, a relation other than spouse or child, a custom type, a sub-field without a vCard slot, the `clientData` entries other clients own), a masked update replacing the field whole. A property too long for the stash, which the projection leaves out, SHALL be named in a warning.

A `gcal` item SHALL be one recurring series with its modified instances, or one lone event, keyed by the series' event id, its revision joining the etags of the series and of its instances so an edit to any of them moves it, projected through io-gcal's `ical` feature. The body of an item SHALL be read from the calendar's last listing when that listing returned it whole (a full listing returns every event with its modified instances, a resumed one a lone event), sending no request, and read again otherwise; any write to the calendar SHALL forget what its listing returned. A create SHALL import an object carrying a UID, so the UID survives as the event's `iCalUID`. An update or delete SHALL be refused when the joined revision moved, then be guarded by the series event's own etag. A series SHALL be written whole, as "A series is pushed whole or not at all" says, and a floating time anchored, as "A floating time is anchored before Google or Graph takes it" says.

#### Scenario: A series edited on one instance moves its item
- GIVEN a weekly series with one modified instance, synced into the store
- WHEN the instance is edited on Google and the account synced again
- THEN the series' item has a new revision and its body carries the edited instance

### Requirement: A series is pushed whole or not at all
A write to a Google Calendar or Graph calendar SHALL carry every occurrence the document sets apart from its master, or SHALL be refused: a write landing as the master alone loses the rest on the server, and the next read loses it in the store. Both providers take an occurrence through its instance, found among the instances listed two days either side of the occurrence's `RECURRENCE-ID` by its original start, compared as an instant: a `RECURRENCE-ID` or `EXDATE` value SHALL be read in its `TZID`, or as UTC, and matched against the instance's original start as the instant it names, never by the wall time either writes, Google writing an original start in the calendar's zone or in UTC whatever the series' zone. A date SHALL match the instance's original date. A wall time in no zone the time zone database names (floating, a `TZID` it does not know, an intent's bare `recurrence_id`) SHALL match the time the provider writes, or the one the series' zone shows (on Google the instance's own `timeZone`, on Graph the series' `start` zone).

Graph SHALL take the master first (created, or patched with what changed), then delete the instance of each `EXDATE` and of each override marked `STATUS:CANCELLED`, an instance already gone counting as deleted, and patch the instance of each other override with what it sets differently from its series carried to that occurrence (io-msgraph's `instance_update_from_ical` against the document's master, the instance's own times standing in for the master's): what the override leaves out SHALL stay as the series has it, never be cleared, a listed instance stating Graph's own `showAs`, `sensitivity` and `importance`, which an override stating no `TRANSP`, `CLASS` or `PRIORITY` would null. An instance Graph holds as an exception already SHALL be patched with what the override sets differently from it, so an override undoing one of its edits sends it. Google SHALL take the master first (imported, inserted or updated), the `EXDATE`s riding its recurrence, then update the instance of each override with the override merged onto it (io-gcal's `from_ical` reading the override alone, `merge` keeping the instance's identity), with the master's `sendUpdates`.

An override of an occurrence the series does not have, and one of an occurrence and every later one (`RANGE=THISANDFUTURE`), which neither provider takes through an instance, SHALL be refused coded `unsupported`, naming the occurrence: the latter before anything is written; the former once the master is created, which is then deleted again. A create whose occurrences do not all land, for any reason, SHALL be deleted again and the write refused, so the store keeps its body and the next run pushes it again. A created series with occurrences set apart SHALL tell its attendees nothing until they all landed, so a create deleted again mails nobody however many runs push it: Graph mails attendees on any write carrying them, so the master SHALL be created without its attendees, which a patch of the master adds once the occurrences landed; Google SHALL insert the master without its attendees with `sendUpdates=none`, write the overrides with `sendUpdates=none` (an override naming the series' attendees leaving them to the series), delete a series taken back with `sendUpdates=none`, and add the attendees last by a patch with `sendUpdates=all`. The patch announces the series and each override (one mail each, observed live on 2026-11-17); a series whose patch fails SHALL be deleted again, on Google with `sendUpdates=all`, which tells nobody the patch did not reach. An update failing on an occurrence SHALL be refused; the next run reads the master the update moved as the server's edit. A write of occurrences SHALL report the revision read after them.

An occurrence the server holds and the document no longer sets apart (an override dropped, on Graph an `EXDATE` removed too) is not written back: Graph restores no cancelled occurrence. Both sources SHALL therefore declare `calendar.occurrence.update` `partial`, the detail naming what such an update leaves on the server.

#### Scenario: A series with an exclusion and a moved occurrence reaches Graph
- **GIVEN** a daily series queued with an `EXDATE` on its third occurrence and an override moving its second
- **WHEN** a sync pushes it to a Graph calendar
- **THEN** the master is created, the third instance deleted and the second patched to its new time, and the item records the master's revision read after them

#### Scenario: A moved occurrence reaches Google
- **GIVEN** the same series pushed to a Google calendar
- **WHEN** a sync imports it
- **THEN** the second instance is updated to its new time and the item records the series' joined revision read after it

#### Scenario: An override in a zoned series reaches Google
- **GIVEN** a weekly series from 2026-12-07 at 10:00 `Europe/Paris` whose override moves the occurrence of 2026-12-21 (`RECURRENCE-ID;TZID=Europe/Paris:20261221T100000`) to 14:00
- **WHEN** a sync pushes it to a Google calendar listing that instance's original start as `2026-12-21T09:00:00Z`
- **THEN** the override updates that instance and the series lands whole

#### Scenario: An override renaming one occurrence reaches Graph
- **GIVEN** a series whose override changes the `SUMMARY` of one occurrence alone, and a Graph instance listed with `importance` high, `sensitivity` private and `showAs` free
- **WHEN** a sync pushes it to a Graph calendar
- **THEN** the instance is patched with its subject alone, its importance, sensitivity and free/busy left as the series has them

#### Scenario: An override of no occurrence
- **GIVEN** a series whose override names an occurrence its rule does not produce
- **WHEN** a sync pushes it to Google or Graph
- **THEN** the series created is deleted again, the write is reported refused, coded `unsupported`, naming the occurrence, and the store keeps the body for the next run

### Requirement: A floating time is anchored before Google or Graph takes it
Neither Google Calendar nor Graph stores a floating time (a `DTSTART`, `DTEND`, `RECURRENCE-ID`, `EXDATE` or `RDATE` naming neither a `TZID` nor UTC). Before a create or an update, every floating date-time of the document's events SHALL be anchored: on Google in the zone the calendar states (its calendar list entry's `timeZone`), as a `TZID`; on Graph, which states no zone for a calendar, and on Google when the calendar states none, in the user's zone, the one the search index states as its floating zone ("The store's owner builds its search index"): the IANA name of the zone `TZ` names, else of the system's, as a `TZID`, and UTC when it has no IANA name; with a warning naming the write and the zone. A floating 10:00 SHALL so stay 10:00 where the person reads it. A series whose `DTSTART` was floating SHALL have the floating `UNTIL` of its `RRULE` and `EXRULE` rewritten as the UTC instant its wall time names in the same zone (RFC 5545 §3.3.10 asks a rule bounding a zoned start to state `UNTIL` in UTC), so the series ends where it did. A date is no floating time. The write SHALL then report no revision, so the next sync reads the event back and the store holds what the server does.

#### Scenario: A floating meeting queued on a Google calendar
- **GIVEN** an event queued at a floating 10:00 on 2026-11-26 into a Google calendar whose zone is `Europe/Paris`
- **WHEN** a sync pushes it
- **THEN** Google takes it at 10:00 `Europe/Paris`, the run reports no refusal, and the next sync reads it back into the store

#### Scenario: A floating meeting queued on a Graph calendar
- **GIVEN** `TZ=Europe/Paris` and an event queued at a floating 10:00 on 2026-11-26 into a Graph calendar
- **WHEN** a sync pushes it
- **THEN** Graph takes it at 10:00 `Europe/Paris`, not at 10:00 UTC (11:00 in Paris), and the next sync reads it back into the store

### Requirement: Gmail is a native mail source
A `gmail` source SHALL open protocol-direct over io-gmail and sync `message/rfc822`. Its collections SHALL be the user labels and the `INBOX`, `SENT`, `DRAFT`, `SPAM` and `TRASH` system labels, keyed and named by label name, as IMAP exposes them, so a Gmail endpoint and an IMAP endpoint on one account pair their labels. A label id SHALL NOT key a collection: the driver pairs endpoints and creates missing collections by key, and an opaque id would never meet the IMAP name.

`UNREAD`, `STARRED` and `IMPORTANT` SHALL be flags (`\Seen` by absence, `\Flagged`, `$Important`), never collections, and the categories SHALL NOT be collections. Any other flag SHALL be dropped on push, a keyword turned label filing the message in a new collection. A Gmail source SHALL declare `mail.flags.important` `full` and `mail.flags.junk` `none` (spam being the `SPAM` collection, reached by a move), so a store reads `$Important` and `$Junk` under their own rows (pimdir STORAGE Annex B.1), and `mail.flags.keywords` `none`, naming `$Important` as the one keyword kept: a frontend's change of any other keyword on a Gmail label, a `set-flags` or an `add`, `copy` or `move` landing a message carrying one, SHALL be refused at the store's gate rather than queued and dropped on push, and one queued before SHALL be parked by the drain. Every reader of a store at this version reads the two own rows (io-pimdir 0.9 on), so `$Important` needs nothing of `mail.flags.keywords`. Other sources state neither row, their two flags read under `mail.flags.keywords`.

A handle SHALL be the Gmail message id, stable across labels. Enumeration SHALL carry the mailbox `historyId` as the checkpoint, taken before a full round's first list. A delta SHALL be the mailbox history, read once a run with no label and projected on the collection's label (a record naming the label, a message the collection holds, an arrival naming no label), each touched message's current labels read before it is reported, and an expired `historyId` (404) SHALL restart a full round. A spam or trashed message SHALL be a member of `SPAM` or `TRASH` only.

While a label's round is open, a run resuming it SHALL list the round's delta head before its next page (pimdir SYNC §4, §5): the mailbox history since the round's history id, read once a run and projected on the label as a delta is, answered as a delta with the history id Gmail holds now; an expired history id (404) or a checkpoint that is no history id SHALL reject the head, which restarts the round; a band head SHALL be answered none. A page of the label listed after its head SHALL read every message the head touched (read again or stated deleted) no earlier than the head, the run's own read of it since the head serving, its labels then deciding its membership and flags; flag listings read before the head SHALL be read again. A message a page reads that no longer carries the label SHALL be left out, as one gone (404) is, a round page stating no removal.

A delete from a label SHALL remove that label (from `INBOX` it archives), a move SHALL swap two labels in one modify, and only a delete from `TRASH` SHALL delete permanently. Gmail refuses `DRAFT` and `SENT` in any label change, added or removed (400 `Invalid label`, a batch failing whole; checked live on 2026-11-17), while an import into them is taken: a delete from either SHALL add `TRASH` alone, a move out of either to `TRASH` likewise, and any other move into or out of either SHALL be refused coded `unsupported` before any request. A write on a message gone already (404) SHALL count as done. An added message SHALL go through `messages.import`, and an answered id that does not exist SHALL be re-found through history by `Message-ID` rather than trusted. A system label SHALL NOT be created or deleted.

Auth SHALL be a bearer access token only, as for every other HTTP source.

#### Scenario: A message arriving while the Inbox's round is open
- **GIVEN** a Gmail Inbox whose round's first page landed from history id 500, the round left open
- **WHEN** a message arrives in the Inbox and the account is synced again
- **THEN** the run lists the history since 500 first, the message lands before the round's next page, and the round closes on the history id that head read, which the next delta lists from

#### Scenario: A keyword set on a Gmail message
- **GIVEN** a store holding a Gmail message, its source declared
- **WHEN** a frontend queues `$Custom` on it, then `$Important`
- **THEN** `$Custom` is refused at the gate naming `mail.flags.keywords` and Gmail's detail, and `$Important` is queued with no partial support to show

### Requirement: Every source declares its capabilities before the drain
At every run, before it drains an action, neverest SHALL declare each source it runs in the store (pimdir STORAGE §15.6): every Annex B capability of the source's kind, from its backend and its configured rights alone, `none` with the reason, and the `calendar.component.vtodo` rows of `A calendar its server keeps from tasks is declared so`, the source-wide ones from the configuration and the per-calendar ones from the sidecar rather than the network. A right the configuration withholds SHALL be `none`. A verb the connector refuses SHALL be `none` too: an item move between Google calendars, Graph calendars or Graph contact folders, which the gate would otherwise pass and every push reject. A one-way source SHALL support no item or flag write; a collection action on it (`collection.create`, `collection.rename`, `collection.delete`) SHALL be declared only where its switch allows it, and the scheduling intents (`mail.submit`, `calendar.reply`, `calendar.cancel` and their options) SHALL stay allowed, being requests to the provider rather than writes the copy would undo. A JMAP source stays undeclared.

#### Scenario: A source is declared with no network
- **GIVEN** a Graph source whose token command fails
- **WHEN** neverest runs
- **THEN** the source's declaration is in the store before the credential is read

### Requirement: An intent is performed by the source it names
A `submit` SHALL be sent by the source its payload names, and left pending for another run when it names a source this run does not perform.

### Requirement: A sent copy is filed once sent
A `submit` asking for `copy` SHALL file the message in that collection only after the send succeeds: natively where the provider files sent mail itself (the Gmail and Graph backends, and an IMAP source whose IMAP and SMTP hosts are a pair neverest knows to file what the SMTP server sends in that mailbox's Sent), otherwise by replacing the intent with an `add` of the message, `\Seen` set, drained in the same run (pimdir STORAGE §15.5, Annex B.2).

A copy staged where the server files its own lands on that copy (pimdir SYNC §6) and pushes its staged `\Seen` onto it, which marks a message sent to oneself read where the provider keeps one message for both mailboxes; the provider's copy, listed by the next round, is the one the store binds.

#### Scenario: A message sent to oneself through a filing pair
- **GIVEN** an IMAP source whose SMTP server files what it sends, and a queued `submit` to the account's own address with `copy` set to its Sent
- **WHEN** the account syncs until the message is delivered
- **THEN** the inbox copy is unread, Sent holds the message once, and the store binds that one copy

#### Scenario: A server that files no copy
- **GIVEN** an IMAP account whose SMTP server files nothing in Sent
- **WHEN** a `submit` with `copy` set to Sent is performed and the account syncs
- **THEN** neverest uploads the message to Sent once, seen

### Requirement: A performed intent leaves a receipt
Every intent neverest performs successfully (`submit`, `calendar-reply`, `calendar-cancel`, `collection-create`, `collection-rename`, `collection-delete`) SHALL be acknowledged with its receipt (io-pimdir `acknowledge_action`, or `replace_action` for a `submit` whose copy neverest files), so its producer reads it applied rather than withdrawn (pimdir STORAGE §15.4, §15.5). The receipt SHALL name no `seq`: a created, renamed or deleted collection and a provider-filed copy arrive with a later listing, a copy neverest files is an `add` followed by its own id, and an invitation leaves its event where the producer found it. A parked or pending intent keeps its row and gets no receipt.

#### Scenario: A send and a folder followed by their producer
- **GIVEN** a producer that queued a `submit` and a `collection-create` and kept their ids
- **WHEN** a sync sends the message and creates the folder
- **THEN** `action_status` reads both `Applied` with no `seq`, where a withdrawn row reads `Unknown`

### Requirement: Google Calendar notifies as the resource asks
A new event carrying a UID SHALL be inserted with it as `iCalUID` and notify its attendees when the resource is scheduled (pimdir Annex B.1) and the account organises it (no organizer, the calendar itself or the account's address); otherwise it SHALL be imported, notifying nobody. An update SHALL notify the attendees only when the resource is scheduled; a delete SHALL notify them when the event has attendees.

### Requirement: A calendar intent is performed through the provider's verbs
Neverest SHALL perform the `calendar-reply` and `calendar-cancel` intents (pimdir STORAGE Annex B.2) at the start of each sync, one-way included, through the source the payload names, on the item that source binds: Graph through the event's `accept`, `tentativelyAccept`, `decline` and `cancel` actions with the comment, Google by answering the account's own attendee or deleting the meeting it organises, both with `sendUpdates=all`. A performed intent SHALL be acknowledged; a refusal (the account attends nothing, or organises nothing), a 4xx, a gone item or an undecodable payload SHALL park it; a 408, 412, 429, 5xx, transport error or an item not pushed yet SHALL leave it pending. Each intent SHALL be marked in flight in the store's local/ before its first request and the mark cleared once its outcome is recorded: an intent a killed run left marked may have told the attendees, so the next run SHALL park it unconfirmed rather than perform it again, and an outcome that cannot be recorded SHALL keep the mark and stop the drain. Each attempt is reported under `intents`. Both SHALL be declared on each calendar the source syncs and `none` source-wide; CalDAV declares them `none`.

#### Scenario: An invitation answered on Google
- **GIVEN** an event of a Google calendar the calendar attends
- **WHEN** a `calendar-reply` `TENTATIVE` is queued on it and the account syncs
- **THEN** the intent is acknowledged and the next sync reads `PARTSTAT=TENTATIVE`

### Requirement: An intent limited to one occurrence acts on that instance
A `calendar-reply` or `calendar-cancel` naming a `recurrence_id` (pimdir Annex B.2, `YYYYMMDD` or `YYYYMMDDTHHMMSS[Z]` in the form of the series' `DTSTART`) SHALL be performed on that occurrence alone, through the same verbs on its instance: the instance of the series whose original start the projection writes as that `RECURRENCE-ID` (the wall time in the series' zone, a date, or a UTC stamp), among those the provider lists two days either side of its date. An occurrence not found, or a series that is not one, SHALL park the row; a malformed value SHALL park it. `calendar.reply.occurrence` and `calendar.cancel.occurrence` SHALL be declared as their intents are: on each calendar a Graph or Google source syncs (`partial` for a Google cancel, which carries no comment), `none` source-wide, and `none` on CalDAV.

An occurrence moved more than two days from its original date is not found, the providers listing instances by their current times.

#### Scenario: One meeting of a series declined
- **GIVEN** a weekly series on a Graph calendar the account attends
- **WHEN** a `calendar-reply` `DECLINED` naming one occurrence's `RECURRENCE-ID` is queued and the account syncs
- **THEN** that instance is declined, the series and its other occurrences unchanged

### Requirement: A collection is created through the queue
Neverest SHALL perform the `collection-create` intent (pimdir STORAGE Annex B.2) at the start of each sync, before the submissions and the calendar intents and before the listing, so the collection is listed by the same run: through the source the payload names, or a source of the anchor's namespace for a payload naming none. IMAP SHALL create the mailbox `name` at the top level or under `parent`, joined by the delimiter the parent's `LIST` row states, refusing a parent with `\Noinferiors` or a flat namespace; Gmail SHALL create the label `parent/name`, refusing a system label as parent or as name; CalDAV and CardDAV SHALL create a collection displayed as `name` and keyed by a path segment made from it, refusing a parent, DAV collections not nesting. A collection the server already lists there SHALL be success, so a repeated intent creates nothing. A refusal, a parent outside the performer's namespace or an undecodable payload SHALL park the row; any other failure SHALL leave it pending. Each attempt is reported under `intents`.

`collection.create` SHALL be declared on each collection an IMAP, Gmail, CalDAV or CardDAV source syncs, `none` source-wide, and `none` where the backend creates no collection (Graph mail, contacts and calendars, Google Calendar, Google People) or `collection.create` is disabled; under `one-way` it stays declared, a creation being asked for rather than derived.

#### Scenario: A folder and its child
- **GIVEN** an IMAP account whose store holds `INBOX`
- **WHEN** a `collection-create` `Archives` anchored on `INBOX` is queued and the account syncs, then one named `2026` under `Archives`
- **THEN** the server lists `Archives` and `Archives/2026`, each listed by the run that created it, and the same intent queued again is acknowledged without creating anything

### Requirement: A collection is renamed or deleted through the queue
Neverest SHALL perform the `collection-rename` and `collection-delete` intents (pimdir STORAGE Annex B.2) with the `collection-create` ones, in queue order, at the start of each sync and before the listing, so the collection is listed as it now stands by the same run: through the source the payload names, on the collection the row is queued on, which the payload does not restate.

A rename SHALL keep the collection's parent: IMAP SHALL `RENAME` the mailbox to `name` under the parent its name holds, joined by the delimiter its `LIST` row states, the mailboxes under it following; Gmail SHALL rename the label to `parent/name` and every label nested under it, the deepest first, Gmail nesting a label by its name alone; CalDAV and CardDAV SHALL set the display name by `PROPPATCH`, the key kept. A collection already so named, or one gone where a collection of the new name stands, SHALL be success; a sibling already named `name`, a name blank or holding the separator, `INBOX`, a Gmail system label and a collection gone with nothing in its place SHALL park the row.

A delete SHALL remove the collection with the items it holds and never another: a collection the server lists under it (an IMAP mailbox below it, a Gmail label nested in it) SHALL park the row naming them, deleting nothing. IMAP SHALL `DELETE` the mailbox; Gmail SHALL delete the label, its messages staying in All Mail as Gmail deletes no message with its label; CalDAV and CardDAV SHALL delete the collection. A collection already gone SHALL be success; `INBOX` and a Gmail system label SHALL park the row. A refusal, a collection outside the performer's namespace or an undecodable payload SHALL park the row; any other failure SHALL leave it pending. Each attempt is reported under `intents`.

On an account syncing its source alone, the store SHALL follow a performed intent at once. A rename giving the collection a new id (IMAP and Gmail key a collection by its name) SHALL move it and every collection under it to their new ids through `rename_collection`, each named as its next listing names it, so their items keep their bindings rather than leaving under the old id and arriving again under the new one; a DAV rename SHALL rename the collection in place. A delete SHALL drop the collection from the store once its row is acknowledged, its receipt going with it, a row still queued on it keeping it there. On a paired account the store SHALL NOT follow: a pair settles the collections its endpoints list itself (see the collection requirement of a pair), and a rename SHALL park, the pair mirroring collections by name and reading a rename as a delete and a create.

`collection.rename` and `collection.delete` SHALL be declared as `collection.create` is: on each collection an IMAP, Gmail, CalDAV or CardDAV source syncs and `none` source-wide, `none` where the backend renames or deletes no collection (Graph mail, contacts and calendars, Google Calendar, Google People), and stated under `one-way`, being asked for rather than derived. Each SHALL be `none` on a collection stating a role (an inbox, a sent or trash folder, a Gmail system label, a default calendar or address book), which the provider keeps. `collection.rename` SHALL be `none` on a paired account, and where the source's `collection.rename` switch is off: unset, it reads the account's `one-way`, off on a one-way account (the detail naming the key) and on otherwise; `collection.rename = true` or `false` on the source sets it whatever the account. `collection.delete` SHALL be `none` where `collection.delete` is disabled, its default, and `partial` on Gmail, whose delete leaves the messages in All Mail.

#### Scenario: A rename on a one-way account
- **GIVEN** a one-way account whose IMAP source sets no `collection.rename`
- **WHEN** `neverest account check` runs, or a frontend asks to rename one of its folders
- **THEN** `collection.rename` reads `none`, its detail naming `collection.rename`, and the frontend's rename is refused up front; with `collection.rename = true` it reads as on a two-way account

#### Scenario: A folder renamed, then deleted
- **GIVEN** an IMAP account syncing its source alone, a store holding `Archives` with one message and `Archives/2026`, and `collection.delete` enabled
- **WHEN** a `collection-rename` `Kept` is queued on `Archives` and the account syncs, then a `collection-delete` on `Kept`
- **THEN** the server lists `Kept` and `Kept/2026`, the store holds them with the message under `Kept` and nothing under `Archives`, and the delete parks naming `Kept/2026`, deleting nothing
- **AND WHEN** the parked row is cancelled and deletes are queued on `Kept/2026`, then on `Kept`
- **THEN** the server and the store hold neither

### Requirement: A source's collections push one at a time
Scans and fetches MAY run over several connections, but no two collections of one source SHALL push at once: the two halves of a move derive from the store, and overlapping pushes would both read the create as pending, the target uploading the member while the source relocates it (pimdir SYNC §5). The second to push reads what the first delivered.

#### Scenario: A move bounced between two mailboxes
- **GIVEN** a message in one IMAP mailbox and four connections
- **WHEN** a move to another mailbox is staged and the account syncs, several times back and forth
- **THEN** the message is held once, on the server and in the store, after every run

### Requirement: Graph mail moves, copies and adds drafts
An `msgraph` source SHALL relocate a move's remove with `message_move` and SHALL deliver a create carrying an origin (pimdir SYNC §4) by `message_copy` from it, accepted under the id the copy answers with. Graph message ids change on a move or a copy, and immutable ids SHALL NOT be asked for, which would change the ids stores already bind: like an IMAP `MOVE`, a relocation reports no handle, and the target's next enumeration lists the member under its new id, whose fetch lands the pending create by its `Message-ID` (pimdir SYNC §6). A failed copy SHALL be rejected rather than uploaded.

Graph creates every MIME message as a draft, so an append SHALL land in the well-known Drafts folder only, through `message_create_mime`, its `\Seen` and `\Flagged` patched in after, and SHALL be refused anywhere else as it stands, never filed there as a draft, its failure coded `unsupported` (the code the store's gate gives the same write refused at enqueue), so the store holds it ("A change refused as it stands is held and reported"). The source declares `mail.message.move` and `mail.message.copy` full and `mail.flags.draft` partial, Graph setting it on what it creates in Drafts and never changing it. It SHALL declare `mail.message.add` by collection, where its rights allow an add: `none` source-wide ("Drafts only"), and `full` on the collection stating the drafts role, so a frontend adding a message elsewhere is refused before it is queued. A check, which knows no store collection, reports the add `partial` ("Drafts only").

An item queued into a folder Graph does not take, and never pushed, stays bound to the source under its provisional handle (pimdir SYNC §2), its refusal standing on that binding: a frontend removes it whatever the source's `item.delete`, the store's gate asking no right of a source binding the item as a pending create alone (pimdir STORAGE §15.6), and the withdrawal takes the refusal with the binding.

#### Scenario: A message bounced between the Inbox and a folder
- **GIVEN** a message in a Graph Inbox
- **WHEN** a move to another folder is staged and the account syncs, back and forth, then a move to Deleted Items
- **THEN** after every run the message is held once on the server and once in the store, under the same `seq`

#### Scenario: A draft staged in Drafts
- **GIVEN** a message added to Drafts with `\Draft` and `\Seen`
- **WHEN** the account syncs
- **THEN** Graph holds it once in Drafts, read, the item bound to its id, and the same add into another folder is refused as it stands, filing nothing, and held

#### Scenario: An add outside Drafts refused at the gate
- **GIVEN** a Graph mail source syncing a folder stating the drafts role and an `Archive` folder
- **WHEN** a frontend queues a message into `Archive`, then into Drafts
- **THEN** the first is refused before it is queued, naming `mail.message.add` and "Drafts only", and the second is queued

### Requirement: A sync records the role each server states
Every one-source sync SHALL record in the store (`set_collection_role`, pimdir STORAGE §14), for each collection it declares, the role the server states for it, and SHALL clear a role the server no longer states: the mail roles `check` lists, from the listing on IMAP and the Gmail API and from the well-known folders on Graph mail; `default` for the calendar or address book `check` marks so. A role the listing does not carry (Graph mail, CalDAV) SHALL be looked up only when the source's set of collection ids differs from the one it was last looked up against, the answer kept in the sidecar (`looked_up_roles`), and a failed lookup SHALL leave the recorded roles as they are. A role SHALL never come from a name.

#### Scenario: A server moves its Sent folder
- **GIVEN** a store where `imap/Sent` holds `sent`
- **WHEN** the server marks `Sent Items` `\Sent` and the next sync runs
- **THEN** `imap/Sent Items` holds `sent` and `imap/Sent` none

### Requirement: A declaration syncs no item
`neverest sync --declare-only` SHALL list every collection each source holds, whatever the collection filter, and record its kind, name and role, perform the queued `collection-create`, `collection-rename` and `collection-delete` intents first, each refused, parked, when the source's rights now forbid it, as the drain it skips would refuse it, and move no item: no queue drain, no send, no other intent, no enumeration and no fetch. It SHALL refuse an account with targets.

#### Scenario: A frontend adding an account
- **GIVEN** an account whose filter keeps `INBOX` alone
- **WHEN** it runs `sync --declare-only`
- **THEN** the store holds every folder with its role and no item, and a later sync keeps syncing `INBOX` alone

### Requirement: A run may perform the queued intents alone
`neverest sync --intents-only` SHALL drain the queue, then perform the queued `collection-create`, `collection-rename`, `collection-delete`, `submit`, `calendar-reply` and `calendar-cancel` intents as a sync does before it lists anything, and stop: no collection listed, no other write pushed, no item fetched, no retention sweep. It SHALL open an endpoint only for an intent that needs its session: one it performs on its server, or a send through its native channel; a send through an `smtp` table SHALL open that channel alone. An endpoint that does not open SHALL be reported unreached and the other endpoints SHALL still perform theirs. It SHALL refuse `--dry-run`, `--reset`, `--declare-only`, `--since` and the collection filters, and SHALL exit as a sync does.

A sent copy neverest files (pimdir STORAGE Annex B.2) is in the store when the run ends and on the server after the next sync.

#### Scenario: A send while the mail server is down
- **GIVEN** an IMAP account with an `smtp` table, whose IMAP server cannot be reached, and a queued `submit`
- **WHEN** it runs `sync --intents-only`
- **THEN** the message leaves through the SMTP channel, the run exits 0, and the report names nothing unreached and no collection

### Requirement: A send that may have been delivered is never sent again
A `submit` that fails after the whole message was handed over and before a verdict came back SHALL be unconfirmed: over SMTP once the data's final `.` was written and flushed and no reply read, the `RSET` following a refused `DATA` command being no data; over Graph `sendMail` once the request was written whole and flushed and no answer read whole (none, one cut short, one that does not parse), or when 500, 502, 503 or 504 answered; over Gmail `messages.send` on any failure with no status but a request it could not build, a 2xx that does not parse included, or when 500, 502, 503 or 504 answered. A `sendMail` or a `messages.send` SHALL never be sent again on a dead connection; only a 429, which Graph and Gmail answer before acting (Gmail's rate-limit 403 waited on as one), SHALL send it again. An unconfirmed `submit` SHALL be parked as an intent whose outcome is unknown (pimdir STORAGE §13, §15.2, io-pimdir `unconfirm_action`), its error beginning with the reserved prefix `unconfirmed: `, which the store's `unconfirm_action` statement alone writes, so its producer reads it unconfirmed rather than refused; it SHALL be reported in `submitted` with `unconfirmed: true` and an `error` beginning `unconfirmed: `, SHALL make the run that parked it exit 3, and SHALL never be sent again by any run.

Before the first byte of a message leaves, the run SHALL count the attempt in the store and SHALL write the intent's id to `submitting` under the store's local/, durably, clearing it once the outcome is recorded; a run finding the mark with that intent still pending SHALL park it as unconfirmed before sending anything, whichever account it belongs to. A store created SHALL start with no mark; a reset of one account SHALL leave it, the mark naming a row of the store.

A run that cannot record an outcome SHALL keep the mark and send nothing more.

#### Scenario: A server hanging up after the data
- **GIVEN** a queued `submit` and an SMTP server that reads the data and closes the connection unanswered
- **WHEN** the account syncs
- **THEN** the run exits 3, the row is parked unconfirmed, its error beginning `unconfirmed: `, and the next run sends nothing

#### Scenario: A run killed while the server holds its verdict
- **GIVEN** a run handing over a queued `submit`
- **WHEN** it is killed before the server answers
- **THEN** the row reads pending with one attempt, and the next run parks it unconfirmed without sending it

### Requirement: Graph mail bodies are fetched in JSON batches
A Graph mail source SHALL fetch the `Full` tier of an id set through JSON batches (`POST /$batch`) of raw MIME gets (`/messages/{id}/$value`), each body answered base64-encoded, matched to its message by request id whatever order Graph answers in, and written to the blob store as soon as it is decoded, before the next is read. A batch SHALL hold at most 20 gets and 4 MiB of the sizes stated for them (the MAPI message size, `$expand=singleValueExtendedProperties($filter=id eq 'Integer 0x0E08')`, which the band listing, the summaries read by id and the whole-folder delta ask for, the delta asking without it once Graph refused it, else the size the store records for the body it owes); a message stated over 1 MiB, or stating no size, SHALL be read alone by a plain get. When more than one message of a fetch states no size to the session nor to the store, their sizes SHALL be read first, 20 a batch, by id alone. A get Graph throttles (429 or a common 5xx), alone or with its whole batch, SHALL be sent again in a later batch once its own `Retry-After` is over (seconds or an HTTP date), else after a back-off doubling from a second, at most four times; one still throttled, or stating a wait over `sync --max-wait` (two minutes by default), SHALL be left unanswered, a throttle (429, 503) giving the source up until its wait is over. A get answered 404, for the size or the body, alone or in a batch, SHALL be answered gone. A get refused otherwise SHALL be left unanswered, so the per-item fallback fetches it and surfaces its error.

#### Scenario: Twenty bodies, one round trip
- **GIVEN** a Graph mail folder holding twenty messages of 50 KiB each whose bodies the store lacks
- **WHEN** a sync hydrates them
- **THEN** one batch request fetches all twenty, stored byte for byte as a direct get would

#### Scenario: Large mail is read alone
- **GIVEN** that folder with three messages of 2 MiB more
- **WHEN** a sync hydrates them
- **THEN** each of the three is read by its own get, and no batch asks for more than 4 MiB

### Requirement: A mail sync lists within a scope on the `Date`
An account MAY bound its mail by `mail.filter.since`, and a run by `sync --since`, which overrides it: a duration back from today (`30d`, `12w`, `6mo`, `1y`), taken to the start of its UTC day so the runs of one day list one scope, a date at midnight UTC, or an RFC 3339 instant. A mail collection SHALL then sync under the scope from that floor (pimdir SYNC §5): a listed message whose `Date` lies below it SHALL be left out of the page, a message with no usable `Date` SHALL be in every scope, and an explicit removal SHALL apply whatever the date. What a scope leaves out SHALL NOT be deleted, neither in the store nor on the server.

A provider's date filter that reads another date than the `Date` header (Gmail's arrival, Graph's `sentDateTime`, which is Exchange's submission) SHALL narrow a round by the scope's floor alone, never by a band's ceiling: a message is dated before it is submitted and received, at times long before (a delayed delivery, a draft sent weeks after it was written, an import), so a ceiling on those dates misses a member its `Date` puts in the band, which the engine then finds absent, dated before the round opened, while the server holds it. Such a listing reaches above the band, and the page keeps what its `Date` puts in the band. IMAP's `SENTSINCE` and `SENTBEFORE` read the `Date` and keep both bounds.

A round whose listing a provider's date filter narrowed (IMAP `SENTSINCE` and `SENTBEFORE`, Gmail's `after:` on the arrival, Graph's `sentDateTime`) SHALL state it on every page (`narrowed`), and its last page SHALL hand every handle the collection holds, read by an enumeration no date filter narrowed (`enumerated`, pimdir SYNC §4, §5): the engine then finds absent only a member in the round's scope the enumeration lacks, so an undated draft received before the window and a message whose `Date` runs ahead of or behind its arrival stay in the store. A round over the band a coverage lacks SHALL state `narrowed` and hand the enumeration on its last page too: a narrowed round handing none finds no member absent (pimdir SYNC §5, vectors 160 and 161), so a member deleted in the band would stay in the store. A round over the whole collection, a delta and a head SHALL state neither. A scope SHALL be refused, by the key or flag it came from, when an endpoint the run syncs holds contacts or calendars.

#### Scenario: An old message received today
- **GIVEN** a mailbox holding, all appended today, a message dated 2020, one dated in the future and one with no `Date`
- **WHEN** it is synced under `mail.filter.since = "30d"`
- **THEN** the store holds the future and the undated messages and not the one dated 2020

#### Scenario: A widening keeps the undated mail
- **GIVEN** a mailbox synced under `--since 2026-09-07`, holding a message with no `Date` and one whose `Date` is garbled
- **WHEN** it is synced under `--since 2026-01-01`, the band listing neither
- **THEN** both stay in the store, beside the band's messages

#### Scenario: A narrower scope deletes nothing
- **GIVEN** that mailbox synced with `--since 2019-01-01`, then under `30d` again
- **WHEN** the run ends
- **THEN** the message dated 2020 is still in the store and on the server, and once deleted on the server the next run removes it from the store

#### Scenario: What an arrival filter misses
- **GIVEN** a Gmail inbox the store binds, holding an undated draft received before the window and a message dated 2030 received before it, which `after:` lists neither of
- **WHEN** a round under `--since` closes, its last page handing the label's ids
- **THEN** both stay in the store

#### Scenario: A band keeps a message dated in it and received after it
- **GIVEN** a Gmail inbox the store binds under a coverage since 2026-10-16, holding a message dated 2026-01-01 and received 2026-11-10 (or a Graph Sent Items message dated 2026-07-15 whose `sentDateTime` is 2026-11-02)
- **WHEN** an unscoped sync widens the coverage, listing the band below 2026-10-16
- **THEN** the message stays in the store, as on the server

### Requirement: A provider narrows a scope with a margin
A connector MAY narrow a scoped listing by its provider's own filter to a superset of the scope: IMAP `SENTSINCE` a day below the floor and `SENTBEFORE` two days above a ceiling, the messages with no `Date` header (`NOT HEADER Date ""`) found by a search of their own, Gmail `after:` two days below the floor, in epoch seconds, and never `before:`. Graph lists by `sentDateTime`, Exchange's submission, from the floor with no margin, below no band's ceiling: a page resumes below the second the page before ended at. The `Date` SHALL decide what is kept. A connector whose checkpoint is bound to no scope (IMAP, Gmail, Graph mail) SHALL widen a scope by listing only the band its coverage lacks.

An IMAP server refusing the search for the messages with no `Date` (iCloud answers `NO [UNAVAILABLE] Unexpected exception`, alone or inside an `OR`) SHALL NOT fail the round: the round lists the messages its date filter keeps, warns once, and the connection asks no more. No absence is concluded wrongly: the engine finds a member absent through the enumeration (`UID SEARCH ALL`) alone. An undated message the store does not hold yet stays unlisted while the scope holds, on such a server.

#### Scenario: A server refusing the undated search
- **GIVEN** an IMAP server answering `NO [UNAVAILABLE]` to `UID SEARCH NOT HEADER Date ""` and `OK` to `SENTSINCE` and `SENTBEFORE`
- **WHEN** a scoped round, then a band, list its mailbox
- **THEN** both list the dated messages and close, and the undated search is sent once on the connection

The enumeration a narrowed round's last page hands SHALL be read after that page's listing, so a member arrived since is in it: IMAP's `UID SEARCH ALL` on the selected mailbox, one command a round; Gmail's ids of the label, listed with no query, 500 ids a request at 5 quota units each, once a round; Graph's ids of the folder's delta pass, the scoped round's last page, which reads them already and costs no request more, and on a band round's last page the ids of a fresh delta over the folder with ids, dates and flags, its link dropped.

### Requirement: A copy is made once
A mail create carrying an origin (pimdir SYNC §4) and a `Message-ID` SHALL be delivered only once its target is known not to hold that message: an IMAP source SHALL search the target by `UID SEARCH HEADER Message-ID "<id>"` before uploading it, and a Graph mail source SHALL list the target folder filtered on `internetMessageId` before `message_copy`. A message found there and bound by no item of this side SHALL be bound to the create, nothing filed; any other find delivers the create as before. A create whose identity was minted for a second copy of one `Message-ID` SHALL NOT search, its twin being the message the search would find. A failed search SHALL reject the create, which stays pending.

A copy whose answer was lost stays staged, and a scope hides the server's copy from every listing (a message dated below the floor, a `Date` the server cannot read), so only a search tells the next run it was made. Outside a scope the listing lands the create by its `Message-ID` first (pimdir SYNC §6), and the search finds nothing more; one search a copy is cheap, copies being rare. A staged move whose answer was lost is made once already on IMAP, its source half moving the member server side; on Graph its target half may deliver it as a copy, which this search covers. A server that matches no `HEADER Message-ID` search files the copy again, as before.

#### Scenario: A copy made before its answer was lost
- **GIVEN** a message dated 2020 bound in `A`, a copy of it staged into `B`, and `B` already holding that message on the server, the answer to the copy having been lost
- **WHEN** the account syncs under `mail.filter.since = "30d"`
- **THEN** `B` holds the message once and the store binds the staged copy to it

#### Scenario: A move made before its answer was lost
- **GIVEN** a message dated 2020 bound in `A`, a move of it staged into `B`, and the server having moved it already
- **WHEN** the account syncs under `mail.filter.since = "30d"`
- **THEN** `B` holds the message once, `A` none, and the store binds it in `B`

### Requirement: A mail round lands page by page, newest first
A mail round SHALL be answered in pages, newest first, each landing in its own write with its resume cursor: IMAP 500 UIDs by UID descending, its cursor `(UIDVALIDITY, lowest UID listed, highest UID found when the round opened)`; Graph 1,000 messages a page, from the message delta with no scope (`Prefer: odata.maxpagesize`, sent with every link) or from `/messages` by `sentDateTime` under one (`$top`), its cursor the second the page ended at, the next page listing below the second after it; Gmail 100 ids, its cursor the page token. The checkpoint SHALL be the one taken when the round began where the provider gives one (IMAP `HIGHESTMODSEQ` at the select, Gmail's profile `historyId` read before the listing), Graph's delta link on its last page. An interrupted round SHALL resume from its cursor, and a cursor the provider refuses (an expired Graph link, a Gmail page token) SHALL restart it, a moved `UIDVALIDITY` being a rekey (see "An IMAP handle-space change rebuilds the collection and bumps its generation").

#### Scenario: A round killed after its first page
- **GIVEN** a mailbox of 1,100 messages synced for the first time
- **WHEN** the run is killed once a page landed, and the account is synced again
- **THEN** the second run closes the round and the store holds every message once

### Requirement: An open IMAP round lists its head
An IMAP source SHALL state delta heads and band heads where its server enabled QRESYNC (its `ENABLED` naming it), band heads alone elsewhere, CONDSTORE without QRESYNC stating no removal, read from the capabilities its connection read when it opened (pimdir SYNC §4); the sync SHALL tell the engine the heads its source states. A delta head SHALL be the QRESYNC `SELECT` from the checkpoint asked, answered as a delta and never as a round page: the messages changed since, their meta read where the store does not bind them under that `UIDVALIDITY`, the vanished UIDs, and the checkpoint the server holds now. A server without QRESYNC, or a checkpoint with no `UIDVALIDITY` or no modseq, SHALL list none; a checkpoint whose `UIDVALIDITY` moved SHALL be refused, the round restarting and its close rebuilding the collection. A band head SHALL be `UID SEARCH SINCE` the day the round opened less two days, by the `INTERNALDATE`, each message's flags or meta read as a round page reads them, with no removal and no checkpoint; an instant it cannot read lists none. A page SHALL read its members by `UID FETCH` when it lists, leaving out a UID the mailbox no longer holds. The last page of a round resumed from a cursor SHALL list again, read after its `SELECT`, every message of the scope above the highest UID the round found when it opened, so what a band head named, stamping nothing, is not found absent when the round closes; a cursor written before it carried that UID SHALL list the whole scope again.

#### Scenario: An arrival and an expunge while a round is open
- **GIVEN** a QRESYNC mailbox of 1,100 messages whose first sync was killed after its first page, then a message appended and one the first page listed expunged
- **WHEN** the account is synced again
- **THEN** the arrival lands and the expunged message goes before any further page, the round still open; the round then closes keeping the arrival, and the next delta lists nothing new

#### Scenario: A band head's arrival survives the close
- **GIVEN** the same mailbox read as a server without QRESYNC
- **WHEN** the account is synced again
- **THEN** a band head lands the arrival while the round is open, the round's close finds nothing absent, and the next round drops the expunged message

### Requirement: A Graph mail folder keeps one delta link over the whole folder
A Graph mail source's checkpoint SHALL be a message delta link made over the whole folder, with no date filter, whatever the scope, and the source SHALL be bound to no scope (pimdir SYNC §4). With no scope, a round SHALL be that delta, with the summary `$select`, its last page carrying the link. Under a scope, a round SHALL list the scope's mail by its `sentDateTime` through `/messages` (`$filter=sentDateTime ge A and sentDateTime lt B`, `B` left open for the newest band, `$orderby=sentDateTime desc`, the summary `$select`, `$top` 1,000), then pass once over the folder's delta with `$select=id,sentDateTime,isRead,isDraft,flag`, `Prefer: odata.maxpagesize=1000` on every request, as its last page: the pass SHALL list every member in scope the store binds by its flags, SHALL list again a member the band listed in the same run only when its flags moved since, by one read, and every other member in scope by one read in JSON batches, SHALL state gone only a member one of its rows marks removed (`@removed`), and SHALL carry the link. A pass interrupted SHALL start again. A round over the band a coverage lacks SHALL list that band alone and keep the link, its last page handing the folder's ids read by a fresh delta with the pass's `$select`, whose link it drops; the messages with no date, which a `sentDateTime` filter never lists, SHALL stay bound, the enumeration holding them.

A Graph mail source SHALL state as vanished only what Graph states removed (pimdir SYNC §4): a row a delta marks `@removed`, in a followed link, a whole-folder round or the pass. It SHALL NOT state gone a member a listing, a fresh delta or the pass did not list. Such a member is the engine's to find absent, from the ids the pass hands as its unnarrowed enumeration, in the round's scope and under the guards that park a mass absence and an empty listing (pimdir SYNC §5).

One removal is so left unseen. A message a band page of a round listed, whose binding that page stamped, and that Graph dropped before the round's pass is never found absent, the engine finding absent only a binding the round did not stamp, and no row states it removed, a fresh delta reporting no removal older than itself. Its row SHALL stay, bound and readable, until a later round over a scope holding its date closes without listing it: one an expired link (410) opens, or a full sync. Neither a rerun, nor a delta, nor a wider scope, whose round lists the band it lacks alone, drops it. Taking the delta link before the band pages and following it at the round's end would have Graph state such a removal; Microsoft documents `$deltatoken=latest`, the way to a link with no enumeration, for directory resources alone (Microsoft Graph, "Use delta query to track changes", read 2026-11-13), not for the messages of a mail folder, so neverest does not rely on it.

A delta SHALL follow the link: a change whose `sentDateTime` lies outside the scope SHALL be dropped, a removal SHALL apply whatever the date, a member the store binds SHALL be listed by its flags, and a member in scope it does not bind SHALL be named by its summary, the row's own on a link made with summaries, else read in JSON batches of 20, a message gone since being left out and any other summary Graph does not answer failing the listing. The link SHALL serve every later scope: a narrower one follows it, a wider one lists its band; only an expired link (410) or a full sync SHALL open a round. The checkpoint SHALL name what the link's rows carry; a link stored before it did SHALL be refused when the coverage is bounded, a `$filter` having made it.

While a folder's round is open, a run resuming it SHALL list the round's band head before its next page (pimdir SYNC §4, §5): the folder's messages Graph received from two days before the instant the round opened, through `/messages` (`$filter=receivedDateTime ge T`, `$orderby=receivedDateTime desc`, the summary `$select`, the size `$expand` until Graph refuses it with a 400, `$top` 512, every next link followed), checked against the run's scope by `sentDateTime`, a member the store binds listed by its flags and any other by its summary, with no checkpoint and no removal. A delta head SHALL be answered none, the link arriving with a round's last page only, as SHALL a band head whose instant reads as none. A run that listed a band head SHALL list it again in the round's last page, read after that page's own rows: a whole-folder round's last delta page and a band round's last band page SHALL add every member in the round's scope that listing names and the page neither lists nor states removed; under a scope, the pass SHALL read the summary of a member the head named this run as of any member the band did not list, listing every member in scope still in the folder.

#### Scenario: A pass that fails to list a member
- **GIVEN** a Graph folder synced under `--since 2026-06-01`, its link expired, and a fresh delta that lists neither a June message nor a July one the folder still holds
- **WHEN** it is synced under `--since 2026-07-01`
- **THEN** the round closes and both messages stay bound: the pass states no removal, the June one lies out of the round's scope and the July one was listed by a band page

#### Scenario: A message dropped between its band page and the pass
- **GIVEN** a scoped round interrupted after a band page listed a message, which Graph then drops
- **WHEN** a later run resumes the round and its pass closes it
- **THEN** the message stays bound, and a later round over its scope drops it

#### Scenario: Three widenings
- **GIVEN** a Graph folder synced under `--since 2026-09-01`
- **WHEN** it is synced under `--since 2026-07-01`, then `2026-06-01`, then `2026-03-01`
- **THEN** each run lists only the messages of the band it adds, the folder is passed over once for the link and once a band for its ids, and the checkpoint is the link the first round made

#### Scenario: A change out of scope
- **GIVEN** that folder synced under `--since 2026-07-01`
- **WHEN** a message of March is flagged on the server, one of September is flagged, and two messages arrive, one dated today and one dated 2020, and the folder is synced again
- **THEN** the September flag reaches the store, the March one does not, and only the message dated today is read and stored

#### Scenario: A message arriving while the folder's first round is open
- **GIVEN** a Graph Inbox synced with no scope whose round's first page landed, the round left open
- **WHEN** two messages arrive, the account is synced and stopped after the head, one of them is deleted, and the account is synced again
- **THEN** both land before the round's next page, the round's last page lists the one still there again after the delta link, and once the round closes the store holds it and not the other

### Requirement: Every listed member arrives named
Every member a listing hands the engine SHALL carry its meta (pimdir SYNC §4): IMAP reads `UID FLAGS RFC822.SIZE` and `BODY.PEEK[HEADER.FIELDS]` (`Date`, `From`, `To`, `Cc`, `Bcc`, `Subject`, `Message-ID`, `In-Reply-To`, `Content-Type`) in the listing's own `UID FETCH`, never `BODYSTRUCTURE`, deriving the meta from the header block as the body would derive it; Graph reads its summary `$select` and the MAPI size `$expand`; Gmail its metadata, `Content-Type` among the headers. A member the store already binds, a message being immutable, SHALL be named by its binding rather than read again. A kind whose meta is its body (CardDAV, CalDAV, Google Calendar and People, Graph contacts and events) SHALL have the bodies of a page fetched before it lands, 64 a request, each member carrying its body, unless the store holds it at the listed revision. The attachment mark SHALL be Graph's `hasAttachments`, else a top-level `multipart/mixed`, until the body is read.

### Requirement: The report states each collection's coverage
The report SHALL list under `coverage`, for every collection a run synced on an endpoint, the scope of that endpoint's last closed round (`since`, `until`) and when it closed (`closedAt`), absent while none ever closed, and the round still open (`round`: `since`, `until`, `startedAt`). A round left open SHALL make the run incomplete.

### Requirement: The report counts what each source downloaded
The report SHALL list under `downloaded` the octets of bodies and listed meta each source received during the run.

### Requirement: A request that creates is sent again only when unprocessed
A request creating something (an uploaded or copied message, an event, a contact, a label, an imported message, a send, an invitation reply or cancel, a DAV create) SHALL be sent again on a 429 or a Google rate limit only, which a provider answers before acting, never on a 503 or another 5xx, after which it may have landed. A Graph body batch answered 500, 502 or 504 SHALL be sent again without making the source give up as throttled.

### Requirement: A Google stream that failed is reopened
A Gmail, Google Calendar or Google People request whose stream failed mid-exchange (an I/O error, a reset, an early EOF) SHALL mark the stream dead, so the next request opens a new one instead of writing to the broken one. The three share one request loop (src/throttle.rs `google_send`) carrying this rule with the throttling ones.

#### Scenario: A reset between two requests
- GIVEN a Google Calendar or Google People session whose connection is reset during a request
- WHEN the next request is sent
- THEN it goes out on a new connection and succeeds

### Requirement: Google People bodies are read in batches
A `gpeople` source SHALL read the persons a page or a hydration asks for through `people:batchGet`, up to 200 a request, never one `people.get` a person. A person the batch answers not found SHALL be gone, never an error, and each person read SHALL be stored with its etag as its batch lands. A person the batch refuses otherwise SHALL be left unanswered, so the per-item read fetches it and surfaces its error.

#### Scenario: An address book of 450 contacts
- **GIVEN** a `gpeople` source whose 450 contacts the store does not hold
- **WHEN** it is synced
- **THEN** their bodies are read in 3 requests

### Requirement: A Graph connection is reused only after a whole answer
A Graph session SHALL send its next request on the same connection only after an exchange that read a whole answer on a connection the server neither ended nor asked to close, and SHALL reopen a connection idle for more than 60 seconds, below the two minutes after which Graph closes one. Any other failure (a read that timed out, a reset, an answer cut short) SHALL drop the connection, so a late answer is never read as the next request's. A request that only reads (a `GET`, a `$batch` of gets) that met a reused connection already dead before any octet of its answer (reset, aborted, broken pipe, end of stream) SHALL be sent again once, on a new connection (RFC 9112 §9.3.1); a read that timed out SHALL NOT be sent again, nor SHALL a request that creates, sends or moves.

#### Scenario: A connection closed while it idled
- **GIVEN** a Graph session whose connection the server closed after its last answer
- **WHEN** the session asks for the next page of a listing
- **THEN** the page is asked again on a new connection and the listing goes on

#### Scenario: A move on a dead connection
- **GIVEN** that session
- **WHEN** it pushes a move
- **THEN** the move fails without being sent again, and the next request opens a new connection

### Requirement: A Graph write on an item gone counts as done
A flag set or a delete on a Graph message answered 404 SHALL count as done, as SHALL the delete of a Graph contact or event answered 404 on the delete or on the read that checks its revision. A move answered 404 SHALL count as done only when the message reads as gone by its id; still there, the 404 names the destination, and the move SHALL fail. A copy of a message gone SHALL fail.

#### Scenario: A message deleted on the server meanwhile
- **GIVEN** a message whose `\Seen` is staged in the store and that was deleted on Microsoft 365 since the last sync
- **WHEN** the account syncs
- **THEN** the flag push counts as done, the run is complete, and the message is gone from the store

### Requirement: A Graph listing keeps a few octets a message
A Graph mail delta and the pass SHALL be read page by page, each page dropped once read, keeping for each message its id and what its last row says: removed, out of scope, its flags, or, on a link made with summaries, the summary of a member in scope the store does not bind. A session SHALL keep no summary from one listing to the next: a band round followed by a pass SHALL keep the flags of what it listed, three bits a message, dropped by the pass, and the `Meta` tier SHALL read each summary by its id.

### Requirement: Collections are taken by role
The one-source sync SHALL take its collections in the order of the roles the run recorded for them (or, a role lookup failing, of those its listing states): the inbox or a default calendar or address book first, then sent, drafts, every collection without one of these roles by name (ASCII case aside, then by id), the all-mail archive, junk, and the trash last. Phase 1 SHALL hand its collections to the connections in that order, and a batch of bodies tied with another in the download order SHALL go first when its collection comes first.

#### Scenario: The inbox before the archive
- **GIVEN** an IMAP account holding `Archive`, `INBOX`, `Sent` and `Trash`
- **WHEN** it is synced over one connection
- **THEN** it lists `INBOX`, `Sent`, `Archive` and `Trash`, in that order

### Requirement: Every collection's newest mail comes first on request
`sync --prime <N>` SHALL, under a positive N, run phase 1 in two passes. The first SHALL take every collection in the run's order: a round SHALL list its first page and stop at rest before asking its second, the store left as a run killed there leaves it (the round open at its cursor); a round an earlier run left open SHALL list its head where its source lists one, and stop before asking its next page; a collection whose rounds are closed SHALL run its delta to the end. Right after, the collection's N newest bodies still owed (by the mail summary's date) SHALL download ahead of any other work, on whichever connection is free, the others it owes joining the feed of the idle connections. The second pass SHALL start once every first-pass spine and every primed body has ended, and SHALL list each stopped collection to its end, in the run's order, from its cursor; its pushes, its staged work and its coverage are done then, as for a collection listed whole. Phase 2 SHALL download each collection's N newest owed bodies ahead of every other, then the rest in the download order. The report SHALL name each fetch once, a stopped spine handing the next one what it owed. The default SHALL be 50 under `--download-order newest` and 0 under `largest`; `--prime 0` lists each collection whole in turn; a dry run never primes. Priming orders a run and SHALL NOT shorten it: a run that ends has listed every page and downloaded every owed body.

#### Scenario: One connection, three large folders
- **GIVEN** three folders of 1,100 mails each, synced for the first time over one connection under `--download-order newest`
- **WHEN** the third folder's first page lands
- **THEN** the first two hold their first page alone, and each folder's 50 newest mails are readable before any second page lists

#### Scenario: Killed once primed
- **GIVEN** that run killed once each folder's 50 newest mails are readable
- **WHEN** the next run syncs the account
- **THEN** every round closes and each folder holds each of its mails once, with its body

### Requirement: A run may download only what it listed
`sync --bodies <all|listed>` SHALL choose which owed bodies a one-source run downloads; `all` is the default. Under `listed`, the run SHALL download, in both phases, only the bodies of members its own pages landed, every other owed body staying owed for a later run. `--prime`, `--bodies` and `--download-order` SHALL be refused beside `--intents-only` and `--declare-only`, which list nothing.

#### Scenario: An arrival behind a backlog
- **GIVEN** an Inbox an interrupted first sync left owing thousands of bodies, and a new mail
- **WHEN** `sync --include-collection INBOX --bodies listed` runs
- **THEN** the new mail's body is downloaded and the backlog is not

### Requirement: A body gone from the server is owed nothing more
A body read the server answers gone SHALL NOT fail the run: a Gmail or Graph read answered 404 (in a batch, a size read, or alone), a CardDAV, CalDAV, Google Calendar or Google People read answered 404 or 410, and an IMAP UID a completed `UID FETCH` answers nothing for (RFC 9051 §6.4.9), or a lone fetch of it that streams nothing. The run SHALL record the body gone for each collection owing it and SHALL NOT ask for it again: no later batch of phase 1 and none of phase 2. The store SHALL keep the member as it was listed, a body read being no listing (pimdir SYNC §5), so the next run's listing takes it away before that run plans its body; a member the listing names and every read finds missing is thus asked for once a run, never failing its collection.

#### Scenario: Expunged while the run downloads
- **GIVEN** an IMAP folder whose three smallest messages are expunged after a `-j 1` run listed them, while it downloads the largest bodies
- **WHEN** the run reaches their batch
- **THEN** the run completes, the three members stay without a body, and the next run's listing removes them, fetching nothing

#### Scenario: A Gmail body gone
- **GIVEN** a Gmail message deleted after it was listed
- **WHEN** its raw read answers 404
- **THEN** the run completes and no later batch of the run asks for it

### Requirement: A message under several labels is downloaded once
On a source whose handle names one item across its collections (a Gmail message id, the same under each label), a run SHALL download each body once: phase 2 SHALL plan a handle once, under the first collection in the run's order listing it, among the primed bodies when any collection primes it; a batch SHALL read where its handles are owed across the source's collections, serve a handle some collection already holds a body for from that body with no download, download the rest once, and raise each body to `Full` in every collection holding its handle without one, one collection at a time and only where its sync is at rest. The store holds one object per hash, which every placement references.

#### Scenario: `INBOX` and a user label
- **GIVEN** a Gmail message under `INBOX` and `Work`, its body owed in both
- **WHEN** the run downloads bodies
- **THEN** one raw read fetches it, and both placements hold the same body

#### Scenario: A label added later
- **GIVEN** a message whose body the store holds under `INBOX`, listed under `Work` by a later run
- **WHEN** that run downloads bodies
- **THEN** `Work` holds the same body and nothing is downloaded

### Requirement: A landed body stands for its attachments
Every body a run lands SHALL reach the store through io-pimdir's write (`PimdirSourceStore::write`, the `WantsWrite` of a sync, an upgrade or a mutation), which records in the same transaction a file holding no body per part of a message disposed as an attachment, in the account's attachments collection, with its name, media type, decoded size and section, and an `attachment` reference from the message (pimdir STORAGE §14.3, io-pimdir `14a8fca`). neverest SHALL write the store by no other way. A body the account already holds (a copy, a move, a second Gmail label) records none, and a body held before is not walked again. A body entering under a `Message-ID` another row of the account holds another body under, a message it does not agree with on subject, date and sender, records none either: the keys stay the first message's (pimdir STORAGE §11.1, §14.3).

#### Scenario: Primed, paged or after the listing
- **GIVEN** an IMAP folder holding a message with two attachments, one with an inline image only, one with an attachment two levels down a `multipart/signed`, and a plain one
- **WHEN** a sync downloads their bodies, primed, by page over four connections, or after the listing over one
- **THEN** the account holds three stand-ins, each named, typed and sized, referenced by its message; a second sync fetches and records nothing, and a copy of the messages in another folder adds no stand-in (tests/attachments.rs `a_synced_body_stands_for_its_attachments`)

#### Scenario: A body landing later
- **GIVEN** a run stopped once its listing landed, every body owed
- **WHEN** a message with an attachment arrives and a `--bodies listed` run follows, then a full run
- **THEN** the arrival's stand-in is recorded by the first, the backlog's by the second (tests/attachments.rs `a_body_landing_later_stands_for_its_attachments_then`)

### Requirement: A calendar its server keeps from tasks is declared so
A source SHALL declare `calendar.component.vtodo` `none` (pimdir STORAGE §15.6), the detail naming why, wherever a calendar it syncs takes no VTODO resource, so the store's gate refuses a task added or moved there before it is queued and a client reading the store does not offer the calendar as a task list:

- source-wide, from the configuration alone, for a Google Calendar or a Graph calendar source, whose provider keeps tasks apart (Google Tasks, Microsoft To Do);
- on each CalDAV calendar whose `supported-calendar-component-set` (RFC 4791 §5.2.3) its listing reads without `VTODO`.

A CalDAV calendar whose listing states no component set, which takes any component, or one naming `VTODO`, SHALL get no `calendar.component.vtodo` row, nor SHALL a source of another kind.

In an account with targets, a hub calendar takes a task only where both endpoints of its pair take one, the task crossing to the target. The source SHALL therefore declare `none` on each calendar of its namespace a target's listing reads without `VTODO`, as on those its own listing does, and source-wide when one of its targets is a Google Calendar or a Graph calendar endpoint, the detail naming that target. A target SHALL stay undeclared: a row would make it a declared source, refusing every capability it has no row for.

What a listing stated SHALL be kept in the sidecar (neverest.json, `taskless`) by source, as hub ids, replaced at the source's next listing, so the declaration before the drain, which opens no endpoint, carries it from the run that listed it. With targets, the record SHALL be replaced once every target of the source has listed; a run stopping earlier SHALL add what it listed to the record rather than drop what a target it did not reach stated. A source never listed declares no per-calendar row.

A run SHALL declare again as soon as a listing is recorded and its collections are in the store, before any item of theirs moves, so a calendar met for the first time carries its `calendar.component.vtodo` row from the listing on rather than from the end of the run. A created calendar is met by the listing that follows its creation, the same run's.

The store reads a source as syncing a collection from its `sources` row or a binding there (pimdir STORAGE §15.6, `load_capabilities`), and a declaration holds only where its source syncs. Before that declaration, a run SHALL therefore record the source as syncing each collection it is about to sync and holds no state in yet (local: every collection kept, all of them under `sync --declare-only`; paired: every collection common to the pair, the target left unrecorded as it stays undeclared): a `sources` row with no checkpoint, which reads as no sync state, written through the canonical `upsert_checkpoint` only after the canonical `load_checkpoint` found no row, in one transaction, so a checkpoint already there SHALL never be overwritten. A dry run records nothing. A failure SHALL be logged, the first item sync recording the source anyway.

#### Scenario: An events-only calendar refuses a task before the push
- **GIVEN** a CalDAV source whose last listing read `events` with the component set `VEVENT` alone and `work` with `VEVENT` and `VTODO`
- **WHEN** the account's sources are declared and a frontend then queues a VTODO add into each
- **THEN** the add into `events` is refused at the producer's gate as `calendar.component.vtodo` unsupported, the store reads `events` as taking no tasks, and the add into `work` is queued

#### Scenario: A Google or Graph calendar refuses a task
- **GIVEN** a Google Calendar source syncing `gcal/primary` and a Graph calendar source syncing `msgraph-calendar/AAMk`
- **WHEN** the account's sources are declared from the configuration alone and a frontend queues a VTODO add into each
- **THEN** both adds are refused at the gate as `calendar.component.vtodo` unsupported, the detail naming where the provider keeps tasks

#### Scenario: A target's events-only calendar refuses a task
- **GIVEN** a CalDAV source paired with a CalDAV target whose listing reads `events` with `VEVENT` alone, the source's listing stating no component set
- **WHEN** the pair lists its collections and a frontend then queues a VTODO add into `caldav/events`
- **THEN** the add is refused at the gate, under the source's declaration, and the target stays undeclared

#### Scenario: A calendar met mid-run is declared before the run ends
- **GIVEN** a CalDAV source whose listing meets `events`, `VEVENT` alone, for the first time
- **WHEN** the run records the listing and the first item sync of `events` lands, before the run's last declaration
- **THEN** the store already reads `events` as taking no tasks

#### Scenario: A calendar listed and not synced yet refuses a task
- **GIVEN** a CalDAV source whose listing meets `/caldav/v2/x%40g/events`, `VEVENT` alone, for the first time, and `work`, holding a checkpoint there
- **WHEN** the run records the listing, before any item sync
- **THEN** a VTODO add into `caldav//caldav/v2/x%40g/events` is refused at the gate, the source syncs that collection with no checkpoint, and `work` keeps its checkpoint

### Requirement: A create superseding a member is delivered by moving it
A CalDAV source SHALL deliver an `Add` naming the member it supersedes (pimdir SYNC §3 "Superseded", §4) by moving that member into the target calendar with WebDAV `MOVE` (RFC 4918 §9.9, `Overwrite: F`, an absolute `Destination`), gated on the member's last-synced ETag, then writing the stored body over the moved member, gated on the ETag the move answered and unconditional when it answered none. The add SHALL be accepted under the moved member's handle and the revision the write left, so the server never holds the `UID` twice: a server keeping one `UID` per account (Fastmail) refuses an upload while the source holds it.

A `MOVE` answered as one the server cannot do (405, 501, or 404 for the member, Google's answer) SHALL have the add delivered by the plain upload, as without a superseded member. A `MOVE` refused for the identity SHALL reject the add as a refused upload does: a duplicate `UID` named as one (`A refused duplicate names itself`), a moved member (412) as a rejected `move`. A body missing from the store SHALL reject the add before anything moves. A write failing after the move SHALL reject the add, the target's next listing naming the moved member. A CardDAV source SHALL upload, io-webdav moving no card, and every other backend SHALL ignore the superseded member.

#### Scenario: A task filed into another calendar moves
- **GIVEN** a task of `caldav/tasks` filed into another calendar of the same CalDAV server, keeping its `UID`
- **WHEN** the target's create is pushed
- **THEN** the member is moved with `MOVE` gated on its ETag, its body is written over it, and the create is accepted under the moved handle with the write's ETag

#### Scenario: A server without MOVE takes an upload
- **GIVEN** the same filing on a server answering `MOVE` with 404
- **WHEN** the create is pushed
- **THEN** the body is uploaded with `If-None-Match: *` and the create accepted

### Requirement: A delete of a member already gone is accepted
A DAV delete answered 404 or 410 SHALL be accepted (pimdir SYNC §4): pushes are at least once, and a create delivered by a move took the member it superseded along before that member's delete is pushed. So SHALL a Google Calendar or People delete whose read or delete is answered 404 or 410, and a Google Calendar delete of an event Google keeps as a cancelled tombstone, which sends no request: answered refused, the delete would be held for good.

### Requirement: A DAV relocation moves the member
A `Remove` relocating a CalDAV member into another calendar SHALL move it with WebDAV `MOVE`, keeping its `UID` in one place. Where the server cannot move it (405, 501, 404), and on CardDAV, the member SHALL be re-created at the target and deleted from the source on its ETag, the delete running only once the create was accepted, so a failure leaves the member where it was.

### Requirement: A truncated `sync-collection` listing is incomplete
A `sync-collection` answer the server truncates (RFC 6578 §3.6, a `507` row) SHALL be followed from the sync token it carries until an answer is not truncated, a bounded number of times. A listing whose last answer was still truncated, the bound reached or the answer carrying no token, SHALL be returned incomplete, read as a delta: what it lists lands and no member is inferred removed from its absence. A truncated answer without a token SHALL end the listing, never restart it from its start, and the checkpoint SHALL stay the one the listing started from.

A listing returned incomplete for a truncation, a `sync-collection` or a `PROPFIND` one, SHALL be reported as a `failures` error of the `listing` step naming the collection, and the run SHALL exit 4, every run the server cuts it again: the members past the cut never reached the store, and a run saying it is in sync hides them.

#### Scenario: A first listing the server keeps truncating
- **GIVEN** a CardDAV server truncating every `sync-collection` answer of a new address book
- **WHEN** the account is synced
- **THEN** the cards it listed reach the store, and no card is inferred deleted on either endpoint

### Requirement: A DAV answer that is no whole multistatus fails
A `207` answer whose body is not one whole `multistatus` document (a gateway's page, a document cut short or malformed, an empty body) SHALL fail the request reading it, as io-webdav refuses it: a `sync-collection` report, the `PROPFIND` listing standing in for it, a collection listing, a multiget. It SHALL NOT be read as a listing of what parsed, which a complete listing turns into the removal of every member left out. A failed listing is reported as "A collection that failed to scan is reported, never only logged" says, and a failed multiget falls back as "A body fetch answers for every handle it was asked about" says.

A `DELETE` or a `MOVE` answered `207` naming a member with a status other than 2xx (RFC 4918 §9.6.1) SHALL fail, the member being still there.

#### Scenario: A gateway answers a listing with its own page
- GIVEN a CardDAV server whose `sync-collection` report is answered `207` with an HTML page
- WHEN the account is synced
- THEN the address book fails to list, and no card is stated removed

### Requirement: A collection listing names every collection the source holds
A source's collection listing SHALL name every collection it holds, whatever the provider leaves out by default: hidden Google calendars, every page and every depth of Graph contact folders. A collection left out of a listing reads as deleted where the store knows both endpoints held it.

### Requirement: A reset never loses work no server holds
`sync --reset` SHALL act on exactly one account and SHALL start it over inside the store it shares: it SHALL drop the account's collections (pimdir GUIDE "Removal": `delete_collection`, the refcounts recomputed in its transaction, then the collector), with what the store records under them (items, bindings, checkpoints, coverage, capability rows), the account's source-wide capability rows and its sidecar entry, stamp the entry again and sync the account anew. Every other account SHALL be left as it was.

It SHALL read the account's collections before dropping any, and SHALL refuse while one holds a queued or parked action, which io-pimdir refuses to drop with its collection: the refusal SHALL name each row and the way out, the queued ones applied by `neverest sync --queue-only` or a sync, the parked ones cancelled from the frontend that queued them. Without `--force` it SHALL also refuse while they hold other work no server has: a conflict, an item whose flags or body no binding's base holds (a staged edit or create), a staged delete, or a retained item, bound to no server, listing that work per collection and naming `--force`, which drops that work with the collections.

`--reset` SHALL be refused beside `--dry-run`, which would drop the real collections before planning, and beside a collection or source filter, the reset being the whole account's.

#### Scenario: A queued send survives a reset
- **GIVEN** an account whose collection holds a queued action
- **WHEN** it is synced with `--reset`, forced or not
- **THEN** the run refuses, naming the row and `neverest sync --queue-only`, and the store is untouched

#### Scenario: A reset leaves the other accounts alone
- **GIVEN** two accounts sharing a store, each holding a collection
- **WHEN** one is reset
- **THEN** its collections and their checkpoints are gone and its sidecar entry starts over, and the other's collections, items and entry are as they were

#### Scenario: A forced reset drops staged work
- **GIVEN** an account holding a retained item
- **WHEN** it is synced with `--reset`
- **THEN** the run refuses naming `--force`, and with `--reset --force` the account's collections are dropped

### Requirement: A store a newer neverest wrote asks for an upgrade
Opening a store refused as `store-version` whose `store_meta.format_version` names a later draft than this neverest's, or any version not of the `storage-draft-NN` form (a release), SHALL fail saying a newer neverest wrote it and naming the upgrade, never a reset: the store is good, and a reset would drop what it holds that no server has.

#### Scenario: A downgrade
- **GIVEN** a store written by a newer neverest
- **WHEN** an older neverest syncs the account
- **THEN** the run fails asking for a newer neverest, and the store is untouched

### Requirement: The sidecar is replaced whole
neverest.json SHALL be written to a temporary file beside it, under the store's local/, synced, and renamed over the previous one, so a run killed while saving it leaves the previous sidecar or the new one, never a torn file the next run refuses.

### Requirement: A relay appends only where the target's listing is closed
A message SHALL be relayed to a target only when the target has closed a round over the collection, has none open, and that round's scope holds the message's date (an undated message being in every scope), as pimdir SYNC §5 guards an add: the store then knows what the target holds there, so a message it already holds is bound to it rather than appended twice. A message held back waits for a later pass.

A relayed body SHALL be exactly the size its listing stated: the append is length-prefixed with it, so the last octet SHALL be held until the source is known to end there, and a body of another length SHALL NOT complete an append. A message whose relay fails (a refusal, a size mismatch, a copy the server does not name) SHALL be reported under `rejected`, action `append`, its id the link, and the relay SHALL go on with the next message; each side that failed SHALL be recovered first, as after a failed body fetch, and only a side that cannot be SHALL fail the collection.

#### Scenario: A target refusing one message
- **GIVEN** a relay whose target refuses the append of the first of two messages
- **WHEN** the collection is relayed
- **THEN** the first is reported rejected, the second is relayed, and the run exits 3

#### Scenario: A target still listing its mailbox
- **GIVEN** a relay account whose target's first round over the inbox is open
- **WHEN** the account is synced
- **THEN** nothing is relayed into that inbox until the round closes

### Requirement: A store error carries its code
A store failure that ends a command SHALL carry, in its JSON failure object, the code io-pimdir gives it (`PimdirError::code`), spelled the same: `store-version`, `store-uncreated`, `store-foreign`, `store-corrupt`, `store-not-current`, `store-hash-algo`, `sync-version`, `store-busy`, `store-owned`, `store-staging`, `store-in-flight`, `store-exposed`, `lock-replaced`, `action-malformed`, `flag-invalid`, `unsupported`, `index-fts5-missing`, `index-version`. A store neverest's own layout refuses SHALL carry `store-layout`. A failure a caller cannot act on (an SQL or I/O error) SHALL carry none, nor SHALL a code neverest cannot meet (`index-busy`, which `index` answers as `busy`; `index-missing`, `index-outdated` and `index-query-only`, a query client's; `index-zone-unknown`, neverest stating only a zone its time zone database named; `cutoff-invalid`, neverest writing its cutoff as the store stamps; `scope-invalid`, neverest writing a scope bound in the one shape the store holds; `store-reset`, neverest naming a queue row by its bare id; `action-not-intent`, neverest parking unconfirmed a `submit` alone; `backup-occupied`, neverest taking no backup; `conflict-unread`, which the one-way overwrite answers by fetching the unread body). A failure folded into one account's outcome SHALL keep its code. No message SHALL advise deleting the store: the remedies are an upgrade, the reinstall of "A store the format outgrew is refused with its remedy", which moves the store directory away, and `sync --reset`, which drops one account's collections.

#### Scenario: A downgrade under `--json`
- **GIVEN** a store whose `store_meta.format_version` names a later draft
- **WHEN** neverest runs `--json sync --queue-only` on the account
- **THEN** it exits 1 printing a failure object whose `code` is `store-version`, the message asking for an upgrade and naming no reset

#### Scenario: A store root naming another program's database
- **GIVEN** a pimdir.db holding another program's tables
- **WHEN** neverest runs `--json sync --queue-only` on the account
- **THEN** it exits 1 printing a failure object whose `code` is `store-foreign`, the database untouched

### Requirement: A body reaches the blob tree under a lock
A body neverest writes SHALL go through the write side io-pimdir hands a lock holder: a fetched body streams into the owner's stage (`PimdirStore::blob_stage`), which hashes it as it passes, and a settled or merged conflict body is staged by the producer that enqueues it, in one step with its action (`PimdirProducer::enqueue_with_body`). neverest SHALL NOT hash or write a body itself.

### Requirement: An IMAP mailbox is never read as smaller than it stated
A mailbox whose `SELECT` answer states no `EXISTS` SHALL be searched as any other, never read as empty: a listing read short is a delete of everything left out.

### Requirement: An IMAP listing is read whole or fails
An answer a listing is read from (a `UID SEARCH`, and the `UID FETCH` of a page's flags or header fields) holding a line io-imap could not decode SHALL fail the listing, naming the command and the count of such lines. The line may be a row, and a UID a page leaves out is a message the mailbox no longer holds: read without it, the page states the message removed. The search looking for a message by its `Message-ID` before a copy or a removal SHALL fail the same way.

The search naming the message an `APPEND` just delivered is no listing: the append is accepted whatever it answers. It, and a `SELECT`, `LIST` or `STATUS` answer holding such a line, SHALL be read with a warning naming the count. The text of an `ALERT` response code SHALL be shown as a warning.

#### Scenario: A row that does not decode
- GIVEN a mailbox one of whose messages carries a flag that is no IMAP atom
- WHEN its page is listed
- THEN the listing fails, naming the `FETCH` answer, and no message is stated removed

### Requirement: A Graph band page resumes below a date
A Graph band page SHALL resume below the second the page before it ended at, that second included, never at the offset Graph's next link carries: a member deleted between two pages, in one run or across two, would shift a later one out of the round, and a band round has no pass to list it again. A page whose messages were all sent within one second SHALL follow Graph's next link within the same request until a page leaves that second. A band next link a round stored before SHALL be refused, which opens the round again.

#### Scenario: A message deleted between two runs of a band round
- **GIVEN** a band round whose first page landed, then a message of that page deleted on Graph
- **WHEN** the next run resumes the round
- **THEN** every message the band still holds is listed, none shifted out of it

### Requirement: A flag push names what it adds and removes
A flag change SHALL reach a server as the flags it adds and the flags it removes (pimdir SYNC §4), never as a whole set replacing the server's: an IMAP `UID STORE +FLAGS` and `-FLAGS`, a Graph patch naming only the properties a flag of either set maps to (`isRead` for `\Seen`, the follow-up status for `\Flagged`), a Gmail modify naming only the labels either set maps to. A flag named in neither set SHALL be left as the server holds it, so a flag another client changed between the listing and the push survives; a change naming no flag the server maps SHALL send nothing.

An IANA keyword whose stored spelling no IMAP atom holds (`\Junk`, `\Important`) SHALL be written in its canonical form (`$Junk`), never abort the push; a spelling that is an atom SHALL be written as stored.

#### Scenario: Another client answers a message while the run removes its flag
- **GIVEN** a message listed seen and flagged, then answered on the server by another client
- **WHEN** the run pushes the removal of `\Flagged`
- **THEN** the message holds `\Seen` and `\Answered` on the server

### Requirement: A listing missing most of a collection waits for a person
A round of a sync whose listing leaves out more than half of at least 20 bound items in its scope, or names no member, none of its pages nor heads listing one, while any bound item is absent, and the rebuild of a renumbered collection whose listing leaves out more than half of at least 20 bound items or names none, SHALL drop nothing (pimdir SYNC §5, §8): such a listing reads as a truncated or emptied answer, an emptied collection and a failing server answering alike. The run SHALL report each under `absences`, with the side, the collection, the items left out and the items counted, and whether it was a rebuild, and SHALL exit 3: a rerun alone meets the same listing and parks again. The round a park leaves open SHALL NOT make the run incomplete. A parked rebuild SHALL keep the collection's handles and checkpoint, and the collection SHALL NOT be listed again that run.

`sync --accept-absences` SHALL confirm the absences for that run only, never stored: its rounds and rebuilds drop what they find absent.

#### Scenario: A mailbox listed nearly empty
- **GIVEN** an inbox bound to 25 messages, whose server lists one
- **WHEN** the account is synced
- **THEN** nothing is dropped, `absences` names 24 of 25, and the run exits 3

#### Scenario: The absences confirmed
- **GIVEN** the same inbox
- **WHEN** the account is synced with `--accept-absences`
- **THEN** the 24 messages are dropped

#### Scenario: An address book listed empty
- **GIVEN** an address book bound to one card, whose server lists none
- **WHEN** the account is synced
- **THEN** the card stays, `absences` names 1 of 1, and the run exits 3; synced with `--accept-absences`, the card is dropped

### Requirement: An edit names the body it was made over
A conflict resolution, a merge and a one-way overwrite SHALL state the body they were made over (the queued `update`'s `if_match`, the `Edit` mutation's, pimdir STORAGE §15.3, SYNC §7): an edit another write replaced since SHALL park rather than drop that write unmerged. A body staged with its action SHALL be staged in one step with it, so a refused action leaves no body in the tree.

### Requirement: A change over an unreadable body is withheld and reported
A change whose body is missing from the store or does not hash to its name SHALL be withheld, the placement staying as it was (pimdir STORAGE §7, SYNC §5), and the run SHALL report it under `unreadable` with the side, the collection and the count, and SHALL exit 3: the store needs a repair a rerun does not make.

### Requirement: A body a policy overruled is kept and reported
A body a policy did not adopt, kept by the store as a retained item (pimdir SYNC §9), SHALL be reported under `overruled` with the collection and the retained item's key, and either the side and the item's handle on it, for a body a side's write or a mutation settled, or the queue row (`action`), for one the drain kept aside applying a queued action, in a sync and in `sync --queue-only`. It SHALL be a warning and SHALL NOT change the exit code: the policy decided as configured.

### Requirement: A one-way overwrite keeps the body it discards
A one-way account overwriting a target's own edit, whether or not the source changed the item meanwhile, SHALL keep the target's body as a retained item and SHALL report it under `overruled`: `one-way` discards the target's change from its server, never from the store. A target body the store holds as wanted only SHALL be fetched before the overwrite settles it, the store refusing to overrule a body nobody read (`conflict-unread`); the fetch itself settles it by the target's `KeepLocal` (see "A fetched divergence settles by the endpoint's policy"), and one still unread after its fetch SHALL stay parked, never failing the run.

#### Scenario: A target edited while the source moved
- **GIVEN** a one-way account whose source and target agreed on a card, the source then changing it and the target editing it
- **WHEN** the account is synced
- **THEN** the target holds the source's card, the store keeps the target's edit as a retained item, and `overruled` names it

#### Scenario: A target edited alone
- **GIVEN** a one-way account whose source and target agreed on a card, the target alone then editing it
- **WHEN** the account is synced
- **THEN** the store's card is not changed, the target is overwritten with it, the store keeps the target's edit as a retained item, and `overruled` names it

#### Scenario: A target edit listed without its body
- **GIVEN** the same account, the target listing its edited card with no body
- **WHEN** the account is synced
- **THEN** the target's card is fetched first, then kept as a retained item and named under `overruled`, and the run does not fail

### Requirement: A fetched divergence settles by the endpoint's policy
Every upgrade neverest runs (a page's or phase 2's body landing, a full hydration, the copy hydration of a pair, the fetch of a diverging body before a merge or an overwrite) SHALL take the options of the endpoint's pushing sync (`PimdirUpgradeOptions::from(&sync_options)`): its conflict policy and whether it may push content, a dry run or an endpoint that writes nothing back pushing none. An upgrade fetching the body of a divergence then settles it by that policy (pimdir SYNC §6): a one-way target's `KeepLocal` keeps the store's body, owed to the target, and the target's own as a retained item; a one-way source's `prefer-remote` adopts the source's; a two-way endpoint's `manual` records it for the merge. A body an upgrade kept and did not adopt SHALL be reported under `overruled` with the endpoint and the item's handle, whichever upgrade kept it.

#### Scenario: A one-way target's edit read late
- **GIVEN** a one-way account whose target edited a card the source changed, the target's pull parking the divergence with no body
- **WHEN** the run fetches the target's body
- **THEN** the fetch settles it: the store's body is owed to the target, the target's own is a retained item, and `overruled` names it

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

### Requirement: A stated identity is named without its escape
A stated identity the store filed with a leading `esc:` (pimdir STORAGE §9), because it begins like a derived key, SHALL be named to a server without it, alone or inside a minted key; a derived key SHALL name none.

### Requirement: A push no server answered is pushed again
A push the connector gave no outcome for SHALL stay pending (pimdir SYNC §5), and the run SHALL report the count the last pass of each collection left under `unanswered`, with the side and the collection, and SHALL exit 4: the next run pushes them again.

### Requirement: A rejected create is reported once
A create the engine reports its server rejected (`rejected_creates`) SHALL be reported as a `rejected` error: with the reason the connector recorded, or as a `duplicates` error for a duplicate `UID`, else as a `create` whose server gave no reason. It SHALL be reported once whichever records it.

### Requirement: A resolution names what the push supports in part
`conflict resolve` SHALL stage its body in one step with the gate (`enqueue_with_body_checked`) and SHALL print the capabilities a source supports in part that the next push relies on, under `partial` in its JSON, one row each, `{capability, source, detail}`: `capability` as the store names it, `source` as the account's configuration names it, `detail` what the source leaves out or `null`. The text SHALL print a row as a line naming the source and the capability, then its detail.

### Requirement: The store's owner builds its search index
A build with the `search` feature (on by default) SHALL refresh the store's search index (index.db, pimdir SEARCH §3, §4) after each account a sync runs over, last, for at most the account's `store.index-timeout` (60 seconds unless set, a duration as `store.purge-after` reads one, `0` turning it off), and after a drain, a run performing the queued intents alone and a run declaring collections, for at most 2 seconds. A dry run SHALL NOT refresh. A refresh cut by its deadline SHALL leave the index behind the store and the next refresh SHALL go on from there.

The run SHALL report the refresh under `index`: the changes folded, the bodies newly indexed, the stamps still `behind`, whether another indexer held it (`busy`), and the error of a failed one. An index failure SHALL be a warning and SHALL NOT fail the run or change its exit code: the index is derived from the store.

`neverest sync --index-only` SHALL build the index of each store the accounts it runs over use, once a store and whole, every account of the store included, until it holds every change the store has, reading no credential and opening no endpoint, and SHALL exit 4 answering `busy` while another indexer holds one. An account whose store `init` never created SHALL fail as a sync fails it, listed under the result's `accounts` with the message naming `neverest init -a <name>` and `neverest sync --init -a <name>`, the run exiting 4, never failing the command with exit 1.

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

### Requirement: A store laid out by an earlier neverest is refused
When the default root holds no pimdir.db yet but one of its immediate subdirectories does, a store an earlier neverest kept per account, neverest SHALL refuse before creating anything, as `store-layout`, naming each such store and the order to follow: sync it with the neverest that wrote it until nothing is queued, move it out of the neverest directory, then run again. It SHALL NOT say to delete it, the store holding what no server has (its queued writes, its retained items). Once the default root holds a pimdir.db, its subdirectories are not looked at.

A store whose sidecar is of an earlier shape, a store directory holding a sidecar beside its database, where an earlier neverest kept it, or one holding a database and no sidecar under its local/, SHALL be refused the same way, as `store-layout`, naming the same order for that store. None is migrated: pimdir stores are reinstalled (pimdir STORAGE §6), and so is neverest's state in them.

#### Scenario: Stores kept per account
- **GIVEN** a neverest directory holding `work/pimdir.db` and `home/pimdir.db` and no pimdir.db of its own
- **WHEN** any command runs over an account naming no `store.root`
- **THEN** it exits 1 with `code` `store-layout`, naming both stores, the sync with the previous neverest until nothing is queued and the move out of the directory, no delete, and the directory holds nothing new

### Requirement: The sidecar holds each account
neverest.json, under the store's local/, SHALL record the collection-id layout once for the store, and, under each account it initialized, the mode it last synced under, whether a mode change was accepted, the roles a listing does not carry and the calendars a listing states take no tasks. An account SHALL be initialized when it has an entry: a run over an account with none SHALL fail that account, naming `neverest init -a <account>`.

`init` SHALL add the account's entry, creating the store and its sidecar in one act when there is none, and SHALL leave an account that has one as it is, reporting it initialized already rather than failing: its store may hold work no server has. `sync --reset` SHALL replace the entry; a run SHALL save its account's entry and leave every other as it read it.

#### Scenario: Two accounts, two modes
- **GIVEN** a store shared by a two-way account and a one-way one
- **WHEN** the two-way account turns `one-way = true` and is synced
- **THEN** that run is refused naming `--accept-mode`, and the one-way account still syncs

### Requirement: `-a` filters the accounts a command runs over
`-a/--account` SHALL be repeatable and SHALL filter the configured accounts. Without it, `sync` (its `-only` flags included), `check` and `init` SHALL run over every configured account, store by store and in name order within a store, one after the other inside one run holding each store's lock once; with it, over the named ones. The configuration is merged into a sorted table before neverest reads it, so name order is the only order it can keep. A name the configuration does not declare SHALL fail the command before anything runs, coded `account-missing`, in the family's words: "Account `x` not found, the configuration holds: a, b" (`default` among them, a name like any other: neverest has no default account); and so SHALL an account whose configuration does not validate, the configuration being one document.

Each account's failure SHALL be its own: the run SHALL go on with the next account and SHALL end with one result listing every account's outcome, a failed one carrying its error and its `code`. The command SHALL exit 0 when every account succeeded, otherwise with the partial-outcome code its own outcomes give (3 for a sync that left work waiting for a person), a failed or a busy account making it 4, the code of a run that could not do all its work. A failure common to the run (no configuration, a store refused as a whole) SHALL fail the command with the failure object, exit 1.

`sync --index-only` is store-wide: it SHALL build the stores of the accounts the filter keeps, each once and whole, every account of a store included. `conflict show`, `conflict resolve` and `sync --reset` address one item or start one account over, so they SHALL need exactly one account: a filter leaving none or several SHALL fail, naming the accounts. `conflict list` SHALL list the conflicts of every account it runs over, each row naming its account.

#### Scenario: Two accounts, one of them failing
- **GIVEN** two accounts, the first initialized and the second not
- **WHEN** `neverest sync --json` runs
- **THEN** the first is synced, the result lists both, the second with its error naming `neverest init -a <account>`, and the run exits 4

#### Scenario: An unknown account
- **GIVEN** a configuration declaring `work` and `home`
- **WHEN** `neverest sync -a perso` runs
- **THEN** it exits 1 before anything runs, coded `account-missing`, its error "Account `perso` not found, the configuration holds: home, work"

#### Scenario: A reset needs one account
- **GIVEN** a configuration declaring `work` and `home`
- **WHEN** `neverest sync --reset` runs with no `-a`
- **THEN** it exits 1, naming both accounts, and resets nothing

### Requirement: A check names the accounts a store holds that no configuration names
`neverest check` SHALL report, under `orphans`, every account a store the run reads holds collections of (pimdir `list_accounts`) and no configured account sharing that store names, with the store: renaming an account's TOML key leaves its collections under the old name. The report SHALL say the key must be renamed back to reach them, and SHALL NOT delete them nor change the exit code.

#### Scenario: A renamed account
- **GIVEN** a store holding the collections of an account `old`, and a configuration naming the same store under `new`
- **WHEN** `neverest check --json` runs
- **THEN** `orphans` names `old` with its store, and the collections of `old` are still there

### Requirement: The owner's files live under local/
Neverest SHALL keep every file of its own that follows a store under the store's local/, the subdirectory pimdir reserves for an owner's state (pimdir STORAGE §3, §6): the sidecar neverest.json, the sync.lock and the `submitting` mark, and no file of its own beside the store's. It SHALL create local/ `0700` and each file in it `0600` (pimdir STORAGE §18), never through a link or a file standing under the name, and SHALL NOT create the store directory on the way. A dry run's replica SHALL carry local/ with the rest of the store, its account reading initialized there, and still open as a store.

#### Scenario: A store after its first sync
- **GIVEN** an account initialized and synced once
- **WHEN** its store directory is listed
- **THEN** neverest.json and sync.lock are under local/, a directory of its user alone, and nothing of neverest's stands beside pimdir.db

### Requirement: A remove relocates or deletes, and says which
A `Remove` carrying a target SHALL relocate its member into the target and answer the engine that it did (`relocated`, pimdir SYNC §4), which is what delivers the create the target holds. When the target already holds the member's identity it SHALL delete the member from its own collection instead, on the precondition the change carries, and answer no relocation, so the target never holds the identity twice and its create stays pending (pimdir SYNC §4, §5). The target is asked only for an identity its backend can search for, a `Message-ID` on IMAP and on Graph mail, and never under a minted key, whose target held the identity before the move. A target that cannot be asked SHALL reject the change. Every other backend relocates as before and says so.

#### Scenario: A message moved into a folder that holds it
- **GIVEN** an IMAP inbox and an archive both holding the message `a@x`, the store binding the inbox one alone
- **WHEN** the message is moved to the archive and the account syncs
- **THEN** the inbox copy is expunged, no `MOVE` is sent, and the archive holds the message once

### Requirement: An add its server does not name is accepted with no handle
An append the server takes without naming the member it made (an IMAP `APPEND` answering no `APPENDUID`, whose message a search by `Message-ID` does not name either, or states none) SHALL be answered accepted with no assigned handle (pimdir SYNC §4), never rejected: the engine records the create delivered, and the page naming the member lands it. The search SHALL name the whole `Message-ID` in its angle brackets, IMAP `HEADER` matching substrings, so a message whose `Message-ID` only holds it is never taken for the append, and SHALL take the highest UID it finds. It SHALL NOT be appended again by that run or a later one. The relay, which binds its copy around the engine, SHALL still fail on such an append, having no create a listing could land.

#### Scenario: An append on a server without UIDPLUS
- **GIVEN** a staged message stating no `Message-ID` and an IMAP server answering its `APPEND` with no `APPENDUID`
- **WHEN** the account syncs twice
- **THEN** the message is appended once, the run reports it waiting as `delivered`, and nothing is reported rejected

#### Scenario: An append searched for beside a longer Message-ID
- **GIVEN** an IMAP server without UIDPLUS answering the `APPEND` of the message `b@x` with no `APPENDUID`, then receiving `b@x.y` from another client
- **WHEN** the search names the appended message
- **THEN** it answers the UID of `<b@x>`, never that of `<b@x.y>`

### Requirement: A create left waiting is reported
The report SHALL list under `waiting`, for every collection a run synced on an endpoint, the creates its last pass left waiting rather than pushing (pimdir SYNC §5), each with its `side`, its `collection`, its key as `id` (the link id, never the provisional handle) and its `reason`: `out-of-scope`, its date lying outside the run's scope; `delivered`, its server having taken it with no listing naming it yet; `undeliverable`, the store holding neither a body nor a member to copy it from; `other`, a reason a later io-pimdir states. The text output SHALL print them under `Waiting`. A waiting create SHALL NOT count as a warning nor change the exit code.

### Requirement: A capped source says so, and keeps what it leaves unlisted
A source whose server lists at most a number of the items a collection holds, whatever it holds (an IMAP folder size limit, as Gmail's "Limit IMAP folders to contain no more than this many messages"), SHALL be declared by `<backend>.collection.capped = true`, false by default, a key of every backend's `collection` table. Neverest cannot learn a cap from a server, none stating it, so the configuration is what the connector knows it by (pimdir SYNC §5).

Every page of every round of such a source's collections SHALL state `capped` (pimdir SYNC §4), a rekey's listing included, and no delta nor head SHALL. A round closing on a capped last page finds no member absent: the based bindings it did not list are kept, and only a removal the source states drops one (an IMAP `VANISHED` under QRESYNC, a Gmail history deletion, a Graph `@removed`, a DAV sync removal). On a server stating none (IMAP without QRESYNC, Gmail's among them), a message deleted on the server so stays in the store while the key is set.

The report SHALL list under `unlisted`, for each collection a capped round of the run closed on, the count of based bindings the round did not list (the engine's `unlisted`), with its `side` and its `collection`, the most one pass of the run counted. The engine counts there a narrowed round handing no enumeration too, which no narrowed round of neverest's connectors does. The text output SHALL print them under `Unlisted`. They SHALL NOT count as a warning nor change the exit code: a capped source leaves the same part of a collection out at every round.

A rekey of a capped collection is guarded by its own park alone (pimdir SYNC §8, which states no rule for a cap): one whose listing leaves out half of at least 20 bindings is not written without `--accept-absences`, and one leaving out less drops them.

#### Scenario: Ten messages past the cap
- **GIVEN** an IMAP source set `collection.capped = true`, 25 messages of its inbox bound, and a server listing the 15 newest
- **WHEN** the account syncs and the round closes
- **THEN** the 25 stay bound, and the report names 10 unlisted in INBOX with no warning, where the same listing of a source not capped drops the 10

### Requirement: A member named apart from its create is reported
The report SHALL list under `disagreed` every member a listing named apart from a create of its own side holding its identity, their summaries disagreeing (pimdir SYNC §5, §6, the engine's `Disagreed`), once whatever the passes that met it, with its `side`, its `collection`, the member's handle as `id` and the create's key as `create`. It SHALL count as a warning and SHALL NOT change the exit code.

### Requirement: An open Graph series older than its window says so
A Graph series with no end SHALL be read within `OPEN_SERIES_YEARS` (five) years either side of today, its exceptions read from the later of its start and that floor (owner, 2026-11-19): reading one from its start costs a request per five years of its age at every read of its body (85 for a birthday Outlook stores from 1604), and a floor at its `createdDateTime` drops the exceptions of a series imported with them, Graph stamping that date at creation.

An exception before the floor is left out of the body, so a series starting before it SHALL NOT be dropped silently: each run that reads its body SHALL list it under `clipped`, once whatever the connections and passes that read it, with its `side`, its `collection`, its handle as `id` and the floor as `since` (RFC 3339 UTC, the window's first `startDateTime`). The text output SHALL print it among the warnings, naming the series and the floor. It SHALL count as a warning and SHALL NOT change the exit code: a rerun reads it the same way and no person can settle it.

The body read SHALL still be written to the other endpoints, as one read whole is: the exceptions inside the window are complete, and failing the read would fail every yearless birthday in every run, for good. An exception further than five years ahead is left out the same way and reported by no row: every open series has its ceiling there.

An exception before the floor is unknown on the Graph side, not removed (owner, 2026-11-19): a body read of a series the run notes clipped SHALL be completed, before the engine compares it, with each override (`RECURRENCE-ID` component) and each `EXDATE` value its base body (the one last synced with the Graph side) sets apart before the floor and the read lacks, with the `VTIMEZONE` each names that the read lacks. The merge so keeps them from the base, an edit or a removal of one on the other endpoint still taking effect. An occurrence counts as before the floor when its date is, when the instant it names in its `TZID` or in UTC is, or, for a wall time in no zone the time zone database names, when it is less than 14 hours past the floor, the furthest ahead of UTC a zone runs. A missing override or `EXDATE` inside the window is a removal on Graph as before. A series with no base, or read whole, is completed with nothing.

A push to Graph rewrites none of them away: a master is patched with what changed against Graph's own read, and an occurrence Graph holds that the document leaves out is not written back (as "A series is pushed whole or not at all" says), while a carried override or `EXDATE` is sent through its instance like any other, found by a listing around its date and patched only where it differs. Each carried one costs that listing at every push of the series. A rule or start changed on Graph is not compared against the base: the carried ones stay in the body until the other endpoint removes them.

No existing row fits: `coverage` is a collection's scope, `unlisted` counts members a capped listing left out, and neither names an item read in part; hence a key of its own.

#### Scenario: A birthday from 1604
- **GIVEN** a Graph calendar holding a yearly series with no end starting 1604-03-12, read on 2026-11-19
- **WHEN** a run reads its body
- **THEN** its instances are listed from 2021-11-19 to 2031-11-19, the body is synced, and `clipped` names the series with `since` 2021-11-19T00:00:00Z, one warning, exit 0

#### Scenario: A 2012 override the window leaves out
- **GIVEN** that birthday last synced with a CalDAV endpoint holding an override of its 2012 occurrence and one of next year's, both exceptions on Graph
- **WHEN** the master is renamed on Graph and a run reads its body, the instances listing naming neither override before 2021-11-19 nor the one of next year, deleted
- **THEN** the CalDAV endpoint is sent the renamed series still holding the 2012 override, next year's gone, nothing deleted, and `clipped` still names the series

#### Scenario: A series inside the window
- **GIVEN** a yearly series with no end starting 2025-11-19, read on 2026-11-19
- **WHEN** a run reads its body
- **THEN** it is read from its start and `clipped` is empty

### Requirement: A create naming no member is owed no body fetch
A create, pending or delivered, holds a provisional handle that names no member of its server (pimdir SYNC §2). Neverest SHALL NOT plan a body fetch under such a handle: the page that lands the create brings the handle its body is read under. A reset SHALL count a delivered create among the work no server is known to hold as the store holds it, as it counts a pending one, and refuse over it without `--force`.

### Requirement: An intent stating an unknown version stays pending
A `submit`, `calendar-reply`, `calendar-cancel`, `collection-create`, `collection-rename` or `collection-delete` row whose payload states an integer `v` other than the one neverest reads SHALL be left pending and untouched (pimdir STORAGE §15.3): read no further, performed by no run, counted no attempt and never parked, for a neverest that knows the version. A payload stating no `v`, or one that is no integer, names no version to wait for and SHALL park as malformed.

#### Scenario: A send queued by a newer frontend
- **GIVEN** a queued `submit` whose payload states `v` 2
- **WHEN** the account syncs
- **THEN** nothing is sent, the row reads pending with no attempt, and the report names no failed send

### Requirement: A server's refusal carries its code
A failure a server refused SHALL carry a code of the vocabulary Himalaya shares, read from the typed error of the protocol that answered it, wherever an error row or an account reports it, the failure object a failed command prints included, through pimalaya-cli's `with_code`:

| Code | When |
|---|---|
| `permission-denied` | an HTTP 403 that is no rate limit (Graph, Gmail, Google Calendar, Google People); a DAV 403 whose body names no precondition, or `need-privileges` (RFC 3744 §7.1.1); an IMAP completion naming `NOPERM` (RFC 5530 §3) |
| `quota-exceeded` | an HTTP 507; a DAV body naming `quota-not-exceeded` (RFC 4331 §6); Graph's `ErrorQuotaExceeded`; an IMAP completion naming `OVERQUOTA` (RFC 5530 §3); an SMTP 5yz reply naming a sending limit |
| `rate-limited` | an HTTP 429, or a Google 403 naming a rate limit or a quota, still answered after the waits neverest takes |
| `recipient-refused` | an SMTP `RCPT TO` refused (RFC 5321 §3.3), io-smtp offering none after the first; a sending limit is `quota-exceeded` instead |

A refusal none of these name SHALL carry no code, its reason kept in the row's `sources`. A DAV 403 naming another precondition is a body the server would not take (RFC 4791 §5.3.2, RFC 6352 §6.3.2), no matter of rights, and SHALL carry none; nor SHALL a 5xx.

The codes are read once a row is built, from the chain the client returned, so the typed errors the run reads back (a duplicate `UID`, an item gone) stay in it.

#### Scenario: A source given up on a quota
- GIVEN a Gmail source whose batched reads meet a daily quota, or a per-minute one asking a wait longer than `--max-wait`
- WHEN the collection the reads were for is reported
- THEN its row's `code` is `rate-limited`, and a batched read refused for rights reads `permission-denied`

#### Scenario: A full mailbox
- GIVEN an IMAP server answering an `APPEND` with `NO [OVERQUOTA]`
- WHEN a run reports the refused append
- THEN its `rejected` error's `code` is `quota-exceeded`

#### Scenario: A refused recipient
- GIVEN an SMTP server refusing the recipient of a queued send with `550 5.1.1`
- WHEN `sync --intents-only` runs
- THEN the send parks, its `submitted` error coded `recipient-refused` with the reply among its `sources`, and the run exits 3

#### Scenario: A read-only address book
- GIVEN a CardDAV server answering an update 403 with no precondition
- WHEN a run reports the refused update
- THEN its `rejected` error's `code` is `permission-denied`, where a 403 naming `valid-address-data` carries none

### Requirement: A run's failures are one list
A sync report SHALL list everything the run could not do in one `errors` list, pimalaya-cli's item error per failure (pimalaya-cli cairn/spec/error.md, *An item error is the failure object of one item*): `id` the item's handle on the side when the failure is one item's (a rejected write, a refused change), the public id of the item an intent addresses, `null` for a hunk, a store step, a send or an endpoint; `code`, `null` when the failure carries none; `error`, the outermost message; `sources`, the messages of its causes, outermost first. After those four keys each row SHALL carry `kind`, the list the report held that failure in before its failures were one list (owner, 2026-11-20, reversing the named failure lists of 2026-11-17), then the keys that kind needs:

| `kind` | Its own keys | Exit |
|---|---|---|
| `collection-patch` | `hunk`, the collection hunk that did not apply (a `*` scan of an endpoint that did not open, a pair's `sync`) | 4 |
| `item-patch` | `hunk`, the item hunk that did not apply | 4 |
| `failures` | `step` (`queue`, `bodies`, `conflicts`, `listing`) and `collection`, `null` for a store-wide step | 4 |
| `rejected` | `side`, `collection`, `action` (`update`, `append`, `copy`, `delete`, `move`, `set-flags`) | 3 |
| `duplicates` | `side`, `collection`, `uid`, coded `item-duplicate` | 3 |
| `refusals` | `side`, `collection`, the reason the store keeps as `error` | 3 |
| `submitted` | `row`, `collection`, `subject`, `parked`, `unconfirmed` | 3 parked, else 4 |
| `intents` | `row`, `collection`, `intent`, `source`, `parked` | 3 parked, else 4 |
| `unreached` | `endpoint` | 4 |

A row waiting for a person exits 3, any other 4, which wins. The lists reporting what the run did (`collectionPatch`, `itemPatch`, `submitted`, `intents`, `drained`) SHALL hold what it did alone; the lists reporting a state rather than a failure SHALL stay lists of their own: `conflicts`, `parked` (queue rows the store holds, as `pimdir queue list` prints them), `absences`, `unreadable`, `unanswered`, `throttled`, `unsettled`, `coverage`, `collisions`, `overruled`, `forbidden`, `waiting`, `disagreed`, `unlisted` and `clipped`. The text output SHALL print a row's message and its causes as one chain, as `{err:#}` reads, a `rejected`, `duplicates` or `refusals` row among the warnings and a failed hunk or step among the errors.

An unconfirmed send's `error` SHALL begin `unconfirmed: `, its causes under `sources`, so its message and causes joined read as its parked row does.

An account's row of `sync`, `sync --queue-only` and `init` and the index refresh SHALL carry `code`, `error` and `sources` the same way, `error` the outermost message, `null` and `[]` when it ran: an account that did not run is the account's own outcome, not one item's. Their `code` SHALL be read as a row's is, a server's refusal included; the index refresh's is the store's (`index-version`, `index-fts5-missing`). An account's row of `init` SHALL list the endpoints it opened under `opened`, sorted, those it opened before a failure included, empty for an account initialized already.

#### Scenario: A refused update under `--json`
- GIVEN a CardDAV source refusing an update 403
- WHEN `neverest --json sync` reports it
- THEN `errors` holds `{id, code: "permission-denied", error: "Cannot update <id> in <collection>", sources: ["WebDAV server returned HTTP 403…"], kind: "rejected", side, collection, action: "update"}`, `itemPatch` no update of it, and the text prints `Cannot update <id> in <collection>: WebDAV server returned HTTP 403…`

#### Scenario: A sync failing two ways
- GIVEN a sync whose CardDAV source refused one update and whose CalDAV source was not reached
- WHEN its result prints with `--json`
- THEN `errors` holds a `rejected` row, an `unreached` row naming `caldav` under `endpoint` and a `collection-patch` row of its `*` scan, and the run exits 4

#### Scenario: An account that failed
- GIVEN an account whose store cannot be opened
- WHEN `neverest --json sync` reports it
- THEN its row reads `error` the outermost message and `sources` its causes, and the text prints them as one chain

#### Scenario: An account `init` could not open
- GIVEN an account whose IMAP server refuses the connection
- WHEN `neverest --json init` reports it
- THEN its row reads `error: "Initialize imap"`, the refusal under `sources`, `opened: []`, and the run exits 4

### Requirement: A queue row is keyed `row`
Every report row naming a row of the store's queue SHALL name it `row`, as `pimdir queue list` does: `applied` and `parked` of a drain; `parked`, `submitted` and `intents` of a sync, and the `submitted` and `intents` errors; and an overruled body a queued action settled (`id` naming the item a side's write settled stays `id`). A parked row, of a drain or of a sync, SHALL be the queue row pimalaya-cli names (cairn/spec/output.md, *A queue row keeps one shape*), `{row, handle, kind, collection, linkId, itemId, producer, queuedAt, attempts, unconfirmed, error}`: `handle` the row under the store's identity, `<store-id>.<incarnation>.<row>` (pimdir STORAGE §4.5), which a frontend's `pimdir queue show` and `cancel` take; `kind` the action as the store spells it, kebab-case (`add`, `update`, `set-flags`, `remove`, `move`, `copy`, `submit`, `calendar-reply`…); `linkId` the link id of the item an `add` or an intent creates, `itemId` the public id of the stored item the row addresses, each `null` when the payload names none or does not decode; `queuedAt` the instant the store stamped the row with; `attempts` how many times it was tried before it was parked; `unconfirmed` whether its outcome is unknown, its `error` then beginning `unconfirmed: `; `error` the text the store recorded. Neverest syncs every domain, so no domain label follows. A drained row SHALL read `{row, collection, kind, seq}`. A rejected write SHALL spell its `action` the same way (`set-flags`).

#### Scenario: A parked flag change
- GIVEN a `set-flags` row himalaya queued, which the drain parks
- WHEN `neverest --json sync --queue-only` reports it
- THEN its `parked` row reads `{row, handle: "<store-id>.<incarnation>.<row>", kind: "set-flags", collection, linkId: null, itemId: "<seq>", producer: "himalaya", queuedAt, attempts, unconfirmed: false, error}`, and the next `--queue-only` reports it again and exits 3

### Requirement: A DAV source may name its home set
A `carddav` or `caldav` table SHALL take `home`, the address book or calendar home set as a full URL, as cardamum and calendula do: the session SHALL connect to it and list its collections there, discovering no principal and no home set, `server` then being ignored. Without `home`, `server` SHALL be required and the home set discovered from it as before. A table naming neither SHALL fail when the account resolves, naming both keys.

The `auth` table SHALL also take `"none"`, for a server asking for no credential: no `Authorization` header is sent. A table naming no `auth` SHALL read as `"none"`, anonymous DAV as the family reads it (a Radicale with no auth, a server behind a VPN or a proxy that authenticates), and a 401 such a session meets SHALL say the account configures no `auth`, so a forgotten table reads as one.

A configured host SHALL be ASCII on every scheme, a `server` or a `home`, refused at load otherwise and naming its Punycode form (`xn--...`): no neverest build carries a Unicode IDNA back end, `idna_adapter` being pinned to 1.0.0 as in every family binary.

#### Scenario: Google's address books
- GIVEN `carddav.home = "https://www.googleapis.com/carddav/v1/principals/<email>/lists/"` and a bearer token
- WHEN the source opens
- THEN its address books are listed under that URL, no `PROPFIND` for a principal sent first

### Requirement: An IMAP source takes the SASL-IR and ID quirks
An `imap` table SHALL take `sasl-ir`, forcing the RFC 4959 initial response on or off, unset following the advertised `SASL-IR` capability; and `id.auto` with `id.fields`, as himalaya and carillon do: with `id.auto = true` the session sends an RFC 2971 `ID` straight after authenticating, each of `id.fields` set to `true` carrying neverest's value for a well-known key (`name`, `version`, `vendor`, `support-url`) and `NIL` otherwise, none at all sending `ID NIL`. Every connection of the source, a reopened one included, opens the same way.

#### Scenario: A server requiring an ID
- GIVEN `imap.id.auto = true` and `imap.id.fields.name = true`
- WHEN the source opens
- THEN the session sends `ID ("name" "neverest")` after authenticating

### Requirement: A connection goes through the configured proxy
An account SHALL take `proxy.url`, `proxy.username` and `proxy.password` (a secret), and so SHALL every backend table and every `smtp` table, as the sibling CLIs do: `url` a `socks5://`, `socks5h://` or `http://` proxy URL, `password` requiring `username`. Every connection a source opens, its send channel's included, SHALL go through the proxy its own table names, else the account's, else the one the `all_proxy` and `https_proxy` environment variables name when the connection opens, else none.

#### Scenario: One source through a SOCKS proxy
- GIVEN an account naming `proxy.url = "http://proxy.example.org:3128"` and one source naming `proxy.url = "socks5h://127.0.0.1:1080"`
- WHEN a sync opens the account's sources
- THEN that source connects through the SOCKS proxy and every other through the HTTP one

### Requirement: A conflict's id prints as a string
`conflict list`, `conflict show` and `conflict resolve` SHALL print the item's public id (the store's `seq`) under `id` as a string, as every Pimalaya binary prints a store id, and the item's handle on the source under `remoteId` (`handle` is the queue row's key, pimalaya-cli cairn/spec/output.md), `source` and `remoteId` `null` for the item's own conflict (pimdir SYNC §9), two endpoints disagreeing, which is no one source's and is addressed by its `id` alone; `conflict show` and `conflict resolve` SHALL take the `id` on the command line as `conflict list` prints it. A sync report's conflict row SHALL print the same `id`, `null` while the store names the item by none, the same `collection` as `conflict list` spells it, the store's `<account>/<namespace>/<name>`, the `side` and the handle as `remoteId`, so a script resolves the conflict a run reported. An id no conflict of the account has SHALL fail coded `conflict-not-found`, exiting 1.

#### Scenario: A script settles the conflict it listed
- GIVEN a store holding one divergence on item 42
- WHEN `neverest conflict list --json` lists it
- THEN its row reads `id: "42"`, and `neverest conflict resolve "42" --prefer-local` settles it

#### Scenario: A script settles the conflict a run reported
- GIVEN a run that parked a divergence on a card, item 42 of `work/carddav/contacts`
- WHEN `neverest sync --json` reports it
- THEN its `conflicts` row reads `{id: "42", collection: "work/carddav/contacts", side: "carddav", remoteId: "<href>"}`, and `neverest conflict show 42` shows it

### Requirement: A report lists its patches as arrays
A sync report SHALL list the hunks it applied under `collectionPatch` and `itemPatch`, each an array of the hunks, in the order the run applied them; a hunk that did not apply SHALL be a `collection-patch` or `item-patch` error instead ("A run's failures are one list"). The report SHALL NOT repeat `account`, which the account row around it names.

#### Scenario: A quiescent account
- GIVEN an account whose collections are in sync
- WHEN `neverest sync --json` reports it
- THEN its row reads `account` and its `report` reads `collectionPatch: []` and `itemPatch: []`, with no `account`, `collection` or `item` key

### Requirement: The rules the code's comments carried
The rules below were kept in inline comments until the-v1-review removed every `//` comment (D5); they are behaviour, so they live here.

- A Graph mail round widening the coverage SHALL list the missing band from that band's floor (by `sentDateTime`), `Date` deciding what belongs to the band, and SHALL keep the stored delta link.
- A member a Graph band head named, and the store did not bind when the run began, SHALL NOT be treated as held: the pass reads its summary so the round's last page lists it again; only a member a band page listed this run, marks unchanged, is left out.
- A failed Graph reopen SHALL leave no earlier attempt's exchange record: the record is cleared before the reopen, which the "unanswered" verdict of a send depends on.
- A DAV `MOVE` SHALL be retried as a create is: a 503 is never resent, the target possibly existing, a 429 is.
- Debug builds SHALL read `NEVEREST_IMAP_NO_QRESYNC`, treating the server as lacking QRESYNC so tests reach band heads against a QRESYNC server; release builds SHALL NOT read it.
- Gmail SHALL match label names ignoring case: a rename changing only the case finds its own label under the new name, which is no conflict.
- A queued collection or calendar intent whose payload does not decode names no source: it SHALL go to a side of its collection's namespace (for a calendar intent, the side binding the item), which parks it as malformed.
- On an account syncing its source alone, a collection a delete intent removed from the server SHALL leave the store too, unless a row still queued on it keeps it.
- The `Abandoned` outcome comes only from an `Abandon`, which no neverest command stages (pimdir SYNC §7); it SHALL only be logged.
- Gmail SHALL stay on one connection until a second one beside a batch is measured against its per-user concurrency; Google Calendar and People meter the whole account, and DAV gains nothing in parallel.
- Phase 2 hydration plans SHALL be read once phase 1 is over, so a body a landed page kept or found gone is not downloaded again; bodies settled by policy while landing are drained from each connection's notes after phase 2.
- Landing bodies SHALL take a collection's gate before the applying lock, one collection at a time, so no applier holds two gates.
- A conflict resolution SHALL settle only the divergence it showed: its queued update's `resolves` names the remote revision shown and no other, so a conflict never seen parks the edit (pimdir STORAGE §15.3).
- A short multiget batch SHALL NOT count as done: its missing members are fetched one by one.
- A sync state row with no checkpoint SHALL read as no sync state, and reading then writing a checkpoint SHALL run in one transaction.
- A dry run SHALL NOT upgrade a probe to `Full` to name what it would fetch (a DAV kind resolves its link id only at `Full`), nor key its plan on the hub level (a content change drops the stale object while the hub keeps the level).
- The io-pimdir error codes neverest never meets are left out of its codes: `index-busy` (answered as busy), `index-missing`, `index-outdated` and `index-query-only` (a query client's), `index-zone-unknown` (neverest states only zones its database names), `cutoff-invalid` and `scope-invalid` (neverest writes the shapes the store holds), `store-reset` (a row is named by its bare id), `action-not-intent` (only a `submit` is parked unconfirmed), `backup-occupied` (no backup is taken) and `conflict-unread` (the one-way overwrite fetches the unread body, an item still unread staying parked).

### Requirement: A connection presents a client certificate when configured
Every `tls` table (a backend's, `smtp`'s) SHALL read an optional `client` table naming a PEM certificate (`cert`, optionally followed by its intermediates) and its PEM private key (`key`, PKCS#8 under native-tls), both shell-expanded, and the connection SHALL present them when the server asks for one, as himalaya does. A `client` table missing either key SHALL be refused at load.

### Requirement: An account's TLS reaches every endpoint field by field
An account SHALL take a `tls` table, as himalaya does and with its keys (`provider`, `rustls.crypto`, `cert`, `client.cert`, `client.key`; pimalaya-config cairn/spec/toml.md, *Every binary spells its TLS table alike*), and every `tls` table of its endpoints (sugar, `sources.<name>`, `targets.<name>`) and of their `smtp` channels SHALL take each field it leaves unset from the account's: the provider, the rustls crypto, the trusted certificate, and the client certificate and its key as one unit. A field an endpoint sets SHALL win. The inheritance SHALL hold wherever an endpoint is opened or checked, the namespaced account a run builds included. The type SHALL stay neverest's own, a copy of himalaya's: a shared type would be rewritten whole by a binary extending one field.

#### Scenario: One CA for every endpoint, one provider overridden
- GIVEN an account naming `tls.provider = "native-tls"` and `tls.cert = "/etc/ca.pem"`, an `imap` source naming `tls.provider = "rustls"` and an `smtp` channel naming no `tls`
- WHEN `neverest --json account check` runs
- THEN the `imap` row's `tls` reads `{provider: "rustls", crypto: null, cert: "/etc/ca.pem", clientCert: null}` and its `smtp` row's `{provider: "native-tls", crypto: null, cert: "/etc/ca.pem", clientCert: null}` (src/config.rs `the_account_tls_is_inherited_field_by_field`)

### Requirement: An unknown conflict and an uninitialized account are coded
A `conflict show` or `conflict resolve` naming an id no conflict of the account has SHALL fail coded `conflict-not-found`, exit 1. An account a run reaches that no `init` initialized SHALL fail coded `account-uninitialized`, its message naming `neverest init -a <name>` and `neverest sync --init -a <name>`, in a sync's account row, a drain's and a conflict command's failure alike. A create a side refuses for a duplicate `UID` SHALL be coded `item-duplicate`, the code the domains share (pimalaya-cli cairn/spec/error.md).

#### Scenario: A sync over an uninitialized account
- GIVEN a configured account no `init` ran over
- WHEN `neverest --json sync --queue-only` runs
- THEN its row reads `code: "account-uninitialized"` and the run exits 4
