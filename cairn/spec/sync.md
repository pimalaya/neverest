---
cairn: spec
capability: sync
status: current
---

# Sync

`neverest sync` reconciles an account's named sources through the io-replica
engine over one pimdir store. It is sync-on-demand: one reconcile per
invocation, no daemon.

### Requirement: An account holds named sources
An account SHALL hold a map of named sources (`sources.<name>`), each declaring
exactly one remote backend, and SHALL require at least one. It MAY additionally
hold a map of named targets (`targets.<name>`) on the same terms. Both map keys
SHALL be pimdir source ids, so a name is what every binding it owns in the store
is recorded against, and a rename SHALL be treated as a new source. One name
SHALL NOT be both a source and a target.

An account SHALL NOT constrain its sources to one kind: mail, contacts and
calendar sources may sit under one account, and their collections never meet
(see the collection key requirement).

`left` and `right` SHALL NOT survive in any form, as keys, as aliases, or as
source ids. A configuration carrying them SHALL be refused at load, naming
`sources` and `targets` and the `one-way` flag that declares the direction they
never could.

`collection.namespace` SHALL likewise be refused by name, naming `targets` and
`one-way`. It expressed which sources met, which is now the arity, and never
expressed which way, which is now `one-way`.

A store written before collection ids carried their namespace SHALL NOT be read.
Neverest keeps its own state beside the store (`neverest.json`) recording the
collection-id layout and the account's mode, and a store directory holding a
database but no such file SHALL be refused, naming `sync --reset`.

Whatever materializes the database SHALL write the sidecar in the same act, so
that pair only ever describes a store an older neverest wrote. `init` SHALL
stamp the store it creates and `sync --reset` SHALL stamp the store it recreates.

#### Scenario: Mail and contacts under one account
- GIVEN an account declaring an IMAP source and a CardDAV source
- WHEN it is synced
- THEN both run against the same store, and neither is refused for disagreeing on its kind

#### Scenario: A two-side config is refused with its replacement
- GIVEN a configuration written with `left` and `right`
- WHEN it is loaded
- THEN it is refused, naming `sources`, `targets` and `one-way`

#### Scenario: A fresh account syncs
- GIVEN an account whose store `init` has just created
- WHEN `sync` runs, with or without `--dry-run`
- THEN the store is read rather than refused as the unnamespaced ancestor

#### Scenario: The named remedy clears the refusal
- GIVEN a store refused for holding a database with no sidecar
- WHEN `sync --reset` runs
- THEN the recreated store carries a sidecar and the next run reads it

### Requirement: The mode is the arity and two flags
An account SHALL declare `sources`, optionally `targets`, and the flags `one-way`
and `retain`. Its mode SHALL be those, and SHALL NOT be inferred from any other
property:

| sources | targets | `one-way` | behaviour |
|---|---|---|---|
| 1 | 1 | false | two-way mirror between the two remotes |
| 1 | 1 | true | source overwrites target |
| 1 | N | true | source overwrites each target |
| N | 0 | false | each source merges two-way with the local store |
| N | 0 | true | sources overwrite the local store, local edits discarded |

Every other combination SHALL be refused at load, naming the cell reached and the
nearest legal one. One source with several targets and `one-way = false` SHALL be
refused specifically: bidirectional propagation across more than two endpoints has
no resolution order, and syncing them pairwise in configuration order would be a
tiebreak the user never wrote.

In the no-targets cases the sources SHALL stay isolated from one another. An item
held by one is never pushed to another, and a frontend unions them at display
time.

#### Scenario: A plural typo cannot change the direction
- GIVEN an account declaring `sources` and `targets`
- WHEN a key is misspelled
- THEN the load fails naming the unknown field, rather than matching a different mode

#### Scenario: An illegal cell names its neighbour
- GIVEN an account declaring one source, two targets and no `one-way`
- WHEN it is loaded
- THEN it is refused, naming the missing `one-way = true` rather than picking an order

### Requirement: `one-way` declares authority, not silence
`one-way = true` SHALL make the `sources` side authoritative: a difference is
resolved in its favour and the other side's change is discarded rather than
merged, so no conflict is recorded and no divergence is reported.

It SHALL NOT mean the other side goes unread. Every run SHALL still enumerate the
target, or every item would be re-pushed on every run; the target's state decides
what the run has left to do and never who wins. The documentation SHALL say
"overwritten, not merged" in those words, because a user arriving from the
two-way mode will otherwise expect a conflict report that cannot come.

#### Scenario: A one-way run copies the difference, not the collection
- GIVEN a one-way account whose previous run completed
- WHEN it runs again with nothing changed on the source
- THEN the target is enumerated, nothing is written, and the run is quiescent

### Requirement: pimdir is the ledger, and `retain` makes it a replica
The pimdir store SHALL NOT be nameable as a source or a target. It is the ledger
in every mode, holding the item spine and the per-collection checkpoints that make
enumeration incremental, and it SHALL be required even where no body is kept: a
body-less IMAP to IMAP copy still needs to know what it has already copied and
where each source's enumeration resumed.

`retain` SHALL declare whether the store additionally holds bodies and is readable
by a frontend, independently of which endpoints the account names. With no
targets, the store is the destination, so `retain` SHALL be true and an explicit
`retain = false` SHALL be refused as syncing to nowhere. With targets declared it
SHALL default to false, a configuration naming sources and targets having asked to
copy between them rather than to fill a disk, and `retain = true` alongside targets
SHALL be honoured rather than refused: migrating while keeping a local copy is a
thing to want.

Whether a crossing is streamed from its holding source to the target or staged in
the store and released SHALL be an internal choice, taken where the pairing
allows, and SHALL NOT be visible in the configuration or in any report. A store
that holds only the bodies that happened to cross SHALL NOT be a state a user can
be in.

#### Scenario: A DAV pair keeps no more than an IMAP pair does
- GIVEN two CardDAV endpoints in a one-way account with `retain = false`
- WHEN the account is synced
- THEN the crossing is staged and released, and the store holds no body afterwards

### Requirement: A mode that would discard data is refused, not warned
The store's state file SHALL record the mode triple (arity, `one-way`, `retain`)
when the store is created, and every run SHALL compare its configuration against
it.

