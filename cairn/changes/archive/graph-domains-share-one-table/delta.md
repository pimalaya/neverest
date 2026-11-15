---
cairn: delta
change: graph-domains-share-one-table
---

# Delta

## ADDED Requirements

### Requirement: One Graph table keeps the source ids of the three it replaced
Writing Graph as one `msgraph` table, where `msgraph-contacts` and `msgraph-calendar` were tables of their own, SHALL move nothing in a store: every source id a configuration of the three tables produced SHALL be produced by its rewrite in the one table, so the first run of the new build finds every collection, binding and checkpoint where the last run of the old one left them, creating, listing whole and orphaning none. The sugar keeps the three names ("A backend under the account is a source named after its protocol"); a named source keeps its name, `sources.<name>.msgraph-contacts` becoming `sources.<name>.msgraph` with the `contacts` domain alone. A store SHALL NOT be migrated for it, nothing in a store naming a table.

#### Scenario: A store synced under the three tables
- GIVEN a store an account synced under `msgraph.*`, `msgraph-contacts.*` and `msgraph-calendar.*`
- WHEN the account is rewritten as one `msgraph` table naming `mail`, `contacts` and `calendar`, and synced
- THEN mail and contacts resume from their stored delta links, the calendars list the same events, and the store holds the same sources, collections and bindings, none created

## MODIFIED Requirements

### Requirement: A backend under the account is a source named after its protocol
A backend table written directly under the account (`imap`, `carddav`, `caldav`, `jmap`, `gmail`, `msgraph`, `gpeople`, `gcal`) SHALL be sugar for `sources.<key>.<key>`, the source taking the key as its name. The `msgraph` table SHALL be sugar for one source a domain it names, each named as the table the domain had before: `msgraph` for `mail`, `msgraph-contacts` for `contacts`, `msgraph-calendar` for `calendar`, each holding the table's session and that domain's table alone. The sugar SHALL produce a configuration indistinguishable from the expanded form, source id included, so expanding it by hand is a no-op on the store.

A key names a protocol, and where a protocol carries several domains a table under it names the domain: `carddav` and `caldav` are two keys over one adapter, `msgraph.mail`, `msgraph.contacts` and `msgraph.calendar` three tables over one session. The domain is what the kind falls out of, so it SHALL be distinct per kind even when the transport is shared.

Declaring the same key both directly and under `sources` SHALL be a configuration error rather than a merge.

#### Scenario: Expanding the sugar changes nothing
- GIVEN an account written as `imap.server = "..."`
- WHEN it is rewritten as `sources.imap.imap.server = "..."`
- THEN the sync opens the same source id and reuses every existing binding

#### Scenario: Expanding the Graph table changes nothing
- GIVEN an account written as `msgraph.auth.token.raw = "..."` with `msgraph.mail = true` and `msgraph.contacts = true`
- WHEN it is rewritten as `sources.msgraph.msgraph.mail = true` and `sources.msgraph-contacts.msgraph.contacts = true`, each with the token
- THEN the sync opens the same two source ids and reuses every existing binding

### Requirement: The configuration refuses every key it does not read
The configuration SHALL refuse at load, naming it, every key it does not read, at every level: the document root, which holds `accounts` alone; an account; a `sources.<name>` or `targets.<name>` table; a backend table, written under a source or directly under the account; and every table nested in one (`tls`, `sasl`, `auth`, `collection`, `item`, `flag`, `smtp`, `store`, `conflict`, an `msgraph` domain table). A source table naming no backend table, or more than one, SHALL be refused at load, naming the backends it found.

A key an earlier neverest read and removed SHALL be refused by name in the family sentence, pimalaya-config's `removed_key!`: "The `x` key is removed, <why>: <what to write instead>, see MIGRATION.md", `table` in place of `key` for a table, as the file is parsed, so in an account no command opens as in one it does: `left` and `right`, `folder`, `envelope`, the account-level `collection`, `mailbox` and `message`, `collection.namespace` on any source (one written directly under the account included), the `mailbox` and `message` tables and the `collection.filters` key of any backend table, the `msgraph-contacts` and `msgraph-calendar` tables under the account, a source or a target, the `collection`, `item` and `flag` tables and the `pool-size` key written directly in an `msgraph` table, `store.retention` and `store.hydration`, and the flat `login` and `password` of an `smtp` table. No key SHALL be read under an old spelling: an alias either loads a configuration the person thinks is refused or refuses it under a name the person never wrote.

