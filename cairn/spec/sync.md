---
cairn: spec
capability: sync
status: current
---

# Sync

`neverest sync` reconciles two sides (`left` / `right`) of an account through the
io-replica engine over a pimdir store. It is sync-on-demand: one reconcile per
invocation, no daemon.

### Requirement: Side count selects the sync mode
An account SHALL configure one or two sides (`left`/`right`, each optional; at
least one required). **One** configured side is a *local sync*: that remote is
reconciled against the retained pimdir store, which is the local replica an app
reads and edits. **Two** configured sides is the remote-to-remote sync through the
store. The store is otherwise implicit (per-account state dir) and customised only
by an account-root `store` config (`root` override), never as a side.

### Requirement: Two sources over one store
When two sides are configured, they SHALL be two source handles (`"left"` /
`"right"`) of one pimdir store, the mailbox name as the bare collection id. The
shared database is the cross-side hub; cross-side propagation of messages, flags
and deletions falls out of the hub's project/absorb, with no hand-rolled
cross-merge.

### Requirement: A two-source sync may relay instead of retain
Relay is a **mail** mode. A two-side sync SHALL support a `store.retention`
mode. Under `Retain` it keeps every body in the store. Under `Relay` a
cross-copy body SHALL be streamed directly from its holding side to the other
through a bounded in-memory pipe — the store keeping only the spine (the item is
never hydrated, no object blob at rest; the target's next enumerate binds the
relayed message). The target APPEND length comes from the item's `v:1` meta
`size`, so no body is buffered to discover it. Relay is the **default for an
IMAP↔IMAP pairing** and unavailable otherwise (any other pairing, including
every DAV pairing, retains; an explicit `relay` there falls back to retain).
Relay trades away dedup / cheap retry / resumability, so it is the pass-through
mirror; retain stays the default wherever a local reader exists.

### Requirement: A local sync retains every body
A one-side sync SHALL hydrate every synced item to `Full` (fetch its body into the
store), because the store is the app's offline copy — distinct from the two-source
path, which hydrates only bodies about to cross. It SHALL pull before pushing so an
edit the app staged locally stays pending and is reported (and pushed) rather than
swallowed, and it SHALL open the store as the one side's source so an app writing
as that same source stages edits the sync pushes.

The item a hydration pass picks up SHALL be selected by the **absence of a
stored body**, not by its detail level. A remote content change drops the stale
body while the hub keeps the level the item had reached, so a pass keyed on the
level would leave an edited item bodiless for good.

### Requirement: Bodies are content-addressed and deduped
An item body SHALL be stored once per content hash; an item present on both
sides or in several collections is stored once and copied by reference. The link
id is per-kind (see the link-id requirement above). Where a kind resolves its
link id at more than one tier — `message/rfc822`, from the IMAP ENVELOPE at
`Meta` and from the parsed body at `Full` — the two derivations MUST produce the
byte-identical string for the same item. In particular the mail date component
SHALL be formatted the one canonical way (`to_rfc3339` with UTC written as `Z`,
an offset as `+hh:mm`, seconds precision), so a message with no `Message-ID`
does not link one way at `Meta` and another at `Full`. Kinds resolving at a
single tier (the DAV kinds) cannot hit this class of bug.

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

### Requirement: Sides are remote backends only
A sync side SHALL be a remote backend: IMAP and Microsoft Graph for
`message/rfc822`, CardDAV for `text/vcard`, CalDAV for `text/calendar` (JMAP and
Gmail as their backends land). Local file backends (m2dir, maildir, vdir) SHALL
NOT be sync sides — the pimdir store is the local replica, so a local file store
is redundant as a side and belongs on the import/export path, which neverest
documents rather than syncing directly. The wizard SHALL produce one-side
(local-sync) remote accounts only: it writes `left` plus the implicit store and
never a `right`, so a remote-to-remote mirror is always configured by hand.

### Requirement: The wizard discovers in parallel and proposes what it found
Unchanged in shape, extended in what it proposes: the discovery fan-out already
resolves CalDAV and CardDAV services alongside IMAP and submission, and the
wizard SHALL offer every reachable service whose backend is compiled into the
running build, not only the mail ones. A run that finds services of several
kinds SHALL offer them as separate entries, one per kind, and the picked one
writes an account of that kind; pairing two kinds against one `store.root` is a
hand-written setup. All other
wizard rules (the single email-address prompt, the derived account name, the
fan-out deadline, the capability-narrowed credential prompts, the connection
test before writing, the save confirmations) are unchanged.

### Requirement: A bare invocation runs the wizard
Running `neverest` with no subcommand SHALL run the configuration wizard
against the target configuration path (the first `--config` path when given,
else the default one), as a bare `himalaya` does. The command list SHALL stay
reachable through `--help`.

