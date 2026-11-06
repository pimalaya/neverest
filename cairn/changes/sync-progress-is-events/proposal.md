---
cairn: change
id: sync-progress-is-events
status: landed
created: 2026-11-03
---

# Sync progress is events

## Why

pimalaya-cli 0.3 (change every-output-is-data) lets a command print events before its result, and its review of 2026-11-03 left neverest one ledger line: "sync progress as events". A sync can run for hours, and under `--json` it printed nothing until its report: a frontend waiting on it had the spinner on stderr, which it cannot parse, or nothing at all when stderr is not a terminal.

The same release gives the registry a check over the clap command tree, which replaces neverest's hand-written walk, and builds schemas from the serialize contract.

## What

- `neverest sync --json` prints `SyncEvent`s (src/sync/event.rs) before its report, one line each, each carrying `event`: `opened` and `listed` per endpoint, `collection-started`, `collection-progressed` and `collection-finished` per collection of a pair, `scan-progressed` and `download-progressed` per source with no target. Progress counts print at most once per percent (`PercentGate`, src/offline/progress.rs), so a run over 100,000 mails prints hundreds of lines, not 100,000.
- Events report progress only: what the run did and left stays in the report, so a caller reading the last line alone loses nothing.
- Text mode keeps the spinner, its text form, and prints no event; `--json` draws no spinner, which would interleave with the event lines on a terminal.
- A collection whose full hydration fails now says so (`collection-finished` with its `error`, and the spinner line), where it said `done`.
- The registry is built with `JsonSchemaRegistry::new` and `insert`, registers `neverest-sync.events`, and a test asserts `check` over the command tree is empty, bare `neverest` (help or wizard) declared silent. The toolkit commands `completion`, `manual` and `json-schema` are now described too.

## Compatibility

Breaking for a caller parsing `sync --json` stdout as one document: it must read the last line, or skip the lines carrying `event`. Schemas now follow the serialize contract, so a key always printed is required, `null` or not. Both land in the next release, unreleased.

## Left

The rest of neverest's ledger line in pimalaya-cli every-output-is-data: `mode` prose and `null` vs omitted keys in the report types.
