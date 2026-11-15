---
cairn: delta
change: the-report-shapes-read-as-the-family
---

# Delta

## ADDED Requirements

### Requirement: A conflict's id prints as a string
`conflict list`, `conflict show` and `conflict resolve` SHALL print the item's public id (the store's `seq`) under `id` as a string, as every Pimalaya binary prints a store id; `conflict show` and `conflict resolve` SHALL take it on the command line as `conflict list` prints it.

#### Scenario: A script settles the conflict it listed
- GIVEN a store holding one divergence on item 42
- WHEN `neverest conflict list --json` lists it
- THEN its row reads `id: "42"`, and `neverest conflict resolve "42" --prefer-local` settles it

### Requirement: A report lists its patches as arrays
A sync report SHALL list the hunks it applied under `collectionPatch` and `itemPatch`, each an array of entries `{hunk, code, error, sources}`, in the order the run applied them; the schema SHALL name their entries `CollectionHunkEntry` and `ItemHunkEntry`. The report SHALL NOT repeat `account`, which the account row around it names.

#### Scenario: A quiescent account
- GIVEN an account whose collections are in sync
- WHEN `neverest sync --json` reports it
- THEN its row reads `account` and its `report` reads `collectionPatch: []` and `itemPatch: []`, with no `account`, `collection` or `item` key

## MODIFIED Requirements

### Requirement: A resolution names what the push supports in part
`conflict resolve` SHALL stage its body in one step with the gate (`enqueue_with_body_checked`) and SHALL print the capabilities a source supports in part that the next push relies on, under `partial` in its JSON, one row each, `{capability, source, detail}`: `capability` as the store names it, `source` as the account's configuration names it, `detail` what the source leaves out or `null`. The text SHALL print a row as a line naming the source and the capability, then its detail.

### Requirement: A failing row carries its code and its causes
A report row naming what a run could not do to one item or one endpoint SHALL carry, beside its own keys, those of pimalaya-cli's item error but its id: `code`, `null` when the failure carries none; `error`, the outermost message; and `sources`, the messages of its causes, outermost first. These rows are the `collectionPatch` and `itemPatch` entries, `rejected`, `submitted`, `intents`, `parked` and `unreached` of a sync, and `parked` of a drain. A `rejected` row SHALL be pimalaya-cli's `ItemError` itself, its `id` the item's handle on the side that refused it, beside `side`, `collection` and `action`. A row that succeeded SHALL carry `code` and `error` null and `sources` empty. The text output SHALL print a row's message and its causes as one chain, as `{err:#}` reads.

A parked queue row SHALL carry the error the store recorded, with no code and no causes, the store keeping its text alone. An unconfirmed send's `error` SHALL begin `unconfirmed: `, its causes under `sources`, so its message and causes joined read as its parked row does.

An account's row of `sync`, `drain` and `init` and the index refresh SHALL carry `code`, `error` and `sources` the same way, `error` the outermost message, `null` and `[]` when it ran. Their `code` SHALL be read as a row's is, a server's refusal included; the index refresh's is the store's (`index-version`, `index-fts5-missing`). An account's row of `init` SHALL list the endpoints it opened under `opened`, sorted, those it opened before a failure included, empty for an account initialized already.

#### Scenario: A refused update under `--json`
- GIVEN a CardDAV source refusing an update 403
- WHEN `neverest --json sync` reports it
- THEN its `rejected` row reads `{side, collection, id, action: "update", code: "permission-denied", error: "Cannot update <id> in <collection>", sources: ["WebDAV server returned HTTP 403…"]}`, and the text prints `Cannot update <id> in <collection>: WebDAV server returned HTTP 403…`

#### Scenario: An account that failed
- GIVEN an account whose store cannot be opened
- WHEN `neverest --json sync` reports it
- THEN its row reads `error` the outermost message and `sources` its causes, and the text prints them as one chain

#### Scenario: An account `init` could not open
- GIVEN an account whose IMAP server refuses the connection
- WHEN `neverest --json init` reports it
- THEN its row reads `error: "Initialize imap"`, the refusal under `sources`, `opened: []`, and the run exits 4

## REMOVED Requirements

None.