A run whose `one-way` moved from false to true SHALL be refused. The previous mode
preserved changes on the side the new one discards, so the first run after the
edit is the one that loses them. The refusal SHALL name a one-time acknowledgement
(`sync --accept-mode`) that records the new mode, and SHALL NOT name `init` or
`--reset`, which drop the store and are a heavier remedy than the situation calls
for.

A `retain` that moved from true to false, and a change of arity that does not turn
`one-way` on, SHALL be reported and SHALL NOT block: bodies already stored stay,
unreferenced, until an explicit `pimdir gc`.

The comparison SHALL gate on those transitions and not on configuration change in
general. A rotated credential, a new filter or an added source in a no-targets
account threatens nothing, and forcing a resync for one would cost a mailbox.

A first run with `one-way = true` against a non-empty target has no recorded mode
to compare against and SHALL NOT be gated. `init` SHALL instead state the
account's behaviour in words.

#### Scenario: Turning on one-way stops the run that would discard
- GIVEN a two-way account synced at least once
- WHEN `one-way = true` is added and the account is synced
- THEN the run is refused before any write, naming the acknowledgement that records the change

#### Scenario: A rotated password is not a mode change
- GIVEN an account whose secret command changed
- WHEN it is synced
- THEN the run proceeds, the recorded mode being unchanged

### Requirement: A backend under the account is a source named after its protocol
A backend table written directly under the account (`imap`, `carddav`, `caldav`,
`jmap`, `gmail`, `msgraph`) SHALL be sugar for `sources.<protocol>.<protocol>`,
the source taking the protocol as its name. The sugar SHALL produce a
configuration indistinguishable from the expanded form, source id included, so
expanding it by hand is a no-op on the store.

Declaring the same protocol both directly and under `sources` SHALL be a
configuration error rather than a merge.

#### Scenario: Expanding the sugar changes nothing
- GIVEN an account written as `imap.server = "..."`
- WHEN it is rewritten as `sources.imap.imap.server = "..."`
- THEN the sync opens the same source id and reuses every existing binding

### Requirement: A collection is keyed by kind, namespace and name
A hub collection SHALL be keyed by the triple `(kind, namespace, name)`: the
source's media type, its namespace, and the collection name the backend
enumerates. The namespace SHALL be internal, derived from the source name, and
SHALL NOT be configurable: it exists so that a CardDAV address book and a mailbox
carrying the same name key apart, not so that a user can decide which sources
meet. A target binds the namespace of the source it is paired with, which is what
makes the two meet.

The id is spelled `<namespace>/<name>` with the kind on the collection row, and
the namespace prefix SHALL be stripped back off before any call reaches a server,
at one seam, so a backend only ever sees the name it gave. A report SHALL name a
collection the way its server does, not the way the store keys it.

Every wire call SHALL pass through that seam, including the ones a hydration pool
makes on its own connections rather than through the remote, and including a
collection named as an argument rather than as the target: a move destination is
a hub id like the collection it leaves. A cache keyed by collection SHALL keep
the hub id as its key, the seam being the wire call and not the plan.

The source's `collection` table SHALL keep `create` and `delete` optional,
defaulting to granting, unlike the `item` table which requires its pair to be
declared in full. The table also carries `filter`, so it is declared for reasons
that have nothing to do with permissions, and demanding a permission pair from
someone writing a filter would be a trap.
### Requirement: One account is one hub and one database
An account SHALL be exactly one pimdir store: one hub, one database, one blob
directory. `sync` SHALL take one account, so the database it opens is never
ambiguous, and SHALL accept `--source <name>` to narrow which sources run inside
that same database.

Sources in one account never meet, whatever command invoked them: an item one
holds crosses to the account's targets, never to another source. Two genuinely
independent replicas SHALL be two accounts.

A source whose sync fails SHALL be reported and SHALL NOT stop the others: they
share nothing but the file the store lives in.

### Requirement: N sources over one store
Every configured source SHALL be one source handle of the account's pimdir
store, keyed by its name. Cross-source propagation of items, flags and deletions
falls out of each source's project/absorb against the shared hub, with no
hand-rolled cross-merge and no special case for the two-source shape.

### Requirement: A source retains every body when the store is a replica
Where `retain` is true the sync SHALL hydrate every synced item to `Full`, the
store being the app's offline copy. It SHALL pull before pushing so an edit the
app staged locally stays pending and is reported rather than swallowed, and it
SHALL open the store as the source it syncs so an app writing under the same id
stages edits the sync pushes.

The item a hydration pass picks up SHALL be selected by the absence of a stored
body, not by its detail level. A remote content change drops the stale body while
the hub keeps the level the item had reached, so a pass keyed on the level would
leave an edited item bodiless for good.

### Requirement: Sync narrows by source
`sync --source <name>` SHALL narrow a run to the named sources. Narrowing no
longer picks namespaces, there being none: an account is one mode, and a source is
addressable on its own.

### Requirement: A send channel belongs to at most one source
At most one source per account SHALL declare `smtp`. Two or more SHALL be a
configuration error, reported at load, rather than a silent tiebreak on source
order. A source that sends by itself (Microsoft Graph, through `sendMail`) needs
none. The account root MAY carry the `smtp` table when its mail backend is the
direct-backend sugar, in which case it completes that one source; with no direct
mail backend, or several, it SHALL be refused.

### Requirement: The send channel authenticates like the sync side
A source's `smtp` table SHALL be spelled as its `imap` one: a `server` that is
either a bare authority, read as `smtps://<authority>`, or a full `smtp://` or
`smtps://` URL; the same `tls` block and `starttls` switch; an optional `alpn`
list; and an optional `sasl` table naming exactly one mechanism out of
ANONYMOUS, LOGIN, PLAIN, OAUTHBEARER, XOAUTH2 and SCRAM-SHA-256.

The mechanism SHALL resolve through the same conversion the IMAP side uses, the
GS2 host and port coming from the resolved submission URL. An omitted `sasl`
table SHALL open an unauthenticated session, stopping after `EHLO` and sending
no `AUTH`. The retired flat `login` and `password` keys SHALL be refused by
name, never ignored: a dropped credential would authenticate nothing against a
server that requires it.

A build declaring the `smtp` feature SHALL enable io-smtp's `scram` feature, so
a configured SCRAM-SHA-256 reaches the wire instead of being reported as an
unsupported mechanism.

