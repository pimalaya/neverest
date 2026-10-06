---
cairn: change
id: an-unreached-endpoint-is-named
status: landed
created: 2026-10-06
---

# An unreached endpoint is named in the report

## Why

A run that cannot reach its server exits 3 and reports a failed scan of `*` in the collection patch (2026-10-04, an-incomplete-run-has-its-own-exit-code). A caller wanting to say "provider unreachable" had to read that scan entry: MOA took the first scan error of an exit-3 report (`scan_error`, `sync.rs`), which a single unreadable folder on a server that answered would also satisfy. The report should say it in a field of its own.

## What

- A marker on the error raised when an endpoint's connections cannot be opened (`Unreached`, the endpoint's name), in the one-source sync and in a paired run, for a source and for a target. The message the chain shows is unchanged.
- `unreached: [{ endpoint, error }]` in the sync report, filled where the run catches a source's failure, when the chain carries the marker. The scan entry and exit 3 stay as they were.

## Not done

No telling offline from a refused credential: both are an endpoint unreached, and the error says which. A caller with its own network probe (MOA) already tells them apart.
