---
cairn: delta
change: a-refusal-carries-its-code
---

# Delta

## ADDED Requirements

### Requirement: A server's refusal carries its code
A failure a server refused SHALL carry a code of the vocabulary Himalaya shares, read from the typed error of the protocol that answered it, wherever a row or an account reports it:

| Code | When |
|---|---|
| `permission-denied` | an HTTP 403 that is no rate limit (Graph, Gmail, Google Calendar, Google People); a DAV 403 whose body names no precondition, or `need-privileges` (RFC 3744 §7.1.1); an IMAP completion naming `NOPERM` (RFC 5530 §3) |
| `quota-exceeded` | an HTTP 507; a DAV body naming `quota-not-exceeded` (RFC 4331 §6); Graph's `ErrorQuotaExceeded`; an IMAP completion naming `OVERQUOTA` (RFC 5530 §3); an SMTP 5yz reply naming a sending limit |
| `rate-limited` | an HTTP 429, or a Google 403 naming a rate limit or a quota, still answered after the waits neverest takes |
| `recipient-refused` | an SMTP `RCPT TO` refused (RFC 5321 §3.3), io-smtp offering none after the first; a sending limit is `quota-exceeded` instead |

A refusal none of these name SHALL carry no code, its reason kept in the row's `sources`. A DAV 403 naming another precondition is a body the server would not take (RFC 4791 §5.3.2, RFC 6352 §6.3.2), no matter of rights, and SHALL carry none; nor SHALL a 5xx.

The codes are read once a row is built, from the chain the client returned, so the typed errors the run reads back (a duplicate `UID`, an item gone) stay in it.

#### Scenario: A full mailbox
- GIVEN an IMAP server answering an `APPEND` with `NO [OVERQUOTA]`
- WHEN a run reports the refused append
- THEN its `rejected` row's `code` is `quota-exceeded`

#### Scenario: A refused recipient
- GIVEN an SMTP server refusing the recipient of a queued send with `550 5.1.1`
- WHEN `sync --intents-only` runs
- THEN the send parks, its `submitted` row coded `recipient-refused` with the reply among its `sources`, and the run exits 3

#### Scenario: A read-only address book
- GIVEN a CardDAV server answering an update 403 with no precondition
- WHEN a run reports the refused update
- THEN its `rejected` row's `code` is `permission-denied`, where a 403 naming `valid-address-data` carries none

### Requirement: A failing row carries its code and its causes
A report row naming what a run could not do to one item or one endpoint SHALL carry, beside its own keys, those of pimalaya-cli's item error but its id: `code`, `null` when the failure carries none; `error`, the outermost message; and `sources`, the messages of its causes, outermost first. These rows are the collection and item patch entries, `rejected`, `submitted`, `intents`, `parked` and `unreached` of a sync, and `parked` of a drain. A row that succeeded SHALL carry `code` and `error` null and `sources` empty. The text output SHALL print a row's message and its causes as one chain, as `{err:#}` reads.

A parked queue row SHALL carry the error the store recorded, with no code and no causes, the store keeping its text alone. An unconfirmed send's `error` SHALL begin `unconfirmed: `, its causes under `sources`, so its message and causes joined read as its parked row does.

An account's row (`sync`, `drain`, `init`) and the index refresh SHALL keep the shape of a check row, `code` beside an `error` holding the whole chain: `init` names the endpoints it opened `sources`. Their `code` SHALL be read as a row's is, a server's refusal included; the index refresh's is the store's (`index-version`, `index-fts5-missing`).

#### Scenario: A refused update under `--json`
- GIVEN a CardDAV source refusing an update 403
- WHEN `neverest --json sync` reports it
- THEN its `rejected` row reads `{side, collection, id, action: "update", code: "permission-denied", error: "Cannot update <id> in <collection>", sources: ["WebDAV server returned HTTP 403…"]}`, and the text prints `Cannot update <id> in <collection>: WebDAV server returned HTTP 403…`

## MODIFIED Requirements

### Requirement: A refused write is reported, never counted as applied
A write a remote would not take SHALL be reported, naming the source, the collection, the item, what was attempted and why it failed, coded where the server's refusal says so ("A server's refusal carries its code"), in the text and `--json` reports alike. It SHALL NOT be counted among the hunks the run applied: the hunk the run derived for that item SHALL be taken back, the item patch being the plan and not the outcome. It SHALL keep appearing on every run until the write lands or the reason is removed.

A body that never reached the wire, because the blob tree no longer holds it, SHALL be reported the same way and for the same reason: the change is still in the store and the next run will try again.

A create refused with the no-uid-conflict precondition SHALL keep its own entry and gain no second one, since that entry names the identity and the remedy, and one write is one line.

#### Scenario: A server refuses a body
- GIVEN a source that answers an update with a refusal
- WHEN the run reports
- THEN the refusal is named with its error, its causes and its code, the update is not among the hunks applied, and a run with no other work does not read as having written anything

### Requirement: An unreached endpoint is named in the report
A run SHALL list in its report, under `unreached`, every endpoint whose connections it could not open, with its failure as a failing row carries it ("A failing row carries its code and its causes"): a server it could not reach, or one refusing the connection or its credentials. The failure SHALL still read as a failed `*` scan in the collection patch and make the run incomplete (exit 4). A collection that failed to read on an endpoint that opened SHALL NOT be listed: that is a server answering, not an endpoint unreached.

A source's SMTP channel SHALL be listed as `<source>.smtp` when its server could not be reached, with no `*` scan, a send channel holding no collection; it SHALL still make the run incomplete. Credentials it refused SHALL NOT list it: the sends report the refusal.

An exit 4 says the run did not do everything, and the patch says which collection failed; neither said whether the server was reached, so a caller told "offline" from "one folder unreadable" by matching the scan of `*`. Reported by MOA, whose "provider unreachable" read the first scan error of an exit-4 report.
