---
cairn: delta
change: the-report-rows-speak-the-family-nouns
---

# Delta

## ADDED Requirements

### Requirement: A queue row is keyed `row`
Every report row naming a row of the store's queue SHALL name it `row`, as himalaya's `pimdir queue list` does: `applied` and `parked` of a drain; `parked`, `submitted` and `intents` of a sync; and an overruled body a queued action settled (`id` naming the item a side's write settled stays `id`). A parked row, of a drain or of a sync, SHALL read `{row, collection, kind, producer, queuedAt, attempts, code, error, sources}`: `kind` the action as the store spells it, kebab-case (`add`, `update`, `set-flags`, `remove`, `move`, `copy`, `submit`, `calendar-reply`…), `queuedAt` the instant the store stamped the row with, `attempts` how many times it was tried before it was parked. A drained row SHALL read `{row, collection, kind, seq}`. A rejected write SHALL spell its `action` the same way (`set-flags`).

#### Scenario: A parked flag change
- GIVEN a `set-flags` row himalaya queued, which the drain parks
- WHEN `neverest --json drain` reports it
- THEN its `parked` row reads `{row, collection, kind: "set-flags", producer: "himalaya", queuedAt, attempts, code: null, error, sources: []}`

## MODIFIED Requirements

### Requirement: A check reports what each source declares it can do
`neverest check` SHALL list, for each source, the capabilities a sync declares for it in the store (pimdir STORAGE §15.6) as `capabilities: [{ name, level, detail }]`, one row per capability, `level` being pimalaya-cli's capability level as every pimalaya binary prints it (`full`, `partial` or `none`, the store stating no `server`) and `detail` what a partial level leaves out or why it is refused, `null` otherwise, from its backend and configured rights alone. An intent a source performs on the collections it holds (`calendar.reply`, `calendar.cancel`, their `.occurrence` options, `collection.create`, `collection.rename`, `collection.delete`), which the store gets as a `none` source-wide row and one row per collection, SHALL read as it does on those collections: the store's collections are unknown to a check, so the `none` a rename or a delete gets on a collection stating a role is not listed. The source-wide `calendar.component.vtodo` row a sync declares from the configuration (a Google Calendar or Graph calendar source, or a source one of whose targets is one) SHALL be listed with them. What a CalDAV listing states of a calendar SHALL be listed on that collection as `takesTasks`, `false` for a component set without `VTODO`, `true` for one naming it, `null` where no set is stated, on a target's collections as on a source's: a sync declares it as `calendar.component.vtodo` `none` on the source's calendar of that name. A target and an undeclared (JMAP) source SHALL list no capability. A capability a source learns to declare is reported by the check with no change to it.

#### Scenario: A check names the calendars that take no tasks
- **GIVEN** a CalDAV source whose server lists `events` with `VEVENT` alone and a Google Calendar target
- **WHEN** `neverest check --json` runs
- **THEN** `events` is listed with `takesTasks: false`, and the source's capabilities hold `calendar.component.vtodo` at level `none` naming the target

### Requirement: A failing row carries its code and its causes
A report row naming what a run could not do to one item or one endpoint SHALL carry, beside its own keys, those of pimalaya-cli's item error but its id: `code`, `null` when the failure carries none; `error`, the outermost message; and `sources`, the messages of its causes, outermost first. These rows are the collection and item patch entries, `rejected`, `submitted`, `intents`, `parked` and `unreached` of a sync, and `parked` of a drain. A `rejected` row SHALL be pimalaya-cli's `ItemError` itself, its `id` the item's handle on the side that refused it, beside `side`, `collection` and `action`. A row that succeeded SHALL carry `code` and `error` null and `sources` empty. The text output SHALL print a row's message and its causes as one chain, as `{err:#}` reads.

A parked queue row SHALL carry the error the store recorded, with no code and no causes, the store keeping its text alone. An unconfirmed send's `error` SHALL begin `unconfirmed: `, its causes under `sources`, so its message and causes joined read as its parked row does.

An account's row of `sync` and `drain` and the index refresh SHALL carry `code`, `error` and `sources` the same way, `error` the outermost message, `null` and `[]` when it ran. Their `code` SHALL be read as a row's is, a server's refusal included; the index refresh's is the store's (`index-version`, `index-fts5-missing`). An account's row of `init` SHALL keep the shape of a check row, `code` beside an `error` holding the whole chain: `init` names the endpoints it opened `sources`.

#### Scenario: A refused update under `--json`
- GIVEN a CardDAV source refusing an update 403
- WHEN `neverest --json sync` reports it
- THEN its `rejected` row reads `{side, collection, id, action: "update", code: "permission-denied", error: "Cannot update <id> in <collection>", sources: ["WebDAV server returned HTTP 403…"]}`, and the text prints `Cannot update <id> in <collection>: WebDAV server returned HTTP 403…`

#### Scenario: An account that failed
- GIVEN an account whose store cannot be opened
- WHEN `neverest --json sync` reports it
- THEN its row reads `error` the outermost message and `sources` its causes, and the text prints them as one chain

### Requirement: A sync prints its progress as events
Under `--json`, `neverest sync` SHALL print its progress as events before its result, one line each, each an object carrying `event`: `account-started` (`account`, `position`, `total`) before each account the run covers, the events after it being that account's; `opened` (`endpoint`) once an endpoint's first connection is open, `listed` (`endpoint`, `collections`) once it listed its collections; for a pair, `collection-started` (`source`, `target`, `collection`, `position`, `total`), `collection-progressed` (`source`, `target`, `collection`, `done`, `total`) and `collection-finished` (`source`, `target`, `collection`, then `code`, `error` and `sources` as an item error carries them, `code` and `error` `null` and `sources` `[]` unless the collection stopped short); for a source with no target, `scan-progressed` and `download-progressed` (`source`, `done`, `total`), each printed as its phase starts. An endpoint SHALL be named as its account names it. A progress count SHALL be printed at most once per percent of its total, a new count of a collection's next phase starting again. Events SHALL report progress only: everything the run did or left SHALL stay in the result, the last line. The events SHALL be registered as `neverest-sync.events`.

Text mode SHALL print no event, the spinner on stderr being their text form; under `--json` no spinner SHALL be drawn. `sync` SHALL print through pimalaya-cli's streaming printer, as carillon's `watch` does, so an event it prints reaches a pipe as it comes.

#### Scenario: A frontend follows a long download
- GIVEN an account with no target whose store owes 2 bodies
- WHEN `neverest sync --json` downloads them
- THEN it prints `account-started`, then `download-progressed` with `done` 0, 1 and 2 of `total` 2, then the result

#### Scenario: A script reads the result alone
- GIVEN a sync piped to a script
- WHEN the run succeeds
- THEN the last line is the result, listing each account's report, without an `event` key, and every line before it carries one


## REMOVED Requirements

None.
