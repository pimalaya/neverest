---
cairn: change
change: list-instead-of-query
---

# Delta

## ADDED Requirements

### Requirement: A DAV server without `sync-collection` is listed instead
`sync-collection` is an extension, so a server MAY implement none of it,
advertising a `supported-report-set` of `addressbook-multiget` and
`addressbook-query` alone. Such a collection SHALL be enumerated through a
`PROPFIND` at Depth 1 requesting the ETag, which yields the same member ids and
revisions, rather than failing to enumerate at all.

The `PROPFIND` SHALL be preferred over the `addressbook-query` the same server
advertises. A query carries a filter, a server evaluates a filter by parsing
every member, and a collection holding one member the server cannot parse then
fails to enumerate at all, which is the case this recovery exists for. A
`PROPFIND` parses nothing.

The listing SHALL be chosen from the collection's advertised
`supported-report-set` where a run has read it, a sync listing its collections
before it enumerates them, and from the RFC 3253 §3.6 `DAV:supported-report`
precondition otherwise, which is the server saying by name that it does not run
the report. It SHALL NOT be chosen on the HTTP status alone, the status wrapping
that precondition being the server's own choice, except for `405` and `501`
which mean the request was never going to run. A permission refusal, a
credential failure and a server fault SHALL all surface as failures, and a
rejected sync token SHALL keep its own recovery, a fresh full report.

The listing carries no sync token, so its checkpoint SHALL be empty and an empty
checkpoint SHALL read as no cursor: such a collection is enumerated in full on
every run, which is what a server offering nothing incremental costs.

A listing the server truncates SHALL be reported as a delta rather than as a
complete snapshot. A snapshot is read as "absence means removed", so a truncated
one taken for a whole collection deletes every member the server left out.

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

## REMOVED Requirements

### Requirement: A DAV server without `sync-collection` is enumerated by query
Replaced by the listing above: the query it named fails on a collection holding
a member the server cannot parse, which is the deployment this recovery exists
for, and it reported a truncated result as a complete snapshot.
