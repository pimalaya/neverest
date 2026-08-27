---
cairn: change
change: declared-sync-mode
---

# Delta

## ADDED Requirements

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

`sources` and `targets` SHALL both be named maps, each key a stable pimdir source
id its bindings are recorded under. A positional list SHALL NOT be used:
reordering it would reassign every binding, which is why `left` and `right` were
removed.

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
- GIVEN two CardDAV sources in a one-way account with `retain = false`
- WHEN the account is synced
- THEN the crossing is staged and released, and the store holds no body afterwards

### Requirement: A mode that would discard data is refused, not warned
The store's state file SHALL record the mode triple (arity, `one-way`, `retain`)
when the store is created, and every run SHALL compare its configuration against
it.

A run whose `one-way` moved from false to true SHALL be refused. The previous mode
preserved changes on the side the new one discards, so the first run after the
edit is the one that loses them. The refusal SHALL name a one-time acknowledgement
that records the new mode, and SHALL NOT name `init` or `--reset`, which drop the
store and are a heavier remedy than the situation calls for.

A `retain` that moved from true to false, and a change of arity that does not turn
`one-way` on, SHALL be reported and SHALL NOT block: bodies already stored stay,
unreferenced, until an explicit `pimdir gc`.

The comparison SHALL gate on those transitions and not on configuration change in
general. A rotated credential, a new filter or an added source in a no-targets
account threatens nothing, and forcing a resync for one would cost a mailbox.

A first run with `one-way = true` against a non-empty target has no recorded mode
to compare against and SHALL NOT be gated. `init` SHALL instead state the
account's behaviour in words, and where `one-way` is set SHALL count what the
target holds that the source does not.

#### Scenario: Turning on one-way stops the run that would discard
- GIVEN a two-way account synced at least once
- WHEN `one-way = true` is added and the account is synced
- THEN the run is refused before any write, naming the acknowledgement that records the change

#### Scenario: A rotated password is not a mode change
- GIVEN an account whose secret command changed
- WHEN it is synced
- THEN the run proceeds, the recorded mode being unchanged

## MODIFIED Requirements

### Requirement: An account holds named sources
An account SHALL hold a map of named sources (`sources.<name>`), each declaring
exactly one remote backend, and SHALL require at least one. It MAY additionally
hold a map of named targets (`targets.<name>`) on the same terms. Both map keys
SHALL be pimdir source ids, so a name is what every binding it owns in the store
is recorded against, and a rename SHALL be treated as a new source.

An account SHALL NOT constrain its sources to one kind: mail, contacts and
calendar sources may sit under one account, and their collections never meet (see
the collection key requirement).

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
that pair only ever describes a store an older neverest wrote. `init` SHALL stamp
the store it creates and `sync --reset` SHALL stamp the store it recreates.

#### Scenario: A two-side config is refused with its replacement
- GIVEN a configuration written with `left` and `right`
- WHEN it is loaded
- THEN it is refused, naming `sources`, `targets` and `one-way`

### Requirement: A collection is keyed by kind, namespace and name
A hub collection SHALL be keyed by the triple `(kind, namespace, name)`: the
source's media type, its namespace, and the collection name the backend
enumerates. The namespace SHALL be internal, derived from the source name, and
SHALL NOT be configurable: it exists so that a CardDAV address book and a mailbox
carrying the same name key apart, not so that a user can decide which sources
meet.

The id is spelled `<namespace>/<name>` with the kind on the collection row, and
the namespace prefix SHALL be stripped back off before any call reaches a server,
at one seam, so a backend only ever sees the name it gave. A report SHALL name a
collection the way its server does, not the way the store keys it.

Every wire call SHALL pass through that seam, including the ones a hydration pool
makes on its own connections, and including a collection named as an argument
rather than as the target: a move destination is a hub id like the collection it
leaves. A cache keyed by collection SHALL keep the hub id as its key, the seam
being the wire call and not the plan.

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

## REMOVED Requirements

### Requirement: Sources sharing a namespace share bindings
Removed. Which endpoints meet is now the arity of `sources` and `targets`, and
which way is `one-way`. A namespace decided the first by coincidence and could not
express the second.

### Requirement: What the store keeps is derived, never configured
Removed. What the store keeps is declared by `retain`. The three-state derivation
is replaced by a boolean, and its middle state, a store holding only the bodies
that happened to cross, is not reachable.

### Requirement: A derived change never drops what is stored
Removed as stated, and replaced by the mode-change requirement above. A transition
that discards data is refused rather than made quietly non-destructive, a declared
mode being something a user can be asked about where a derived one was not.

### Requirement: Every run reports what the store keeps
Removed. What the store keeps is readable from the configuration, so a run that
wrote nothing reports nothing. `check` states the mode in plain language, and the
persisted mode serves the refusal above rather than a per-run line.
