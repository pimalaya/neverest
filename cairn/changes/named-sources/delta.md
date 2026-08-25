---
cairn: change
change: named-sources
---

# Delta

## ADDED Requirements

### Requirement: An account holds named sources
An account SHALL hold a map of named sources (`sources.<name>`), each declaring exactly one remote backend, and SHALL require at least one. The map key SHALL be the pimdir source id, so a source's name is what every binding it owns in the store is recorded against. Renaming a source orphans its bindings, so a rename SHALL be treated as a new source.

An account SHALL NOT constrain its sources to one kind: mail, contacts and calendar sources may sit under one account, and their collections never meet (see the collection key requirement).

`left` and `right` SHALL NOT survive in any form, as keys, as aliases, or as source ids. v1 is unreleased, so they are not a shipped surface, and an alias would exist only to inject a shared collection namespace implicitly, which is exactly the behaviour this change makes explicit. A configuration carrying them SHALL be refused at load, naming `sources` and the namespace needed to keep a mirror mirroring.

A store written before this change SHALL NOT be read. Collection ids gain the kind and the namespace, and no compatibility mapping is provided: nothing is deployed on the previous shape, so the upgrade path is a fresh store.

#### Scenario: Mail and contacts under one account
- GIVEN an account declaring an IMAP source and a CardDAV source
- WHEN it is synced
- THEN both run against the same store, and neither is refused for disagreeing on its kind

#### Scenario: A two-side config is refused with its replacement
- GIVEN a configuration written with `left` and `right`
- WHEN it is loaded
- THEN it is refused, naming `sources.left` / `sources.right` and the shared `collection.namespace` that preserves the mirror

### Requirement: A backend under the account is a source named after its protocol
A backend table written directly under the account (`imap`, `carddav`, `caldav`, `jmap`, `gmail`, `msgraph`) SHALL be sugar for `sources.<protocol>.<protocol>`, the source taking the protocol as its name. The sugar SHALL produce a configuration indistinguishable from the expanded form, source id included, so expanding it by hand is a no-op on the store.

Declaring the same protocol both directly and under `sources` SHALL be a configuration error rather than a merge.

#### Scenario: Expanding the sugar changes nothing
- GIVEN an account written as `imap.server = "..."`
- WHEN it is rewritten as `sources.imap.imap.server = "..."`
- THEN the sync opens the same source id and reuses every existing binding

### Requirement: A collection is keyed by kind, namespace and name
A hub collection SHALL be keyed by the triple `(kind, namespace, name)`: the source's media type, the source's `collection.namespace`, and the collection name the backend enumerates. The bare collection name SHALL NOT be the id, because a CardDAV address book and a mailbox may carry the same name in one store.

`collection.namespace` SHALL default to the source's own name, so sources are isolated unless configured otherwise. A namespace SHALL NOT be shared by two kinds: the two would key onto the same collection ids, which is the collision the triple exists to prevent.

The source's `collection` table SHALL keep `create` and `delete` optional, defaulting to granting, unlike the `item` table which requires its pair to be declared in full. The table now also carries `namespace` and `filter`, so it is declared for reasons that have nothing to do with permissions, and demanding a permission pair from someone writing a namespace would be a trap.

### Requirement: Sources sharing a namespace share bindings
Two sources of the same kind SHALL share a hub collection when, and only when, they declare the same `collection.namespace`. Propagation is not a mode and SHALL NOT be configured as one: it is the engine filling a binding gap, an item present in a collection a source participates in with no binding for that source.

Sources sharing a namespace therefore mirror each other, subject to their own permissions. Sources isolated by the default namespace sit side by side in one store and never push to one another, which is the merged read view a frontend unions at display time.

A local create SHALL attribute itself through the same mechanism, with no owner field: created in an isolated source's collection it lands on that source alone; created in a shared collection it goes to every source in that namespace whose permissions allow it.

Isolated is the default because its failure mode is a mirror that did nothing, visible in the report, where merged-by-default fails by copying one real provider's mailbox into another.

