---
cairn: change
change: connector-listings-fail-rather-than-shrink
---

# Delta

## ADDED Requirements

### Requirement: A DAV member row answered with an error makes its listing incomplete
A `sync-collection` member row the server answered with a non-2xx status other than 404 (removed) and 507 (truncated) SHALL make the listing incomplete, read as a delta: a 403 or a 5xx member is not a deleted one.

### Requirement: A refused DAV sync token restarts the listing
A sync token the server refuses, with a 403 `valid-sync-token` precondition or a 409, SHALL restart the listing from no token rather than fail the collection in every run.

### Requirement: A Graph series is read whole or not at all
A Graph event series SHALL be read with every exception it holds, past five years included, or fail its read: a partial series pushed to another endpoint loses the exceptions left out.

## MODIFIED Requirements

None.

## REMOVED Requirements

None.
