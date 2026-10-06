---
cairn: change
change: an-unreached-endpoint-is-named
---

# Delta

## ADDED Requirements

### Requirement: An unreached endpoint is named in the report
A run SHALL list in its report, under `unreached`, every endpoint whose connections it could not open, with the error: a server it could not reach, or one refusing the connection or its credentials. The failure SHALL still read as a failed `*` scan in the collection patch and make the run incomplete (exit 3). A collection that failed to read on an endpoint that opened SHALL NOT be listed: that is a server answering, not an endpoint unreached.

An exit 3 says the run did not do everything, and the patch says which collection failed; neither said whether the server was reached, so a caller told "offline" from "one folder unreadable" by matching the scan of `*`. Reported by MOA, whose "provider unreachable" read the first scan error of an exit-3 report.

#### Scenario: A closed port
- **GIVEN** an account whose CalDAV source points at a port nothing listens on
- **WHEN** it is synced
- **THEN** the run exits 3 and `unreached` names `caldav` with an error beginning `Open connection`

## MODIFIED Requirements

None.

## REMOVED Requirements

None.