#### Scenario: Isolated sources do not push to one another
- GIVEN two IMAP sources in one account, both with an `INBOX`, both on their default namespace
- WHEN the account is synced
- THEN the store holds two `INBOX` collections and neither source is written to on behalf of the other

#### Scenario: A shared namespace propagates a delete
- GIVEN two IMAP sources sharing a namespace, an item bound to both
- WHEN the item is deleted on one source and the account is synced
- THEN the delete is pushed to the other source, subject to its `item.delete` permission

### Requirement: One account is one hub and one database
An account SHALL be exactly one pimdir store: one hub, one database, one blob directory. `sync` SHALL take one account, so the database it opens is never ambiguous, and SHALL accept `--source <name>` to narrow which sources run inside that same database.

Two sources of one kind in one account are merged or isolated by their namespace, never by which command invoked them. Two genuinely independent replicas SHALL be two accounts.

### Requirement: N sources over one store
Every configured source SHALL be one source handle of the account's pimdir store, keyed by its name. Cross-source propagation of items, flags and deletions falls out of each source's project/absorb against the shared hub, with no hand-rolled cross-merge and no special case for the two-source shape.

### Requirement: What the store keeps is derived, never configured
`store.retention` and `store.hydration` SHALL leave the configuration surface. What the store keeps SHALL be derived per kind from the namespace's source count and pairing:

- one source in the namespace: **every** body, the store being that source's offline replica;
- exactly two sources sharing a namespace on a pairing that can stream (mail, IMAP to IMAP): **no** body, each crossing streamed from its holding source to the target through a bounded in-memory pipe, the store keeping only the spine, with the target's APPEND length taken from the item's `v:1` meta `size` so no body is buffered to discover it;
- anything else, every DAV pairing included: **the bodies that crossed**.

A namespace of three or more sources SHALL be refused, naming the namespace, its sources and the two ways out (give one of them a namespace of its own, or split it into another account). The hub reconciles any number of sources, but the paths that move a body between them are written for a pair, and refusing is honest where quietly syncing three sources pairwise would not be. The derivation above is therefore only ever asked about one or two.

A configuration still carrying `store.retention` or `store.hydration` SHALL be refused at load, naming the derived value that replaces it for that kind. Accepting and ignoring `retention = "retain"` on a pairing that derives "keep nothing" would hand the user the opposite of what they wrote, so a removed key is refused rather than tolerated, on the same terms as `left` and `right`.

Deriving removes two expressible configurations, a mirror that also keeps a local offline copy and a single source kept as an envelope-only index. Both are given up deliberately: an override may be added later without breaking a configuration, where removing one could not.

#### Scenario: An unstreamable pairing is not silently substituted
- GIVEN two CardDAV sources sharing a namespace
- WHEN the account is synced
- THEN the derived value is "bodies that crossed", and no configuration was honoured or overridden to get there

### Requirement: A derived change never drops what is stored
The derived value SHALL govern only what a sync fetches and keeps from that run on. It SHALL NOT retroactively remove bodies already in the store. A kind flipping from keeping every body to keeping none, which is what adding a second source to a namespace does, SHALL leave every stored object in place, unreferenced, to be reclaimed only by an explicit `pimdir gc` or `sync --reset`.

A one-shot tool cannot prompt, so the transition is made non-destructive rather than confirmed.

#### Scenario: Adding a mirror target does not erase the offline copy
- GIVEN an account with one IMAP source and a fully hydrated store
- WHEN a second IMAP source is added to the same namespace and the account is synced
- THEN the sync stops fetching new bodies for that kind, every already-stored body is still on disk, and the report names them as unreferenced with the command that reclaims them

### Requirement: Every run reports what the store keeps
A sync SHALL report, per kind and namespace, the number of sources it holds and what the store keeps for them, in text and `--json`, whether or not the run wrote anything. Where bodies are kept it SHALL report the objects and bytes held, so a store expected to *be* a backup is seen to be one on the first run rather than on the day it is needed.

