---
cairn: log
change: the-report-rows-speak-the-family-nouns
date: 2026-11-15
---

# The report rows speak the family nouns

Lines N5 and N6 of the CLI alignment round of 2026-11-15 (audit findings F15 and F16; JSON shape review lines A1, A3, A4, B9).

## What landed

- src/cli/check.rs: `CheckedCapability.level`, pimalaya-cli's `CapabilityLevel`, in place of `support: String`.
- src/cli/drain.rs, src/sync/report.rs: every queue row keyed `row` (`DrainedAction`, `ParkedAction`, `ParkedQueueAction`, `SubmitEntry`, `IntentEntry`, `OverruledBody`); both parked rows read `{row, collection, kind, producer, queuedAt, attempts, code, error, sources}`, the sync's `action` key now `kind`. src/offline/remote.rs: a rejected flag push is `set-flags`.
- src/cli/sync.rs, src/cli/drain.rs, src/offline/index.rs: `AccountSync`, `DrainOutput` and `IndexRefresh` carry `sources`, `error` the outermost message; the text joins them.
- src/sync/report.rs: `RejectedWrite` flattens pimalaya-cli's `ItemError`; its JSON is unchanged (the existing test on it passes as it was).
- src/main.rs: `sync` prints through `StdoutPrinter::streaming()`.
- Tests: `a_capability_row_carries_the_shared_level`, `a_queue_row_is_keyed_row_and_a_parked_one_names_its_kind`, `a_drain_keys_its_rows_row`, `a_failed_account_carries_its_causes_apart`, `an_undrained_account_carries_its_causes_apart`; the overruled and unconfirmed send tests read `row`; the ignored suites read `row` and `level`.

## Left

- `init` keeps its whole chain in `error`: its `sources` key names the endpoints it opened.

## Capabilities moved

- [sync](../spec/sync.md): *A queue row is keyed `row`* added; *A check reports what each source declares it can do*, *A failing row carries its code and its causes* and *A sync prints its progress as events* amended.
