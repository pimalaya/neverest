---
cairn: log
change: an-unreached-endpoint-is-named
date: 2026-10-06
---

# An unreached endpoint is named in the report

An exit-3 run reported its unreachable server only as a failed `*` scan in the collection patch, so a caller saying "provider unreachable" read the first scan error, which an unreadable folder also gives. Reported by MOA (`scan_error`, `sync.rs`).

## What landed

- src/offline/driver.rs: `Unreached`, a marker carrying the endpoint's name, put on `Pool::open` failures in `open_source_contexts` and on the paired source and target of `run_pair`, the message unchanged ("Open connection", "Open source …", "Open target …"); `run` adds an `UnreachedEndpoint` when a source's failure carries it, beside the scan entry.
- src/sync/report.rs: `unreached: [{ endpoint, error }]`, omitted when empty, carried by `absorb`.
- Tests: `an_open_failure_reads_as_unreached_through_later_context` (unit); tests/drain.rs `an_unreachable_endpoint_is_named_in_the_report` (a closed port, exit 3, `caldav` named). Unit and drain tests green.

## Capabilities moved

- sync: an unreached endpoint is named in the report (new requirement).