### Requirement: The wizard configures one channel from the account's credentials
The wizard SHALL offer to back the send channel with the IMAP `sasl` table
whatever mechanism it names, a token mechanism included. Declining SHALL ask
whether the submission server authenticates at all before offering a mechanism
menu, so a relay taking no `AUTH` stays reachable. Credentials prompted for a
service SHALL be keyed under that service, so an account's IMAP and SMTP secrets
do not collide.

The SMTP menu SHALL be keyed on the capabilities discovery advertised rather
than on a live probe: io-imap reads `CAPABILITY` into mechanism values and
io-smtp offers no equivalent reader for the `AUTH` line.

### Requirement: A collection filter belongs to the source
`collection.filter` SHALL be declared per source rather than per account, because
an account may hold sources of several kinds and a mailbox include list means
nothing to a contacts source. An account-level `collection` table SHALL be
refused, naming its replacement.

Filters are consequently asymmetric: a collection may be synced on one source and
skipped on another, which the documentation SHALL state, since the previous
account-level filter guaranteed symmetry. A source and its target SHALL apply the
source's filter to both, since they bind one set of hub collections and filtering
them apart would read as a delete on the next pass.

### Requirement: Bodies are content-addressed and deduped
An item body SHALL be stored once per content hash; an item present on both
sources or in several collections is stored once and copied by reference. The
link id SHALL be the identity pimdir SPEC Annex A gives, with nothing prepended:
the bare `Message-ID` for mail, the bare `UID` for a card. Only a kind's own
fallback is marked (`alt:` over subject, date and sender; `hash:` over a card's
body), that being the one case a prefix is for, a name no server has heard of; a
real id cannot be mistaken for one, RFC 5322 `atext` admitting no colon before
the `@`.

Where a kind resolves its link id at more than one tier — `message/rfc822`, from
the IMAP ENVELOPE at `Meta` and from the parsed body at `Full` — the two
derivations MUST produce the byte-identical string for the same item. In
particular the date component SHALL be formatted the one canonical way, the
**UTC instant** in RFC 3339 at seconds precision, so a message with no
`Message-ID` does not link one way at `Meta` and another at `Full`. Kinds
resolving at a single tier (the DAV kinds) cannot hit this class of bug.

### Requirement: Bodies are named by the store's own hash
The content hash naming an object SHALL come from the store handle
(`PimdirStore::blobs`), which computes the algorithm `store_meta.hash_algo`
records (pimdir SPEC §5), never from a digest neverest defines. A consumer
picking its own names bodies where no other reader of the same store looks, and
it fails silently, as a dedup that never dedups.

### Requirement: Every item carries a per-kind sort key
The sync SHALL write a `sort_key` beside the `meta` of every item it summarises
(pimdir SPEC §9.3), derived by the same per-kind seam and never parsed back out
of the summary by the store. `message/rfc822` SHALL carry the `Date:` header
normalised to RFC 3339 in UTC at seconds precision, so byte order is
chronological order whatever offset the sender wrote; `text/vcard` SHALL carry
the display name (`FN`) casefolded and trimmed. A kind resolving at two tiers
SHALL derive the byte-identical key at both, on the same terms as its link id: a
key that moved when the body arrived would re-sort the item on hydration.
Content carrying nothing to derive from SHALL keep the empty key, which the
store reads as unknown.

