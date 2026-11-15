---
cairn: change
change: dav-without-sync-collection
---

# Delta

## ADDED Requirements

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
