---
cairn: change
id: the-index-reads-the-user-s-zone
status: landed
created: 2026-11-06
---

# The index reads the user's zone

## Why

The indexer reads floating times and dates in the zone it states (pimdir SEARCH §7, `index_meta.floating_zone`), and neverest stated none, leaving `PimdirIndexRefreshOptions::floating_zone` at its `UTC` default: in Paris a floating 09:00 meeting stood at 10:00, and an all-day event on 7 November ran from 01:00 on the 7th to 01:00 on the 8th. A client SHOULD state the user's zone, the one its indexer states (SEARCH §8.1), and the query clients read it back from the index (`PimdirIndex::floating_zone`), so the indexer's zone is the one every frontend cuts its days in. calendula/cairn/changes/pimdir-search-reads-local-days names this half as neverest's.

## What

- src/offline/index.rs: `floating_zone()` resolves the user's zone once a run through `jiff::tz::TimeZone::try_system()`, which reads `TZ` then the system's, and takes its IANA name; a zone with no IANA name (a POSIX `TZ` rule, a TZif file read unnamed) or none found reads as `UTC` with one `warn!`. `refresh` takes the zone; the refresh after a run and `neverest index` pass `floating_zone()`.
- Cargo.toml: `search` enables `dep:jiff` and `jiff/tz-system`.
- The first refresh in another zone re-expands every live calendar item: io-pimdir does it (SEARCH §7).
- Docs: cairn/spec/sync.md (the index requirement, the error codes' `index-zone-unknown`), README.md, CHANGELOG.md.

## Not in this change

- A `--floating-zone` option or a config key: `TZ` already overrides the system's zone per run.