#### Scenario: A typo beside a source's backend
- GIVEN `sources.work.imap.server = "…"` and `sources.work.imap-typo = 1`
- WHEN any command loads the configuration
- THEN it fails naming the unknown field `imap-typo`, where the key used to be ignored

#### Scenario: Two backends in one source
- GIVEN a source naming an `imap` and a `jmap` table
- WHEN the configuration is loaded
- THEN it fails naming `imap` and `jmap`, where the second used to be dropped

#### Scenario: A v0.1 table
- GIVEN an account carrying `folder.filter = "all"`
- WHEN a command opens it
- THEN it is refused with "The `folder` table is removed, …: write its filter as `<protocol>.collection.filter`, …, see MIGRATION.md"

#### Scenario: A v0.2 mail spelling
- GIVEN an account carrying `imap.mailbox.create = false`
- WHEN the configuration is loaded
- THEN it is refused with "The `mailbox` table is removed, …: write `collection` in its place, as in `imap.collection.filter`, see MIGRATION.md", where it loaded as `imap.collection.create`

#### Scenario: A v0.3 Graph contacts table
- GIVEN an account carrying `msgraph-contacts.auth.token.command = "…"`
- WHEN the configuration is loaded
- THEN it is refused with "The `msgraph-contacts` table is removed, …: write `msgraph.contacts`, …, see MIGRATION.md"

### Requirement: The account group is the family's
Neverest SHALL carry pimalaya-cli's `account` group (pimalaya-cli cairn/spec/commands.md), over the accounts `-a` names, every one when it names none, as every neverest command:

- `account check` SHALL be the check the *A check …* requirements describe, and `check`, its name before the group, SHALL run it too, hidden from the help, its JSON schema kept under `neverest-check` beside `neverest-account-check`. It SHALL keep its own report rather than pimalaya-cli's one-account `AccountCheckOutput`: a run covers several accounts, each with its mode, every endpoint's collections and capabilities and the orphans a store holds; its rows carry the shared backend check keys (`backend`, `ok`, `code`, `error`), the send channel's being pimalaya-cli's `BackendCheck`. A row's `backend` SHALL be the backend table the source writes, `msgraph` for every Graph domain.
- `account list` SHALL print pimalaya-cli's `AccountListOutput`, one `{name, default, backends, notBuilt}` row an account, sorted by name, reaching nothing. `backends` SHALL hold the backend tables the account writes, its sources' and its targets', sugar and named alike, each once, in the order `imap`, `carddav`, `caldav`, `jmap`, `gmail`, `msgraph`, `gpeople`, `gcal`, `smtp` (a send channel), those this build serves; `notBuilt` those it leaves out, JMAP always (it parses and no build opens it). A source's own name is the account's, not a backend, and reads in `account check`. `default` SHALL be `true` on every account: a command naming no `-a` runs over them all, the `default` key being ignored.

Neverest has no `account capability`: what each source can do reads in `account check`, from the configuration.

#### Scenario: Listing a mail account and a JMAP one
- GIVEN an account `a` with an `imap` table and an `smtp` table, and an account `b` with a `jmap` table
- WHEN `neverest --json account list` runs
- THEN it prints `a` with `backends: ["imap", "smtp"]` and `b` with `notBuilt: ["jmap"]`, both `default: true`, and exits 0 without opening anything

#### Scenario: A script calling `check`
- GIVEN a script running `neverest --json check`
- WHEN the account group lands
- THEN it prints the report `account check` prints, and `check` is not listed in the help

### Requirement: Microsoft Graph is a first-class source
An `msgraph` source SHALL open protocol-direct over io-msgraph (never through a frozen aggregator). Graph carries several PIM domains behind one protocol, one host and one credential, so one `msgraph` table SHALL hold the session (`user-id`, `tls`, `alpn`, `auth`, `proxy`) and name the domains it syncs by their tables, as carillon, cardamum and calendula read one `msgraph` table: `msgraph.mail` the mail folders, `msgraph.contacts` the address book, `msgraph.calendar` the calendars. A domain table SHALL hold that domain's source keys (`collection`, `flag`, `item`, `pool-size`), or be written `true` to take their defaults, `false` reading as absent. A table naming no domain SHALL be refused at load, naming `msgraph.mail = true`. Each domain SHALL be a source of its own: under the account as the sugar names them, under `sources.<name>` or `targets.<name>` exactly one, a named table naming two or more being refused at load. One adapter SHALL serve every domain, parameterised by which it speaks, as one adapter serves both DAV protocols. A value naming the kind (`kind = "…"`) SHALL NOT be introduced for this or any other backend.

