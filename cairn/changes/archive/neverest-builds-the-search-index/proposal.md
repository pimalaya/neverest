---
cairn: change
id: neverest-builds-the-search-index
status: landed
created: 2026-10-30
---

# neverest builds the search index

## Why

Pimdir stable-release plan (2026-10-30), Phase A. The frontends query the store's search index (index.db, pimdir SEARCH) and never build it (himalaya's plan-himalaya-pimdir-search: "the owner builds and refreshes the index at sync time, himalaya only queries"). neverest is the store's sole owner, so it is the indexer. io-pimdir's a-query-only-index (2026-10-30) gave frontends `PimdirIndex::open_query`, which refuses a missing index as `index-missing`: without an owner building it, himalaya's search has nothing to answer from.

## What

- A `search` cargo feature, enabling io-pimdir's, **on by default**: a default himalaya searches only what a default neverest built, so a default neverest builds it. Off, neverest neither refreshes nor offers `index`, for a build that would rather not carry mail-parser's full encodings, vCard and the bundled time zone database. The ical-rs pin moves to the revision io-pimdir's search reads occurrences with (99b9273, one commit past the one neverest pinned).
- After a sync, last of all and outside a dry run, neverest refreshes index.db for at most `store.index-timeout` (60 seconds by default, `"0"` turns it off); after a drain, a run performing intents alone or declaring collections, for 2 seconds at most, a frontend waiting on those. A refresh cut by its deadline resumes on the next run.
- `neverest index` builds it whole, a second-long slice at a time with its progress on a spinner, reading no credential, opening no endpoint and taking no sync.lock: a sync meeting the build reports the index `busy`. Another indexer holding the index answers `busy` and exit 3.
- The sync report and the drain output carry `index`: folded, indexed, `behind` (stamps not folded yet), `busy`, and an `error`. An index failure is a warning and never changes the exit code: the index is derived.
- `--reset` drops index.db with an idle store, and `--reset --force` moves it aside with the store, never deleting it (a-reset-keeps-pending-work already did both; now tested).
