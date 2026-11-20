---
cairn: delta
change: the-contract-audit-rulings
---

# Delta

Folded into cairn/spec/sync.md.

## ADDED Requirements

### Requirement: A run's failures are one list
Replaces *A failing row carries its code and its causes*: every failure of a sync report is one row of `errors`, pimalaya-cli's item error tagged `kind` (`collection-patch`, `item-patch`, `failures`, `rejected`, `duplicates`, `refusals`, `submitted`, `intents`, `unreached`) with its own keys; the lists reporting what happened or a state stay; exit 3 for a row waiting for a person, 4 otherwise.

### Requirement: An account's TLS reaches every endpoint field by field
An account `tls` table, himalaya's keys, inherited field by field by every endpoint's and `smtp`'s `tls`, the client certificate as one unit.

### Requirement: An unknown conflict and an uninitialized account are coded
`conflict-not-found`, `account-uninitialized`, and `item-duplicate` on a refused duplicate.

## MODIFIED Requirements

### Requirement: A first run meets the family's welcome
A bare `neverest --json` is a refused command line through pimalaya-cli's `meet_bare_invocation`, exit 2.

### Requirement: The command line is the family's
`completion`, `manual` and `json-schema` require their names or `--dir`, clap refusing a run with neither, exit 2.

### Requirement: The account group is the family's
`account check` prints pimalaya-cli's account check, rows under `backends` with `tls`, neverest's keys after the shared ones, `mode` per account, `orphans` at the top; its text one block of fields per account.

### Requirement: A drain applies the queue without syncing
Every parked row of the account's collections is reported, exit 3; a malformed row fails the store's accounts coded, never the command.

### Requirement: Text mode prints plain lines
Progress lines reach a pipe for a full `sync` alone; `account check`, `init`, `--queue-only` and `--index-only` print them on a terminal alone.

### Requirement: A signal stops a sync cleanly
The shutdown is installed before `--init` and the lock wait, a signal there printing the result.

### Requirement: A check opens the send channel too
The channel's row carries `tls`.

### Requirement: A check reports every endpoint
Rows under `backends`, the shared keys first, `tls` filled from the endpoint's table, `null` over a cleartext URL.

### Requirement: Deciding is a command, never a run
An aborted merger prints `aborted` and exits 3.

### Requirement: A conflict body is printed as stored
`{contents, encoding}`, `utf-8` or `base64`, one shape whatever the bytes; the handle as `remoteId`.

### Requirement: A conflict's id prints as a string
The handle prints as `remoteId`; a sync report's conflict row prints the same `id` and `collection` as `conflict list`; an unknown id is coded `conflict-not-found`.

### Requirement: A report lists its patches as arrays
The patches list the hunks applied, a failed one being an error.

### Requirement: A queue row is keyed `row`
A parked row is pimalaya-cli's queue row, `handle`, `linkId`, `itemId` and `unconfirmed` added, `code` and `sources` dropped.

### Requirement: An unreached endpoint is named in the report
An `unreached` error naming `endpoint`, beside the `collection-patch` error of its `*` scan.

### Requirement: A refused write is reported, never counted as applied
A `rejected` error.

### Requirement: A change refused as it stands is held and reported
A `refusals` error, the reason as its message.

### Requirement: A failed pair collection is reported
A `collection-patch` error of a `sync` hunk.

### Requirement: A store step that fails is reported
A `failures` error.

### Requirement: A quota refusal keeps the send queued
A `submitted` error.

### Requirement: A server's refusal carries its code
The scenarios read the `rejected` and `submitted` errors.

### Requirement: Every key is printed
The quiet run's scenario lists `conflicts`, `parked` and `errors`.

### Requirement: A data command describes what it prints
A bare `neverest` under `--json` fails `usage-invalid`.

## REMOVED Requirements

### Requirement: A failing row carries its code and its causes
Replaced by *A run's failures are one list*.
