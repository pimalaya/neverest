---
cairn: delta
change: an-events-only-calendar-declares-no-tasks
---

# Delta

## ADDED Requirements

### Requirement: A calendar its server keeps from tasks is declared so
A source SHALL declare `x-vtodo` `none` (pimdir STORAGE §15.6), the detail naming why, wherever a calendar it syncs takes no VTODO resource, so the store's gate refuses a task added or moved there before it is queued and a client reading the store does not offer the calendar as a task list:

- source-wide, from the configuration alone, for a Google Calendar or a Graph calendar source, whose provider keeps tasks apart (Google Tasks, Microsoft To Do);
- on each CalDAV calendar whose `supported-calendar-component-set` (RFC 4791 §5.2.3) its listing reads without `VTODO`.

A CalDAV calendar whose listing states no component set, which takes any component, or one naming `VTODO`, SHALL get no `x-vtodo` row, nor SHALL a source of another kind.

In an account with targets, a hub calendar takes a task only where both endpoints of its pair take one, the task crossing to the target. The source SHALL therefore declare `none` on each calendar of its namespace a target's listing reads without `VTODO`, as on those its own listing does, and source-wide when one of its targets is a Google Calendar or a Graph calendar endpoint, the detail naming that target. A target SHALL stay undeclared: a row would make it a declared source, refusing every capability it has no row for.

What a listing stated SHALL be kept in the sidecar (neverest.json, `taskless`) by source, as hub ids, replaced at the source's next listing, so the declaration before the drain, which opens no endpoint, carries it from the run that listed it. With targets, the record SHALL be replaced once every target of the source has listed; a run stopping earlier SHALL add what it listed to the record rather than drop what a target it did not reach stated. A source never listed declares no per-calendar row.

A run SHALL declare again as soon as a listing is recorded and its collections are in the store, before any item of theirs moves, so a calendar met for the first time carries its `x-vtodo` row from the listing on rather than from the end of the run. A created calendar is met by the listing that follows its creation, the same run's.

The store reads a source as syncing a collection from its `sources` row or a binding there (pimdir STORAGE §15.6, `load_capabilities`), and a declaration holds only where its source syncs. Before that declaration, a run SHALL therefore record the source as syncing each collection it is about to sync and holds no state in yet (local: every collection kept, all of them under `sync --declare-only`; paired: every collection common to the pair, the target left unrecorded as it stays undeclared): a `sources` row with no checkpoint, which reads as no sync state, written through the canonical `upsert_checkpoint` only after the canonical `load_checkpoint` found no row, in one transaction, so a checkpoint already there SHALL never be overwritten. A dry run records nothing. A failure SHALL be logged, the first item sync recording the source anyway.

#### Scenario: An events-only calendar refuses a task before the push
- **GIVEN** a CalDAV source whose last listing read `events` with the component set `VEVENT` alone and `work` with `VEVENT` and `VTODO`
- **WHEN** the account's sources are declared and a frontend then queues a VTODO add into each
- **THEN** the add into `events` is refused at the producer's gate as `x-vtodo` unsupported, the store reads `events` as taking no tasks, and the add into `work` is queued

#### Scenario: A Google or Graph calendar refuses a task
- **GIVEN** a Google Calendar source syncing `gcal/primary` and a Graph calendar source syncing `msgraph-calendar/AAMk`
- **WHEN** the account's sources are declared from the configuration alone and a frontend queues a VTODO add into each
- **THEN** both adds are refused at the gate as `x-vtodo` unsupported, the detail naming where the provider keeps tasks

#### Scenario: A target's events-only calendar refuses a task
- **GIVEN** a CalDAV source paired with a CalDAV target whose listing reads `events` with `VEVENT` alone, the source's listing stating no component set
- **WHEN** the pair lists its collections and a frontend then queues a VTODO add into `caldav/events`
- **THEN** the add is refused at the gate, under the source's declaration, and the target stays undeclared

#### Scenario: A calendar met mid-run is declared before the run ends
- **GIVEN** a CalDAV source whose listing meets `events`, `VEVENT` alone, for the first time
- **WHEN** the run records the listing and the first item sync of `events` lands, before the run's last declaration
- **THEN** the store already reads `events` as taking no tasks

#### Scenario: A calendar listed and not synced yet refuses a task
- **GIVEN** a CalDAV source whose listing meets `/caldav/v2/x%40g/events`, `VEVENT` alone, for the first time, and `work`, holding a checkpoint there
- **WHEN** the run records the listing, before any item sync
- **THEN** a VTODO add into `caldav//caldav/v2/x%40g/events` is refused at the gate, the source syncs that collection with no checkpoint, and `work` keeps its checkpoint

## MODIFIED Requirements

### Requirement: Every source declares its capabilities before the drain
At every run, before it drains an action, neverest SHALL declare each source it runs in the store (pimdir STORAGE §15.6): every Annex B capability of the source's kind, from its backend and its configured rights alone, `none` with the reason, and the `x-vtodo` rows of `A calendar its server keeps from tasks is declared so`, the source-wide ones from the configuration and the per-calendar ones from the sidecar rather than the network. A right the configuration withholds SHALL be `none`; a one-way source SHALL support no write. A JMAP source stays undeclared.

#### Scenario: A source is declared with no network
- **GIVEN** a Graph source whose token command fails
- **WHEN** neverest runs
- **THEN** the source's declaration is in the store before the credential is read

### Requirement: A drain applies the queue without syncing
`neverest drain` SHALL apply the account's queued actions as a sync's drain does, after declaring the sources as a sync does before its drain (the configuration and the sidecar), reading no credential and opening no endpoint. It SHALL NOT wait for the store: while a run holds `sync.lock` it SHALL report `busy` and exit 3, the queue untouched. It SHALL report each row it applied, with the `seq` of the item an `add` created, the rows it parked, and how many it left for a sync.

### Requirement: A check reports what each source declares it can do
`neverest check` SHALL list, for each source, the capabilities a sync declares for it in the store (pimdir STORAGE §15.6) as `capabilities: [{ name, support, detail? }]`, one row per capability, `support` being `full`, `partial` or `none`, from its backend and configured rights alone. An intent a source performs on the collections it holds (`calendar.reply`, `calendar.cancel`, their `.occurrence` options, `collection.create`), which the store gets as a `none` source-wide row and one row per collection, SHALL read as it does on those collections: the store's collections are unknown to a check. The source-wide `x-vtodo` row a sync declares from the configuration (a Google Calendar or Graph calendar source, or a source one of whose targets is one) SHALL be listed with them. What a CalDAV listing states of a calendar SHALL be listed on that collection as `takesTasks`, `false` for a component set without `VTODO`, `true` for one naming it, left out where no set is stated, on a target's collections as on a source's: a sync declares it as `x-vtodo` `none` on the source's calendar of that name. A target and an undeclared (JMAP) source SHALL list no capability. A capability a source learns to declare is reported by the check with no change to it.

#### Scenario: A check names the calendars that take no tasks
- **GIVEN** a CalDAV source whose server lists `events` with `VEVENT` alone and a Google Calendar target
- **WHEN** `neverest check --json` runs
- **THEN** `events` is listed with `takesTasks: false`, and the source's capabilities hold `x-vtodo` `none` naming the target