For **calendars** (`msgraph.calendar`): the user's calendars keyed by id and named by name, events as `text/calendar` through io-msgraph's `ical` projection. Graph's event delta only runs over a calendar view, a time window, and an event leaving a sliding window would read as deleted and be deleted on every other source, so the enumeration SHALL list each calendar's lone events and series masters in full every run, with no window, the master's `changeKey` as the revision and removals reported by absence; an event Graph repeats across `@odata.nextLink` pages SHALL be enumerated once, and one id at two revisions SHALL fail the enumeration. A series SHALL be one item, read with its exceptions from the instances of its own date range, bounded by its range type (a `numbered` range sized from its occurrence count, an open-ended one five years either side of today), never by the `0001-01-01` `endDate` Graph fills the other types with, and listed in windows of at most five years, the longest Graph accepts, its cancelled occurrences as EXDATEs. The UID SHALL ride the projection's stash extended property, a create whose UID did not survive being deleted again and refused; an event with no stashed UID SHALL be keyed by Graph's `iCalUId`, unwrapped when it is a global object id wrapping the organizer's UID (an invitation Exchange took in by mail, the `vCal-Uid` form), so the event keeps the UID the organizer and the other attendees know; an update or delete SHALL be refused when the master's `changeKey` moved. A series SHALL be written whole, as "A series is pushed whole or not at all" says, and a floating time anchored, as "A floating time is anchored before Google or Graph takes it" says; an edit Graph makes to one occurrence alone is seen once the master's `changeKey` moves.

For **mail** (`msgraph.mail`): folders listed two levels deep (`Parent/Child` naming), enumeration through the messages delta query carrying the `@odata.deltaLink` as the engine's opaque checkpoint (HTTP 410 = expired link, restarting a fresh full round; any other failure surfaces), the `Meta` tier read by one summary get a message (`mid:`/`alt:` link ids, meta v1), the `Full` tier from the raw MIME content streamed into the blob store. Flags map to the IANA wire spellings (`isRead` = `\Seen`, a flagged follow-up = `\Flagged`, `isDraft` = `\Draft`). Push scope is honest: flag changes push through `message_update`, deletes through `message_delete`, moves and copies as the requirement "Graph mail moves, copies and adds drafts" says; an append lands in Drafts only, and content updates are rejected and documented.

Auth SHALL be a bearer access token only, resolved through the standard secret-command idiom (`auth.token.raw` / `auth.token.command`) once per run with every other credential; neverest SHALL NOT run any OAuth flow itself (no device sign-in, no client credentials, no token persistence): acquiring and refreshing the token is delegated to an external command, typically ortie. The token SHALL carry the scopes the declared domains need, which config.sample.toml names per domain so the first run does not fail on a bare 403. No token is ever logged.

### Requirement: A Graph contacts source syncs the address book
An `msgraph` table MAY name the `contacts` domain, whose items are `text/vcard` cards and whose collections are the user's contact folders, listed two levels deep, keyed by folder id and named by display name. The default Contacts folder, which the folders endpoint does not list, SHALL be the collection `contacts`, addressed by omitting the folder segment.

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

### Requirement: A source's backend declares the kind it syncs
Every sync backend SHALL declare the IANA media type of the items it syncs, and that type SHALL be recorded as the pimdir collection's `kind`. The kind SHALL be derived from the backend table the source writes and, on Graph, the domain table under it, never declared by a value in the configuration: one backend and one domain per source.

The media type SHALL also be knowable from the protocol alone, without opening a connection. A pair that disagrees SHALL still fail with a clear error before any store write, since a mailbox cannot reconcile against an address book. A store MAY hold collections of several kinds, which is what pimdir is built for, and an account MAY now feed it several.

## REMOVED Requirements

None.
