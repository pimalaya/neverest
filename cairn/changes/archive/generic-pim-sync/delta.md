---
cairn: change
change: generic-pim-sync
---

# Delta

## ADDED Requirements

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
schema SHALL follow the pimdir SPEC §13 convention registered for it.

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

### Requirement: A side may be denied item updates
The per-side permission set SHALL gate item updates (`item.update`, default
true) beside the existing collection and item create/delete and flag gates. An
update hunk a side's policy forbids SHALL be dropped from the patch and surfaced
in the report, so a mutable-content side can be made read-only.

### Requirement: DAV collections enumerate by sync token and resolve at Full
A CardDAV or CalDAV side SHALL enumerate through `REPORT sync-collection`,
storing the returned sync token verbatim as the collection's opaque checkpoint,
and SHALL fall back to a full `PROPFIND` snapshot (reported complete) when the
server rejects the stored token. Because that report returns hrefs and ETags but
no `UID`, a DAV placement SHALL resolve directly at the `Full` tier — there is no
`Meta` tier for DAV — so a DAV item's link id has exactly one derivation and
cannot differ between tiers. Bodies SHALL be fetched in batches through
`addressbook-multiget` / `calendar-multiget`.

### Requirement: A backend without flags reports them known-empty
A backend with no flag concept (CardDAV, CalDAV) SHALL report an item's flags as
*known-empty*, never as *unknown*. The distinction is normative (pimdir SPEC
§11): reporting unknown makes the engine treat the flag set as unfetched and
re-probe the item on every run.

## MODIFIED Requirements

### Requirement: Sides are remote backends only
A sync side SHALL be a remote backend: IMAP and Microsoft Graph for
`message/rfc822`, CardDAV for `text/vcard`, CalDAV for `text/calendar` (JMAP and
Gmail as their backends land). Local file backends (m2dir, maildir, vdir) SHALL
NOT be sync sides — the pimdir store is the local replica, so a local file store
is redundant as a side and belongs on the import/export path, which neverest
documents rather than syncing directly. The wizard SHALL produce one-side
(local-sync) remote accounts only: it writes `left` plus the implicit store and
never a `right`, so a remote-to-remote mirror is always configured by hand.

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

### Requirement: The wizard discovers in parallel and proposes what it found
Unchanged in shape, extended in what it proposes: the discovery fan-out already
resolves CalDAV and CardDAV services alongside IMAP and submission, and the
wizard SHALL offer every reachable service whose backend is compiled into the
running build, not only the mail ones. A run that finds services of several
kinds SHALL offer one account per kind, sharing one store directory. All other
wizard rules (the single email-address prompt, the derived account name, the
fan-out deadline, the capability-narrowed credential prompts, the connection
test before writing, the save confirmations) are unchanged.

### Requirement: The Outbox and its send channel are mail-only
The account's local-only `Outbox` collection and the per-side send channel
(SMTP submission, or a backend that sends natively) SHALL exist only for a
`message/rfc822` account. An `smtp` table configured on a side of any other kind
SHALL be refused at config load rather than silently ignored.

## REMOVED Requirements

None. Every mail behaviour is retained; the mail-only ones become explicitly
kind-gated rather than universal.