### Requirement: A probed item is raised to the tier its kind resolves at
Every freshly probed placement SHALL be raised to the tier its kind resolves its
link id and summary at: `Meta` where the backend offers a cheap server-side
summary (mail's IMAP `ENVELOPE`), `Full` where only the body carries the
identity. Raising a DAV item to `Meta` asks its backend for a summary tier it
does not have, which fails the scan of every DAV collection.

### Requirement: Sources are remote backends only
A sync source SHALL be a remote backend: IMAP and Microsoft Graph for
`message/rfc822`, CardDAV for `text/vcard`, CalDAV for `text/calendar` (JMAP and
Gmail as their backends land). Local file backends (m2dir, maildir, vdir) SHALL
NOT be sync sources: the pimdir store is the local replica, so a local file store
is redundant as a source and belongs on the import/export path, which neverest
documents rather than syncing directly.

### Requirement: The wizard discovers in parallel and proposes what it found
The discovery fan-out already resolves CalDAV and CardDAV services alongside
IMAP and submission, and the wizard SHALL offer every reachable service whose
backend is compiled into the running build, not only the mail ones. A run that
finds services of several kinds SHALL offer them as separate entries, one per
kind.

The wizard SHALL write **one account with one source**, the offline replica,
which is the common case and the only one worth automating. Everything beyond
it, a second kind, a mirror, a fan-in, is configured by hand against
config.sample.toml. The picked service is written through the direct-backend
sugar (`imap.server = …`), and an account with no target retains every body and
reads offline with no further setting. The wizard SHALL NOT write `one-way`,
`retain` or a `targets` table: their defaults are the offline replica it exists
to produce.

All other wizard rules (the single email-address prompt, the derived account
name, the fan-out deadline, the capability-narrowed credential prompts, the
connection test before writing, the save confirmations) are unchanged.

### Requirement: A bare invocation offers the wizard only on a first run
Running `neverest` with no subcommand SHALL print the help, except on a machine
with no configuration, where it SHALL offer the wizard first, as a bare
`himalaya` does. The wizard targets the first `--config` path when given, else
the default one. A configuration that fails to parse counts as present, so the
offer never proposes to write over a broken file; the parse error surfaces when
a real command reads it. A declined offer SHALL fall back to the help, a bare
invocation having nothing else to run.

The offer SHALL be skipped, and the help printed, in JSON mode and when stdin
is not a terminal, neither being able to answer a prompt. It SHALL also be
skipped when `--account` names an account: with no subcommand that is a
half-typed command rather than a first run, and the help is what points at the
commands.

The wizard SHALL NOT write a configuration file unconditionally: it SHALL ask
for confirmation before saving, SHALL ask again before overwriting an existing
file, and SHALL print the generated TOML document on stdout when either
confirmation is declined, so a generated configuration is never lost. In JSON
mode or when stdout is not a terminal, the wizard SHALL emit the document on
stdout without the save prompts, so `neverest > config.toml` and scripted runs
keep working.

A command that finds no configuration file SHALL propose the wizard ("No
configuration found, create one at `<path>`?"), under the same two guards, and
SHALL then read the configuration again rather than trust the wizard's result,
which is only printed when the save is declined. A command still finding none
SHALL fail naming the path it looked at and the documented sample; it SHALL NOT
exit reporting success.

### Requirement: The generated configuration is a dotted document
A configuration neverest writes or prints SHALL render as Himalaya's does: one
`[accounts.<name>]` table header per account, the only headers in the document,
with every field below it written as a dotted key. An empty table SHALL write
nothing. The saved file and the document printed on stdout SHALL be identical.

The document SHALL hold only what was actually decided: every field equal to
its default SHALL be omitted (the account `default` flag when false, the
per-source collection / flag / item permissions, the per-source pool size, the
collection filter, the HTTP-backend ALPN list, `starttls`). Omitting
a field SHALL be lossless: every skipped field keeps a deserialization default
equal to the value that was skipped.

### Requirement: Every remote backend is a cargo feature
Each remote SHALL be gated by a cargo feature: `imap` for the IMAP backend,
`msgraph` for the Microsoft Graph backend, `smtp` for the SMTP submission
channel. All three SHALL ship in the default feature set. A missing backend
SHALL surface at runtime, never at build time: every feature combination
compiles, the configuration surface stays whole (every source config still
parses), and an unavailable backend fails when the source is *opened*, as the
JMAP and Gmail sources already do. A build with neither `smtp` nor `msgraph` has
no send channel and SHALL warn rather than perform a submit intent. Each
optional backend crate SHALL take its TLS provider from neverest's own
`native-tls` / `rustls-aws` / `rustls-ring` / `vendored` features rather than
pinning one.

### Requirement: A backend owns its ALPN default
The `alpn` field of a source or channel config that has a backend crate SHALL be
optional, and unset SHALL mean that crate's own default (io-imap's `["imap"]`,
io-smtp's `["smtp"]`), resolved where the connection is opened. An explicit `[]`
SHALL skip ALPN. Neverest SHALL NOT restate a backend's default, in the config
schema or in the values the wizard writes, so the default lives in exactly one
place.

### Requirement: The pimdir store is the sole local copy
A message body SHALL be held locally exactly once — content-addressed in the pimdir
blob store (where the derivation keeps bodies), deduped across sources and
collections, and Neverest SHALL keep no parallel local copy in another format.
Sync sources are remote backends only;
an existing on-disk store (maildir/m2dir) is brought in through io-pimdir's
conversion tooling, not synced as a source. The store lives per account as
`pimdir.db` plus an `objects/` blob directory.

### Requirement: The collection kind is declared
Each synced collection's media type SHALL be declared on the store from the
backend (`Client::media_type`; `message/rfc822` for the mail backends), so the
store is self-describing and ready to carry other item kinds.

### Requirement: A collection records the account that syncs it
Every store handle SHALL be opened for the account being synced, so each
collection it writes is grouped under that account (pimdir SPEC §9.2). Within
the account, a collection is further keyed by its source's namespace, which is
its name, so two sources of one kind are told apart without inferring it from
the collection naming. Two hand-written accounts may still share one `store.root`, and a reader
of such a store SHALL be able to tell whose collection is whose.

### Requirement: A store the format outgrew is refused with its remedy
A store an earlier draft of the pimdir format wrote cannot be migrated in place.
Opening one SHALL fail naming `sync --reset` for the account, the command that
drops the replica and resyncs it, rather than surfacing the raw refusal: the
store is a derived cache, so recreating it costs a resync and loses nothing but
un-pushed local mutation.

### Requirement: The mail summary is a versioned schema
The `meta` written for a `message/rfc822` item SHALL be `v: 1` JSON — `v`
(required), `subject` (required), and optional `message_id`, `in_reply_to`,
`from`, `to`, `date` and `size` (octets), with absent optionals omitted — so a
reader can render an envelope list without fetching a body. `date` SHALL be the
UTC instant in RFC 3339, never the local reading the sender wrote, which is what
lets two writers of one store compare and order items without re-parsing. Flags
are not in `meta`. Both the enumerate (`Meta`) and the streamed (`Full`) paths
SHALL emit this schema, the streamed path carrying the message's known octet
length as `size` rather than the header prefix it read. The schema is
`PimdirMailMeta`, documented in `pimdir/SPEC.md` Annex A.

`in_reply_to` SHALL be a list of bare msg-ids, the `In-Reply-To:` grammar being
`1*msg-id`, each normalised like `message_id` so a reply and its parent compare
byte-for-byte. It SHALL be read from the response the enumeration already makes:
the 9th `ENVELOPE` element on IMAP (RFC 3501 §7.4.2) and the parsed header at the
streamed tier. Microsoft Graph SHALL leave it empty, `In-Reply-To` living in
`internetMessageHeaders`, which a listing selection does not return.

### Requirement: The report shows remote-originated changes a pull applied
A sync SHALL report the remote-originated changes a pull applied to already-synced
items — flag changes and removals — not only the local→remote pushes and the
download plan. Because the pull applies them silently (the item reads `Clean`
afterwards), they SHALL be recovered from the sync's per-item `events`: a
`FlagsChanged` diffed against a pre-pull `handle → flags` snapshot into precise
add/remove flag hunks, and a `Vanished` into a delete hunk. A newly-pulled message
(`Added`) is already reported by the pull plan (a `Fetch` hunk) and is not
re-itemized.

### Requirement: The report shows the one-source pull plan
A one-source sync SHALL report its pull plan, each non-tombstone item whose body
it would download into the store, as `Fetch` hunks, in both a dry run (which
stops there) and a real run (which then hydrates them). A dry run SHALL fetch no
body to produce that report.

The plan SHALL select an item by the absence of a stored object, never by its
detail level. The two differ twice over, and both cases are real: a kind that
resolves its identity only at `Full` (a DAV one, a `sync-collection` REPORT
carrying no `UID`) lands its probe on the level a level-keyed plan reads as
complete, and a remote content change drops the stale object while the hub keeps
the level the item had reached, so an item about to be re-fetched reads as
complete too. Selecting on the object matches the hydration pass the plan is a
preview of, so the two cannot disagree.

Raising a fresh probe to the tier its kind resolves at SHALL be skipped in a dry
run where that tier is `Full`. A cheaper tier MAY still resolve, so a mail
preview keeps naming messages by their `Message-ID`; an item left probed SHALL be
named by whatever handle it has. A preview that downloads an entire address book
to print a plan is not a preview.

#### Scenario: A first dry run over a DAV account names its items
- GIVEN an initialized account with a CardDAV source and cards on the server
- WHEN `sync --dry-run` runs before any real sync
- THEN the report names each card, rather than reporting the account already in sync

#### Scenario: A dry run after a server-side edit names the re-fetch
- GIVEN a synced card edited on the server
- WHEN `sync --dry-run` runs
- THEN the report names the card whose body would be re-fetched

### Requirement: Meta and size fetches are targeted
The `Meta` fetch (link id + summary) and the largest-first size probe SHALL
fetch **only the handles being processed** (a `UID FETCH <handle-set>`), never
the whole mailbox: the `Meta` fetch as `(UID FLAGS ENVELOPE RFC822.SIZE)`, the
size probe as size-only `(UID RFC822.SIZE)` (no ENVELOPE). So an incremental
sync's silent pre-download work scales with the number of changed messages, not
the mailbox size — no whole-mailbox ENVELOPE sweep runs to resolve a handful of
link ids or to order a download. This mirrors the lean, targeted `enumerate`
(QRESYNC delta); a first sync stays inherently heavy (every new message's link
id is fetched once), but the redundant second sweep is gone.

### Requirement: Sync is one-shot
A `sync` run SHALL perform a bounded number of reconcile passes until quiescent
and exit. Watch and real-time triggers are out of scope: watching belongs to
carillon (carillon-core and its frontends), whose content-free ring kicks a sync
run through its cmd consumer.

### Requirement: Neverest is the store's sole owner and drains the queue first
Neverest SHALL be the only process writing a pimdir store; frontends read it and
enqueue mutations through io-pimdir's producer queue. At the start of every sync
run, before any network work, each collection with pending queue work SHALL be
drained (`drain_collection`: exactly-once apply-and-delete per action,
permanently bad actions parked, transient failures left queued in order). The
applied counts SHALL be logged (info when nonzero) and reported, and every
parked action SHALL surface in the run report until repaired. The subsequent
sync of a drained collection pushes the resulting dirty state. An action kind
the drain cannot apply itself (a capability-bound intent such as `submit`)
SHALL be left pending for the phase that can, never parked.

### Requirement: A run holds the store lock, waiting bounded
A sync run SHALL hold an advisory `sync.lock` in the **actual** store directory
(honouring `store.root`) for the whole run. A second run SHALL wait for the
holder up to a bounded timeout (60 s) and then exit with a clear error, so cron
ticks and connector-triggered scoped runs serialize instead of failing or
corrupting.

### Requirement: An IMAP handle-space change rebuilds the collection and bumps its generation
For an IMAP source, the driver SHALL compare the stored checkpoint's UIDVALIDITY
before and after the pull; on a change it SHALL drive io-replica's rekey
(carrying cached bodies, summaries and pending state over by link id) and route
the rebuild write batch through `write_rekeyed`, so `collections.generation`
bumps atomically with the rebuild and a frontend derives its epoch (an IMAP
UIDVALIDITY) from the store alone. Ordinary syncs and full resyncs never bump.
Graph sources never rebuild: Graph message ids survive a delta reset (an expired
delta link restarts a full round without changing identity).

### Requirement: Microsoft Graph is a first-class source
An `msgraph` source SHALL open protocol-direct over io-msgraph (never through a
frozen aggregator): folders listed two levels deep (`Parent/Child` naming),
enumeration through the messages delta query carrying the `@odata.deltaLink`
as the engine's opaque checkpoint (HTTP 410 = expired link, restarting a fresh
full round; any other failure surfaces), the `Meta` tier served from the cached
delta rows (`mid:`/`alt:` link ids, meta v1), the `Full` tier from the raw MIME
content streamed into the blob store. Flags map to the IANA wire spellings
(`isRead` = `\Seen`, a flagged follow-up = `\Flagged`, `isDraft` = `\Draft`).
Auth SHALL be a bearer access token only, resolved through the standard
secret-command idiom (`auth.token.raw` / `auth.token.command`) once per opened
client; neverest SHALL NOT run any OAuth flow itself (no device sign-in, no
client credentials, no token persistence): acquiring and refreshing the token
is delegated to an external command, typically ortie. No token is ever logged.
Push scope is honest: flag changes push through `message_update` and deletes
through `message_delete`; appends, moves and content updates are rejected
(pull-only) and documented.

### Requirement: A queued submission is a `submit` queue intent
Neverest SHALL NOT reserve a collection for queued sends. Submission is a
**mail** capability: a `submit` intent belongs to a `message/rfc822` account,
and an `smtp` channel declared on a source of any other kind SHALL be
refused before any connection is made, rather than silently ignored. A submission SHALL be
a **queue action** whose kind (`submit`) is defined by neverest, not by pimdir:
the format carries an action kind and a versioned JSON payload, and which kinds
an owner can perform is the owner's business. An owner that does not recognise a
kind, or recognises it but lacks the capability, SHALL **skip** the row, leaving
it pending, never parking it (parking means permanently unappliable) and never
blocking later actions of that collection.

The intent's payload SHALL be `v: 1` JSON carrying `object` (the body hash, by
the convention every action kind follows), `from` (empty means the null reverse
path), `rcpts` and an optional `subject`. The body SHALL be written durably
before the enqueue, so the queue row pins it (queued bodies are pinned, and no
GC can sweep it between the enqueue and the send); it belongs to no collection.
The intent SHALL anchor on whatever collection the producer chose: neverest
scans every collection's pending actions, so there is no anchor rule.

