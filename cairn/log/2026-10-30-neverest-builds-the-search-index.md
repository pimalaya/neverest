---
cairn: log
change: neverest-builds-the-search-index
date: 2026-10-30
---

# neverest builds the search index

The store's owner is its indexer (plan-himalaya-pimdir-search, io-pimdir a-query-only-index): himalaya opens index.db for queries alone and finds nothing until an owner builds it.

## What landed

- Cargo.toml: the `search` feature (`io-pimdir/search`), on by default, so a default himalaya finds an index a default neverest built. The ical-rs pin moves from caa1422 to 99b9273, the revision io-pimdir's search reads occurrences with (one commit later: `recur::instant`, `tzdb::zones`, `tzdb::version`); Cargo.lock changes that source and the dependencies `search` activates (ical-rs with `jiff-tzdb`, mail-parser with `encoding_rs`, vcard-rs under io-pimdir), no windows-sys flip.
- src/offline/index.rs (gated once in src/offline/mod.rs): `after_run`, the refresh after a run, for `store.index-timeout` or `QUICK` (2 seconds), reporting a failure and a held index (`busy`) rather than raising; `refresh`, one pass under a deadline.
- src/offline/driver.rs: `run` refreshes last, outside a dry run, briefly after a run performing intents alone or declaring collections. src/cli/drain.rs refreshes briefly after a drain.
- src/cli/index.rs: `neverest index`, the whole build in second-long slices with its progress on a spinner, no sync.lock taken; a held index answers `busy`, exit 3. `neverest-index` schema.
- src/config.rs: `store.index-timeout`, 60 seconds by default, `"0"` turning the refresh off.
- src/sync/report.rs: `index` in the sync report, a failure a warning that changes no exit code.
- Tests: src/offline/index.rs `a_refresh_after_a_run_indexes_what_it_landed` (read back through `open_query`, as himalaya does), `a_refresh_its_deadline_cut_is_continued_by_the_next`, `an_index_failure_is_reported_not_raised`, `a_zero_timeout_refreshes_nothing`; src/cli/exit.rs `an_index_refresh_never_changes_the_exit_code`; src/cli/sync.rs `a_reset_takes_the_search_index_with_the_store` (the reset already did both, since a-reset-keeps-pending-work).

## Notes

- `index` takes the subcommand prefix `i` from `init`: `neverest in` is now ambiguous under clap's inferred subcommands, `neverest ini` still is `init`.
- io-pimdir's wave 3d (the index shape of SEARCH's 0001_init.sql, announced 2026-10-30) rebuilds an index of another shape on the indexer's open, which `after_run` and `index` both go through.

## Capabilities moved

- sync: the store's owner builds its search index (added).
