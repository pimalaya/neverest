---
cairn: delta
change: the-index-reads-the-user-s-zone
---

# Delta

## ADDED Requirements

## MODIFIED Requirements

### Requirement: The store's owner builds its search index
A build with the `search` feature (on by default) SHALL refresh the store's search index (index.db, pimdir SEARCH §3, §4) after a sync, last, for at most the account's `store.index-timeout` (60 seconds unless set, `"0"` turning it off), and after a drain, a run performing the queued intents alone and a run declaring collections, for at most 2 seconds. A dry run SHALL NOT refresh. A refresh cut by its deadline SHALL leave the index behind the store and the next refresh SHALL go on from there.

The run SHALL report the refresh under `index`: the changes folded, the bodies newly indexed, the stamps still `behind`, whether another indexer held it (`busy`), and the error of a failed one. An index failure SHALL be a warning and SHALL NOT fail the run or change its exit code: the index is derived from the store.

`neverest index` SHALL build the index until it holds every change the store has, reading no credential and opening no endpoint, and SHALL exit 3 answering `busy` while another indexer holds it.

A reset SHALL take the index with the store: dropped with an idle store, moved aside with it by `--force`.

An index of the canonical shape at an earlier search draft SHALL be rebuilt by the next refresh; an index of another shape, or at a later or unknown version, SHALL be refused as `index-version` and left as it is, never rebuilt, written nor deleted (pimdir SEARCH §2), the refusal reported under `index` like any index failure. Every refresh SHALL state the user's zone as the index's floating zone (pimdir SEARCH §7, §8.1): the IANA name of the zone `TZ` names, else of the system's; a zone with no IANA name (a POSIX `TZ` rule) or none found SHALL read as `UTC`, with one warning a run. The first refresh in another zone than the index records re-expands every calendar item.

#### Scenario: A message a sync landed is found by a frontend
- **GIVEN** a sync landing a message whose subject says "Quarterly report"
- **WHEN** a frontend opens the index for queries alone and searches `quarterly`
- **THEN** it finds the message

#### Scenario: A first build longer than the deadline
- **GIVEN** a store whose index a refresh's deadline cut
- **WHEN** the next run refreshes it
- **THEN** it goes on from where the cut left it until the index is current

#### Scenario: A floating meeting in Paris
- **GIVEN** `TZ=Europe/Paris` and a calendar holding a meeting at a floating 09:00 on 7 January
- **WHEN** a run refreshes the index
- **THEN** the index records `Europe/Paris` as its floating zone and places the meeting at 08:00 UTC, 09:00 in Paris

### Requirement: A store error carries its code
A store failure that ends a command SHALL carry, in its JSON failure object, the code io-pimdir gives it (`PimdirError::code`), spelled the same: `store-version`, `store-uncreated`, `store-foreign`, `store-corrupt`, `store-not-current`, `store-hash-algo`, `sync-version`, `store-busy`, `store-owned`, `store-staging`, `store-in-flight`, `store-exposed`, `action-malformed`, `flag-invalid`, `unsupported`, `index-fts5-missing`, `index-version`. A failure a caller cannot act on (an SQL or I/O error) SHALL carry none, nor SHALL a code neverest cannot meet (`index-busy`, which `index` answers as `busy`; `index-missing`, `index-outdated` and `index-query-only`, a query client's; `index-zone-unknown`, neverest stating only a zone its time zone database named; `cutoff-invalid`, neverest writing its cutoff as the store stamps; `conflict-unread`, which the one-way overwrite answers by fetching the unread body). No message SHALL advise deleting the store: the remedies are an upgrade, the reinstall of "A store the format outgrew is refused with its remedy", `sync --reset` on a store holding no pending work, and `sync --reset --force`, which moves it aside.

#### Scenario: A downgrade under `--json`
- **GIVEN** a store whose `store_meta.format_version` names a later draft
- **WHEN** neverest runs `--json drain` on the account
- **THEN** it exits 1 printing a failure object whose `code` is `store-version`, the message asking for an upgrade and naming no reset

#### Scenario: A store root naming another program's database
- **GIVEN** a pimdir.db holding another program's tables
- **WHEN** neverest runs `--json drain` on the account
- **THEN** it exits 1 printing a failure object whose `code` is `store-foreign`, the database untouched

## REMOVED Requirements