Neverest SHALL perform each pending intent through the one source offering a
send channel: its own `smtp` table, else its native send
(the Graph `sendMail` action, which files the message in Sent itself), sources
walked in the order the account declares them. At most one source may
declare an `smtp` table, so the only pick that order decides is between a
declared channel and a natively-sending source. On success the row SHALL
be acknowledged, releasing the body's pin. A **transient** failure (an SMTP 4xx,
a transport error) SHALL leave the row pending; a **permanent** one (an SMTP
5xx, an undecodable payload, a missing body) SHALL park it with its error. A
build with no send channel (neither `smtp` nor `msgraph`) SHALL skip submit
intents and warn, never park them. Message content is never logged.

Submission is **at-least-once**: a crash between the server's acceptance and the
acknowledgement resends on the next run, so deduplication is the receiving
provider's job (`Message-ID`).

### Requirement: A run reclaims retained items on a schedule
The store retains an item rather than deleting it when its last binding
vanishes, so reclaiming is the client's schedule. An account SHALL configure it
as `store.purge-after`, a human duration (one integer plus `s`/`m`/`h`/`d`/`w`).
**Unset SHALL mean never purge**; `"0"` SHALL purge immediately, reproducing a
terminal delete. There SHALL be no boolean beside it: the delay is the switch.

