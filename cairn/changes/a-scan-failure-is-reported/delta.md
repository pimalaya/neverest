---
cairn: change
change: a-scan-failure-is-reported
---

# Delta

## ADDED Requirements

### Requirement: A collection that failed to scan is reported, never only logged
A collection whose spine fails SHALL be recorded in the run's report, carrying
its error, and SHALL NOT be reported through the log alone. The other
collections SHALL still run: they share nothing but the file the store lives in.

A run that failed to scan a collection SHALL NOT report itself in sync. "In
sync" is a claim about what the sync compared, and a collection it could not
enumerate was never compared; a run that says it anyway hides a broken account
for as long as nobody reads the log.

An error crossing an engine boundary SHALL be rendered with its full chain, not
with its outermost context alone. A backend keeps a server's status and response
body so a caller can read them, and a wrapper that renders only the top drops
exactly the part naming what the server said.

#### Scenario: A refused enumeration surfaces
- GIVEN a source whose server refuses to enumerate a collection
- WHEN the account is synced
- THEN the report names the collection, the run does not claim to be in sync, and the error carries the server's status
