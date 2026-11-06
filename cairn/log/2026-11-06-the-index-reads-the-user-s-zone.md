---
cairn: log
change: the-index-reads-the-user-s-zone
date: 2026-11-06
---

# The index reads the user's zone

neverest stated no zone to the indexer, so the index read floating times and dates in UTC (pimdir SEARCH §7): a floating 09:00 meeting stood at 10:00 in Paris. Every refresh now states the user's zone.

## What landed

- src/offline/index.rs: `floating_zone()` resolves the zone once a run (`OnceLock`) through `jiff::tz::TimeZone::try_system()`, which reads `TZ` (empty reading as UTC) then the system's, and takes its IANA name. A zone with no IANA name (a POSIX `TZ` rule, a zone read from an unnamed TZif file) or none found reads as `UTC`, warning once. `refresh` takes the zone, which the refresh after a run (`after_run`) and `neverest index` (src/cli/index.rs) pass. A name comes from jiff's own database, the one io-pimdir checks it against, so `index-zone-unknown` stays out of neverest's codes (src/error.rs header).
- Cargo.toml: `search` enables `dep:jiff` and `jiff/tz-system`.
- io-pimdir re-expands every live calendar item on the first refresh in a zone the index does not record (SEARCH §7).
- Docs: cairn/spec/sync.md ("The store's owner builds its search index", with the scenario "A floating meeting in Paris"; "A store error carries its code"), README.md, CHANGELOG.md.

## Evidence

- `a_refresh_records_the_zone_it_states` refreshes in `Europe/Paris` and reads it back through `PimdirIndex::floating_zone` on an `open_query` handle. With the options' zone put back to `FLOATING_ZONE` it failed: `left: "UTC"`, `right: "Europe/Paris"`.
- `a_zone_without_an_iana_name_reads_as_utc`: `Europe/Paris` names itself, a POSIX rule (`CET-1CEST,M3.5.0,M10.5.0/3`) and an unknown name read as `UTC`.

## Risk

A run whose `TZ` differs from the last one's (a cron job without `TZ` on a machine whose shell sets one) re-expands every calendar item at each switch.

## Capabilities moved

- sync (cairn/spec/sync.md): "The store's owner builds its search index", "A store error carries its code".