A sync run SHALL sweep **after** the sync and before the report is finalised, on
both the pair and the solo paths, never in a dry run, purging every
retained item whose `retained_at` precedes `now - purge-after` (RFC 3339, the
shape the store stamps). Sweeping after the sync means an item this run retired
starts its delay now rather than being reclaimed by the run that retired it. The
sweep SHALL warn rather than fail the run, as the send channel does, and `sync
--no-purge` SHALL skip it. The report SHALL carry what was reclaimed (items and
bytes) in both the text and `--json` output.

A read-only source (`<protocol>.item.delete = false`,
`collection.delete = false`) with no purge delay is therefore a backup: a remote
expunge retires the local row without losing the item or its body.

### Requirement: The checkpoint is opaque to the shared client seam
The backend-neutral enumeration seam SHALL carry the incremental-sync cursor as
opaque checkpoint bytes and string member handles: the IMAP adapter encodes its
`(UIDVALIDITY, HIGHESTMODSEQ)` pair, the Graph adapter its delta link, and the
engine stores whichever bytes the source produced. (Supersedes the IMAP-shaped
`(u32, u64)` cursor on the shared seam; the QRESYNC behaviour itself is
unchanged and stays specified under "IMAP enumeration is incremental".)

### Requirement: The spinner reports in-mailbox progress
While syncing a mailbox, the spinner SHALL report progress through the slow inner
phase — body hydration (and, under relay, the relayed messages) — as a percentage
appended to the mailbox line (`[2/7] Syncing INBOX 66%`), updated per streamed
`Full` body. Fast phases (enumerate, the `Meta` upgrade) stay silent. The progress
tick is invoked from the concurrent fetch pool and is safe to call from several
threads at once.

### Requirement: IMAP enumeration is incremental (QRESYNC)
Enumeration SHALL carry a per-mailbox cursor `(UIDVALIDITY, HIGHESTMODSEQ)` in the
`ReplicaCheckpoint`. On a QRESYNC-capable server (ENABLEd on connect) with a cursor
whose UIDVALIDITY still matches, `enumerate` SHALL issue a QRESYNC
`SELECT (QRESYNC (uidvalidity highestmodseq))` and return a **delta**
(`complete = false`): only the messages changed since the modseq plus the vanished
UIDs — issuing **no FETCH when nothing changed**. Without a usable cursor (first
sync, UIDVALIDITY change, malformed checkpoint) or on a non-QRESYNC server it SHALL
return a **full** `FETCH 1:* (UID FLAGS)` snapshot (`complete = true`). Enumeration
SHALL fetch UID and FLAGS only — never ENVELOPE — since the link id is resolved at
the `Meta` tier.

### Requirement: A connection SELECTs a mailbox once per run of commands
An IMAP connection SHALL cache the mailbox it currently has `SELECT`ed and skip a
redundant `SELECT` when the next command targets the same mailbox, so a run of
commands on one mailbox — most importantly a batch of body fetches — pays a single
`SELECT`, not one per command. Every select path SHALL record the selection so a
cached skip is always correct. For a hydrate of N bodies across a W-connection
pool this makes `SELECT`s ~W rather than ~N, halving the fetch path's round trips
over a high-latency link without changing what is fetched. (Pipelining the body
`FETCH`es themselves — mbsync-style — is a further io-imap change, not yet done.)

### Requirement: Bodies transfer with bounded memory
A body SHALL be fetched and appended by streaming — fetched straight into the
blob store and appended straight from it — so no full message is held in memory;
peak memory is bounded to a chunk regardless of message size, including in the
batched fetch (each message opens its own streaming sink; the socket-to-blob copy
uses a 128 KB buffer). The `Message-ID` link id and the summary SHALL be read from
the streamed header prefix, so no extra pass over the body is needed. (The local
m2dir backend is not yet chunked; the IMAP path is.)

### Requirement: The one-source sync runs as three account-wide phases
The one-source (retain) sync SHALL run as three phases across the whole account,
not a per-mailbox loop, so the connection pool never idles at a mailbox boundary:

- **Phase 1 — spine (parallel over mailboxes).** A work-stealing pool of workers,
  each on its own IMAP connection *and* its own store handle, reconciles each
  mailbox's spine (pull + meta + itemize + push) and collects its bodies to
  hydrate (`handle` + size from the local envelope meta). The network overlaps
  across mailboxes; store writes serialise on the store's single-writer lock (the
  seam sanctions process-level serialization), and every collection is pre-created
  serially first so no worker races lazy creation. Phase 1 is Meta-tier only (no
  objects/blobs), so concurrent handles touch only disjoint per-collection rows.
- **Phase 2 — hydrate (one global pool).** Every mailbox's bodies are chunked into
  largest-first per-mailbox batches, biggest batches queued first for a global
  largest-first order, and work-stolen across the connections through **one**
  queue: a worker finishing one mailbox's last batch immediately steals the next
  mailbox's (`select_cached` re-SELECTs across the boundary), so no connection
  idles at a mailbox edge. Bodies stream into the blob store; the fetched items are
  cached by `(collection, handle)`.
- **Phase 3 — apply (serial, no network).** Each mailbox's `Full` upgrade is driven
  over a cache-backed remote serving the Phase-2 bodies (a miss falls back to a
  real fetch); only index writes happen, single writer. Cross-mailbox blobs are
  safe: object rows exist only for applied mailboxes, so no GC deletes a
  not-yet-applied mailbox's blob.

A dry run stops after Phase 1 (reporting the pull plan, downloading nothing).
Progress is the three phases: `Scanning mailboxes (k/M)`, one global
`Downloading n% (done/total)` over every body, then `Writing (k/M)`.

