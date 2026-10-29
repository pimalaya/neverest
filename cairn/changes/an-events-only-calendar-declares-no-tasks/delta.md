---
cairn: delta
change: an-events-only-calendar-declares-no-tasks
---

# Delta

## ADDED Requirements

### Requirement: A calendar its server keeps from tasks is declared so
A CalDAV source SHALL declare `x-vtodo` `none` (pimdir STORAGE §15.6) on each calendar it holds whose `supported-calendar-component-set` (RFC 4791 §5.2.3) its listing reads without `VTODO`, the detail naming why, so the store's gate refuses a task added or moved there before it is queued and a client reading the store does not offer the calendar as a task list. A calendar whose listing states no component set, which takes any component, or one naming `VTODO`, SHALL get no `x-vtodo` row; no other backend declares one.

What a listing stated SHALL be kept in the sidecar (neverest.json, `taskless`) by source, replaced at the source's next listing, so the declaration before the drain, which opens no endpoint, carries it from the run that listed it. A source never listed declares no `x-vtodo` row. An account with targets records none.

#### Scenario: An events-only calendar refuses a task before the push
- **GIVEN** a CalDAV source whose last listing read `events` with the component set `VEVENT` alone and `work` with `VEVENT` and `VTODO`
- **WHEN** the account's sources are declared and a frontend then queues a VTODO add into each
- **THEN** the add into `events` is refused at the producer's gate as `x-vtodo` unsupported, the store reads `events` as taking no tasks, and the add into `work` is queued

## MODIFIED Requirements

### Requirement: Every source declares its capabilities before the drain
At every run, before it drains an action, neverest SHALL declare each source it runs in the store (pimdir STORAGE §15.6): every Annex B capability of the source's kind, from its backend and its configured rights alone, `none` with the reason, and the `x-vtodo` rows of `A calendar its server keeps from tasks is declared so`, read from the sidecar rather than the network. A right the configuration withholds SHALL be `none`; a one-way source SHALL support no write. A JMAP source stays undeclared.

#### Scenario: A source is declared with no network
- **GIVEN** a Graph source whose token command fails
- **WHEN** neverest runs
- **THEN** the source's declaration is in the store before the credential is read

### Requirement: A drain applies the queue without syncing
`neverest drain` SHALL apply the account's queued actions as a sync's drain does, after declaring the sources as a sync does before its drain (the configuration and the sidecar), reading no credential and opening no endpoint. It SHALL NOT wait for the store: while a run holds `sync.lock` it SHALL report `busy` and exit 3, the queue untouched. It SHALL report each row it applied, with the `seq` of the item an `add` created, the rows it parked, and how many it left for a sync.