The wizard SHALL NOT write a configuration file unconditionally: it SHALL ask
for confirmation before saving, SHALL ask again before overwriting an existing
file, and SHALL print the generated TOML document on stdout when either
confirmation is declined, so a generated configuration is never lost. In JSON
mode or when stdout is not a terminal, the wizard SHALL emit the document on
stdout without the save prompts, so `neverest > config.toml` and scripted runs
keep working.

A command that finds no configuration file SHALL propose the wizard ("No
configuration found, create one at `<path>`?") and SHALL exit when the proposal
is declined; the confirmation belongs to that proposal, not to the wizard, so a
bare invocation never asks it.

### Requirement: The generated configuration is a dotted document
A configuration neverest writes or prints SHALL render as Himalaya's does: one
`[accounts.<name>]` table header per account, the only headers in the document,
with every field below it written as a dotted key. An empty table SHALL write
nothing. The saved file and the document printed on stdout SHALL be identical.

The document SHALL hold only what was actually decided: every field equal to
its default SHALL be omitted (the account `default` flag when false, the
per-side collection / flag / item permissions, the per-side pool size, the
collection filter, the HTTP-backend ALPN list, `starttls`). Omitting
a field SHALL be lossless: every skipped field keeps a deserialization default
equal to the value that was skipped.

### Requirement: Every remote backend is a cargo feature
Each remote SHALL be gated by a cargo feature: `imap` for the IMAP backend,
`msgraph` for the Microsoft Graph backend, `smtp` for the SMTP submission
channel. All three SHALL ship in the default feature set. A missing backend
SHALL surface at runtime, never at build time: every feature combination
compiles, the configuration surface stays whole (every side config still
parses), and an unavailable backend fails when the side is *opened*, as the
JMAP and Gmail sides already do. A build with neither `smtp` nor `msgraph` has
no send channel and SHALL warn rather than perform a submit intent. Each
optional backend crate SHALL take its TLS provider from neverest's own
`native-tls` / `rustls-aws` / `rustls-ring` / `vendored` features rather than
pinning one.

### Requirement: A backend owns its ALPN default
The `alpn` field of a side or channel config that has a backend crate SHALL be
optional, and unset SHALL mean that crate's own default (io-imap's `["imap"]`,
io-smtp's `["smtp"]`), resolved where the connection is opened. An explicit `[]`
SHALL skip ALPN. Neverest SHALL NOT restate a backend's default, in the config
schema or in the values the wizard writes, so the default lives in exactly one
place.

### Requirement: The pimdir store is the sole local copy
A message body SHALL be held locally exactly once — content-addressed in the pimdir
blob store (under retain), deduped across sides and mailboxes — and Neverest SHALL
keep no parallel local copy in another format. Sync sides are remote backends only;
an existing on-disk store (maildir/m2dir) is brought in through io-pimdir's
conversion tooling, not synced as a side. The store lives per account as
`pimdir.db` plus an `objects/` blob directory.

### Requirement: The collection kind is declared
Each synced collection's media type SHALL be declared on the store from the
backend (`Client::media_type`; `message/rfc822` for the mail backends), so the
store is self-describing and ready to carry other item kinds.

### Requirement: A collection records the account that syncs it
Every store handle SHALL be opened for the account being synced, so each
collection it writes is grouped under that account (pimdir SPEC §9.2). Two
hand-written accounts may share one `store.root`, and a reader of such a store
SHALL be able to tell whose collection is whose without inferring it from the
collection naming.

### Requirement: A store the format outgrew is refused with its remedy
A store an earlier draft of the pimdir format wrote cannot be migrated in place.
Opening one SHALL fail naming `sync --reset` for the account, the command that
drops the replica and resyncs it, rather than surfacing the raw refusal: the
store is a derived cache, so recreating it costs a resync and loses nothing but
un-pushed local mutation.

### Requirement: The mail summary is a versioned schema
The `meta` written for a `message/rfc822` item SHALL be `v: 1` JSON — `v`
(required), `subject` (required), and optional `message_id`, `in_reply_to`,
`from`, `to`, `date` (RFC 3339) and `size` (octets), with absent optionals
omitted — so a reader can render an envelope list without fetching a body. Flags
are not in `meta`. Both the enumerate (`Meta`) and the streamed (`Full`) paths
SHALL emit this schema, the streamed path carrying the message's known octet
length as `size`. The schema is documented in `pimdir/SPEC.md` Annex A.

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
A one-source sync SHALL report its pull plan — each not-yet-`Full`, non-tombstone
item it would download into the store — as `Fetch` hunks, in both a dry run (which
stops there) and a real run (which then hydrates them). So `sync --dry-run` shows
what a fresh sync would download (rather than "already in sync"), and a real run's
report reflects the download, its main work.

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
For an IMAP side, the driver SHALL compare the stored checkpoint's UIDVALIDITY
before and after the pull; on a change it SHALL drive io-replica's rekey
(carrying cached bodies, summaries and pending state over by link id) and route
the rebuild write batch through `write_rekeyed`, so `collections.generation`
bumps atomically with the rebuild and a frontend derives its epoch (an IMAP
UIDVALIDITY) from the store alone. Ordinary syncs and full resyncs never bump.
Graph sides never rebuild: Graph message ids survive a delta reset (an expired
delta link restarts a full round without changing identity).

### Requirement: A two-source sync may mirror every body
`store.hydration = "full"` SHALL make a two-source retain sync hydrate every
non-tombstone placement to `Full` on both sides (bodies mirrored in the store),
reusing the body dedup so a shared body is fetched once. The default stays
per-mode: a one-source sync always retains every body, a two-source sync
hydrates only bodies about to cross (`"crossing"`). Full hydration forces
retain; combined with an explicit `relay` it warns and retains.

### Requirement: Microsoft Graph is a first-class side
An `msgraph` side SHALL open protocol-direct over io-msgraph (never through a
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
and an `<side>.smtp` channel declared on a side of any other kind SHALL be
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

Neverest SHALL perform each pending intent through the first side offering a
send channel: its own `<side>.smtp` table (`server` `smtps://host:465` or
`smtp://host:587` + `starttls`, optional login/password), else its native send
(the Graph `sendMail` action, which files the message in Sent itself), sides
walked in configuration order (`left`, then `right`). On success the row SHALL
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
both the two-side and the single-source paths, never in a dry run, purging every
retained item whose `retained_at` precedes `now - purge-after` (RFC 3339, the
shape the store stamps). Sweeping after the sync means an item this run retired
starts its delay now rather than being reclaimed by the run that retired it. The
sweep SHALL warn rather than fail the run, as the send channel does, and `sync
--no-purge` SHALL skip it. The report SHALL carry what was reclaimed (items and
bytes) in both the text and `--json` output.

A read-only remote side (`<side>.<backend>.item.delete = false`,
`collection.delete = false`) with no purge delay is therefore a backup: a remote
expunge retires the local row without losing the item or its body.

### Requirement: A side pairs one backend with its send channel
A side SHALL be a table naming exactly one backend (`<side>.imap`,
`<side>.jmap`, `<side>.gmail`, `<side>.msgraph`) and, optionally, the
`<side>.smtp` channel completing it. A backend key that matches no backend
SHALL be refused. The account root SHALL carry no `smtp` table: a configuration
keeping one SHALL fail to parse rather than silently stop sending.

### Requirement: The checkpoint is opaque to the shared client seam
The backend-neutral enumeration seam SHALL carry the incremental-sync cursor as
opaque checkpoint bytes and string member handles: the IMAP adapter encodes its
`(UIDVALIDITY, HIGHESTMODSEQ)` pair, the Graph adapter its delta link, and the
engine stores whichever bytes the side produced. (Supersedes the IMAP-shaped
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
across the pool over one queue with no per-mailbox barrier; the cross-side copy
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


### Requirement: A side's backend declares the kind it syncs
Every sync backend SHALL declare the IANA media type of the items it syncs, and
that type SHALL be recorded as the pimdir collection's `kind`. The kind SHALL be
derived from the side's backend, never declared in the configuration: the
account schema stays `left` / `right`, one backend per side, and gains no
per-kind nesting.

An account's two sides SHALL therefore be checked **at runtime**, not at config
load: when the sides open and report their media types, a pair that disagrees
SHALL fail with a clear error before any store write, since a mailbox cannot
reconcile against an address book. A store MAY hold collections of several
kinds, which is what pimdir is built for.

### Requirement: Link id and meta are per-kind, resolved at one seam
The cross-collection link id and the `v:1` meta summary SHALL be produced by one
implementation per media type, selected from the side's declared kind at a single
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
that may omit it. A relayed copy (streamed side to side, never reaching the
projection the report is otherwise built from) SHALL therefore be itemized where
it is performed.

### Requirement: A side may be denied item updates
The per-side permission set SHALL gate item updates (`item.update`, default
true) beside the existing collection and item create/delete and flag gates. An
update hunk a side's policy forbids SHALL be dropped from the patch and surfaced
in the report, so a mutable-content side can be made read-only.

### Requirement: DAV collections enumerate by sync token and resolve at Full
A CardDAV or CalDAV side SHALL enumerate through `REPORT sync-collection`,
storing the returned sync token verbatim as the collection's opaque checkpoint,
and SHALL fall back to a tokenless report (the whole member
set, reported complete) when the server rejects the stored token. Because that report returns hrefs and ETags but
no `UID`, a DAV placement SHALL resolve directly at the `Full` tier — there is no
`Meta` tier for DAV — so a DAV item's link id has exactly one derivation and
cannot differ between tiers. Bodies SHALL be fetched in batches through
`addressbook-multiget` / `calendar-multiget`.

### Requirement: A DAV connection survives a server that closes it
A DAV side SHALL reopen its connection and run the exchange again when the
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