### Requirement: Hydration may run concurrently, largest-first
Full-tier hydration SHALL fetch bodies in **batches** — one `UID FETCH <set>
(UID BODY.PEEK[])` streaming K bodies (`BATCH_SIZE`, default 64) in a single
response — so N bodies cost ~N/K round trips per connection rather than one round
trip per message. Each message is routed to its own streaming sink by the **UID
on its own FETCH line**, so an out-of-order server response still lands
correctly; a body line without a parseable UID SHALL fail the batch so the caller
falls back to per-message fetches rather than misroute. In the one-source sync,
hydration is a single account-wide phase (see the three phases above): bodies are
ordered **largest-first** globally using each item's size from the store meta (no
size probe), chunked into per-mailbox batches, biggest first, and work-stolen
across the pool over one queue with no per-mailbox barrier; the cross-source copy
path, lacking sizes, falls back to UID order. On any batch error the fetch SHALL
fall back to per-message fetches; content-addressing makes the partial retry
idempotent.
The pool is **persistent**: connections are opened up front and kept for the run,
so their auth is paid once, not per batch. The budget defaults to 4, is
configurable per account (`connections`) and overridable by a `sync --connections`
flag, and SHALL stay under the backend's per-account connection cap. Body bytes
stream lock-free into the blob store; the engine serialises the index write on
the single-writer store afterwards. The largest-first order takes its sizes from
the store meta, never a server size probe.

> Initial seed spec (Cairn adopted 2026-07-31): captures the sync driver's core
> guarantees; the CLI surface, mailbox diff, and report are further capabilities
> to spell out as they are touched.


### Requirement: A source's backend declares the kind it syncs
Every sync backend SHALL declare the IANA media type of the items it syncs, and
that type SHALL be recorded as the pimdir collection's `kind`. The kind SHALL be
derived from the source's backend, never declared in the configuration: one
backend per source, and no per-kind nesting.

The media type SHALL also be knowable from the protocol alone, without opening a
connection. A pair that disagrees
SHALL still fail with a clear error before any store write, since a mailbox
cannot reconcile against an address book. A store MAY hold collections of
several kinds, which is what pimdir is built for, and an account MAY now feed it
several.

### Requirement: Link id and meta are per-kind, resolved at one seam
The cross-collection link id and the `v:1` meta summary SHALL be produced by one
implementation per media type, selected from the source's declared kind at a single
dispatch point. `message/rfc822` keeps the `Message-ID` (`mid:`) identity with
its `(subject, date, sender)` (`alt:`) fallback. `text/vcard` and `text/calendar`
SHALL use the vCard / iCalendar `UID` (`uid:`), falling back to the content hash
(`hash:`) for a body carrying no `UID`; an iCalendar `RECURRENCE-ID` SHALL NOT
enter the link id, so a recurrence override stays the same item. Each kind's meta
schema SHALL follow the pimdir SPEC Annex A convention registered for it.

### Requirement: Mutable-content backends carry a revision and push updates
A backend whose item bodies change in place SHALL report a content revision (an
ETag) on every enumerate and fetch, and SHALL return the revision the server
assigned from every accepted write. `ReplicaChange::Update` SHALL be pushed as a
conditional write against the base revision. A write the server refuses because
the revision moved SHALL be reported as rejected, so the engine re-merges and
records the divergence as a conflict rather than overwriting the remote. Item
bodies on an immutable-content backend (mail) keep no revision, and an update
there is still rejected as impossible.

### Requirement: Conflicts are surfaced in the run report
A placement the engine marked `conflicted` SHALL appear in the sync report (text
and `--json`), naming its collection and item, and SHALL keep appearing on every
run until it is resolved. Neverest SHALL NOT resolve a content conflict by
itself; resolution is an edit, staged through the pimdir queue by whoever owns
the decision.

### Requirement: An ambiguous identity is reported, never judged
An identity the engine marked ambiguous (a collection holding two items with one
link id, two messages with the same `Message-ID`) SHALL appear in the sync
report, text and `--json`, naming its collection and every handle involved, and
SHALL keep appearing on every run until the collection holds the identity once.

Neverest SHALL NOT repair a duplicated collection, and SHALL NOT report it as an
invalid mailbox: RFC 5322 §3.6.4 binds the generator of a `Message-ID` and says
nothing about what a store may hold, so the report states what neverest cannot
tell apart rather than what the user did wrong. Detection, policy and state
belong to the engine and the store; this crate surfaces them and derives no
duplicate rule of its own.

### Requirement: The report accounts for every write the run made
A run that wrote to a remote SHALL report it. `already in sync` SHALL mean the
run wrote nothing, and an append performed by the sync SHALL appear as a hunk,
so a report can be read as the record of what happened rather than a summary
that may omit it. A relayed copy (streamed source to source, never reaching the
projection the report is otherwise built from) SHALL therefore be itemized where
it is performed.

### Requirement: A source may be denied item updates
The per-source permission set SHALL gate item updates (`item.update`, default
true) beside the existing collection and item create/delete and flag gates. An
update hunk a source's policy forbids SHALL be dropped from the patch and surfaced
in the report, so a mutable-content source can be made read-only.

### Requirement: DAV collections enumerate by sync token and resolve at Full
A CardDAV or CalDAV source SHALL enumerate through `REPORT sync-collection`,
storing the returned sync token verbatim as the collection's opaque checkpoint,
and SHALL fall back to a tokenless report (the whole member
set, reported complete) when the server rejects the stored token. Because that report returns hrefs and ETags but
no `UID`, a DAV placement SHALL resolve directly at the `Full` tier — there is no
`Meta` tier for DAV — so a DAV item's link id has exactly one derivation and
cannot differ between tiers. Bodies SHALL be fetched in batches through
`addressbook-multiget` / `calendar-multiget`.


### Requirement: A DAV server without `sync-collection` is enumerated by query
`sync-collection` is an extension, so a server MAY implement none of it,
advertising a `supported-report-set` of `addressbook-multiget` and
`addressbook-query` alone. Such a server SHALL be enumerated through
`addressbook-query` (RFC 6352 §8.6), which yields the same member ids and
revisions, rather than failing to enumerate at all.

The fallback SHALL be chosen on the RFC 3253 §3.6 `DAV:supported-report`
precondition in the error body, which is the server saying by name that it does
not run the report. It SHALL NOT be chosen on the HTTP status alone, the status
wrapping that precondition being the server's own choice, except for `405` and
`501` which mean the request was never going to run. A permission refusal, a
credential failure and a server fault SHALL all surface as failures, and a
rejected sync token SHALL keep its own recovery, a fresh full report.