A run whose derived value differs from the previous run's SHALL say so explicitly, naming the old value, the new value, the configuration change that caused it, and what became unreferenced.

`check` SHALL report the same derivation, and SHALL derive it without contacting a server, so it answers before a first sync has ever run and while a remote is down.

#### Scenario: A relaying account says it keeps nothing
- GIVEN two IMAP sources sharing a namespace
- WHEN the account is synced, and again when `check` runs
- THEN both state that the store keeps no bodies for `message/rfc822`

### Requirement: A source alone in its namespace retains every body
A source alone in its namespace SHALL hydrate every synced item to `Full`, because the store is the app's offline copy. It SHALL pull before pushing so an edit the app staged locally stays pending and is reported rather than swallowed, and it SHALL open the store as that source so an app writing under the same id stages edits the sync pushes.

The item a hydration pass picks up SHALL be selected by the absence of a stored body, not by its detail level. A remote content change drops the stale body while the hub keeps the level the item had reached, so a pass keyed on the level would leave an edited item bodiless for good.

### Requirement: Sources are remote backends only
A source SHALL be a remote backend: IMAP and Microsoft Graph for `message/rfc822`, CardDAV for `text/vcard`, CalDAV for `text/calendar` (JMAP and Gmail as their backends land). Local file backends (m2dir, maildir, vdir) SHALL NOT be sources: the pimdir store is the local replica, so a local file store is redundant and belongs on the import/export path.

### Requirement: A send channel belongs to at most one source
At most one source per account SHALL declare `smtp`. Two or more SHALL be a configuration error, reported at load, rather than a silent tiebreak on source order. A source that sends by itself (Microsoft Graph, through `sendMail`) needs none.

### Requirement: A collection filter belongs to the source
`collection.filter` SHALL be declared per source rather than per account, because an account may hold sources of several kinds and a mailbox include list means nothing to a contacts source. Filters are consequently asymmetric: a collection may be synced on one source and skipped on another, which the documentation SHALL state, since the previous account-level filter guaranteed symmetry.

## MODIFIED Requirements

### Requirement: The wizard discovers in parallel and proposes what it found
Unchanged in scope: the wizard SHALL keep writing **one account with one source**, the offline replica, which is the common case and the only one worth automating. Everything beyond it, a second kind, a mirror, a fan-in, is configured by hand against config.sample.toml.

Only the spelling changes: the picked service is written through the direct-backend sugar (`imap.server = …`) rather than under `left`. A single source shares its namespace with nobody, so the account it writes keeps every body and reads offline with no further setting.

The wizard SHALL NOT write a collection namespace at all. A namespace only matters when two sources of one kind meet, and the wizard never writes two.

All other wizard rules (the single email-address prompt, the derived account name, the fan-out deadline, the capability-narrowed credential prompts, the connection test before writing, the save confirmations) are unchanged.

### Requirement: A collection records the account that syncs it
Every store handle SHALL be opened for the account being synced, so each collection it writes is grouped under that account (pimdir SPEC §9.2). Within the account, a collection is further keyed by its source's namespace, so two sources of one kind are told apart without inferring it from the collection naming. Two hand-written accounts may still share one `store.root`.

## REMOVED Requirements

### Requirement: Side count selects the sync mode
Replaced by "An account holds named sources" and "One account is one hub and one database". Side count was a two-source heuristic and says nothing about an account holding several kinds, or several sources of one kind.

### Requirement: Two sources over one store
Replaced by "N sources over one store".

### Requirement: A two-source sync may relay instead of retain
Replaced by "What the store keeps is derived, never configured". The mode survives as a derived behaviour; the `store.retention` and `store.hydration` settings do not survive at all.

### Requirement: A local sync retains every body
Replaced by "A source alone in its namespace retains every body".

### Requirement: Sides are remote backends only
Replaced by "Sources are remote backends only". Its wizard clause, which required one-side accounts only, is superseded by the modified wizard requirement.