The fallback carries no sync token, so its checkpoint SHALL be empty and an
empty checkpoint SHALL read as no cursor: such a collection is enumerated in
full on every run, which is what a server offering nothing incremental costs.

#### Scenario: An address book on a server without the report syncs
- GIVEN a CardDAV server whose `supported-report-set` holds no `sync-collection`
- WHEN the account is synced
- THEN the address book is enumerated by `addressbook-query` and its cards reach the store

#### Scenario: A permission refusal is not mistaken for a missing report
- GIVEN a server refusing the REPORT for lack of privileges
- WHEN the account is synced
- THEN the run reports the refusal rather than retrying it as a query
### Requirement: A DAV connection survives a server that closes it
A DAV source SHALL reopen its connection and run the exchange again when the
server closed it between requests (an HTTP/1.0 answer, a `Connection: close`),
carrying the discovery it already paid for over to the new connection. Only an
end-of-stream or reset failure SHALL be retried, being the shape of a request
the server never read, so a create or a delete is never replayed against a
server that acted on it.

### Requirement: A backend without flags reports them known-empty
A backend with no flag concept (CardDAV, CalDAV) SHALL report an item's flags as
*known-empty*, never as *unknown*. The distinction is normative (pimdir SPEC
Annex A): reporting unknown makes the engine treat the flag set as unfetched and
re-probe the item on every run.


### Requirement: A refused delete is held, never reverted
Every source SHALL sync under `ReplicaDeletePolicy::Keep`. Both refusals (`push`
off, or `item.delete = false`) run through that one disposition, and each source
here is bound to the store's hub, which fixes the answer: reverting a tombstone
states that the source still holds the member, and a hub reads that as the item
being alive (add-beats-delete across sources), so it clears the deletion for
every source and mirrors the item back to the one it was deleted on.

A source configured to take no deletes would then resurrect on both what the user
removed on one, which is the opposite of what that setting is for.

#### Scenario: A read-only source keeps the removal
- GIVEN a staged delete on a source whose `item.delete` is false
- WHEN the source is synced
- THEN nothing is pushed and the tombstone stays, rather than being undone into a clean row

### Requirement: A purge is followed by a collection
The store reclaims nothing by itself (pimdir SPEC §5: an object at refcount zero
is unreferenced, not deleted, because the batch that attaches a body may not be
the one that indexed it), so the retention sweep SHALL run the collector after a
purge that removed rows, and SHALL report the objects it dropped and the bytes it
freed beside the items the purge removed. A purge releases a body; it does not
reclaim one.

The collector SHALL NOT run after a sweep that took nothing: its cost is a walk
of the whole blob tree, and a purge that removed no row released nothing. Orphan
blobs a crash left are not this run's to find; they are what `pimdir gc` is for.

#### Scenario: The bytes a purge reports are bytes that left
- GIVEN a retained item past the purge cutoff, holding a body nothing else references
- WHEN the sweep runs
- THEN the item is purged, the object row is dropped, and the blob is gone from the tree

### Requirement: A submission greets with an address literal
An SMTP submission session SHALL greet with the loopback address literal
(`EHLO [127.0.0.1]`), the form RFC 5321 §4.1.3 reserves for a client with no
resolvable domain name of its own, which a desktop client behind a NAT never
has. It SHALL NOT greet with a bare `localhost`, which is not such a name
either: RFC 5321 §4.1.4 entitles a server to check, and one that does (Stalwart)
answers `550 5.5.0 Invalid EHLO domain`, failing the session before `MAIL FROM`
and leaving every intent pending behind a warning.

### Requirement: The conventions are the format's, the readers are not
A link id, a summary and a sort key SHALL be what pimdir SPEC Annex A and the
format's `vectors/meta.json` give, and the summary SHALL be
`io_pimdir::conventions`'s own type (`PimdirMailMeta`, `PimdirCardMeta`), so the
schema cannot drift from the format's by a field or a spelling. This crate SHALL
NOT define a summary struct of its own.

The **scanners** that read those fields off a body stay here while io-pimdir's
lose data these do not, and each gap SHALL be held by a test naming it:

- `conventions::mail` reads headers raw, so an RFC 2047 encoded-word subject
  reaches a reader as `=?utf-8?q?…?=`;
- `conventions::card` splits a property on the first colon, cutting the value of
  a legal quoted parameter that holds one (RFC 6350 §3.3), and leaves RFC 6350
  §3.4 escaping in place.

The format's vectors are ASCII-only and cover neither, so nothing upstream
reports the difference. When io-pimdir closes a gap, its `derive` SHALL replace
the scanner rather than be mirrored beside it.

#### Scenario: A non-ASCII subject reaches a reader readable
- GIVEN a message whose `Subject:` is RFC 2047 encoded
- WHEN either tier summarises it
- THEN `meta.subject` holds the decoded text, not the encoded-word

### Requirement: A `server` is an authority or a URL, resolved at one seam
Every backend's `server` SHALL accept either a bare authority, with or without a
port, or a full URL, and both SHALL resolve through one shared function rather
than per backend. The scheme a bare authority takes is the backend's own:
`imaps` for IMAP, `smtps` for SMTP, `https` for a DAV entry point.

The presence of `://` SHALL be what tells the two forms apart. A value carrying
it SHALL be parsed verbatim, so an explicit cleartext scheme or a non-default
port survives; a value without it SHALL take the default scheme. Resolution
SHALL NOT be decided by a parse error: a bare authority carrying a port parses
as a URL whose scheme is the hostname and whose path is the port, so it reports
no error and carries no host, and a backend handed one rejects it for a reason
that names neither the value the user wrote nor the field it came from.

### Requirement: A collection that failed to scan is reported, never only logged
A collection whose spine fails SHALL be recorded in the run's report, carrying
its error, and SHALL NOT be reported through the log alone. The other
collections SHALL still run: they share nothing but the file the store lives in.

A run that failed to scan a collection SHALL NOT report itself in sync. "In
sync" is a claim about what the sync compared, and a collection it could not
enumerate was never compared; a run that says it anyway hides a broken account
for as long as nobody reads the log.

An error crossing an engine boundary SHALL be rendered with its full chain, not
with its outermost context alone. A backend keeps a server's status and response
body so a caller can read them, and a wrapper that renders only the top drops
exactly the part naming what the server said.
