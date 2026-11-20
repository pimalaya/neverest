---
cairn: log
change: the-contract-audit-rulings
date: 2026-11-20
---

# The contract audit rulings

neverest follows pimalaya-cli's contract where its own spec had ruled otherwise, as the owner ruled on the 2026-11-20 audit (NEV-X1 to NEV-X14): one `errors` list, the kit's account check and queue rows, usable conflict ids and one body shape. It reverses the named failure lists of 2026-11-17 and narrows the N6 progress-line ruling.

## What landed

- src/sync/report.rs: `SyncError` (pimalaya-cli's `ItemError` flattened, then `SyncErrorRow`, tagged `kind`) and `SyncOutput.errors`; `failures`, `rejected`, `duplicates`, `refusals` and `unreached` are no lists, `collectionPatch` and `itemPatch` hold hunks, `submitted` and `intents` what was performed; `note_*` methods route a failure into `errors`. `ParkedQueueAction` is the kit's queue row, built from the store's parked row and its identity. `ItemConflict` prints `id` (the store seq), the store `collection` and `remoteId`.
- src/sync/driver/*: every producer of a failure goes through the report's `note_*`; `itemize_conflicted` reads the item's seq; a refused duplicate is coded `item-duplicate`.
- src/cli/check.rs: `account check` on the kit's shape, `BackendCheck` flattened into each row with neverest's keys after, `tls` from the endpoint's table, text as pimalaya-cli's fields.
- src/config.rs, src/config/{tls,source,smtp}.rs: the account `tls` table, `TlsConfig::inherit` (himalaya's), `secured` and `checked`.
- src/conflict/report.rs, src/conflict.rs, src/cli/conflict.rs: `{contents, encoding}` bodies on base64-codec, `remoteId`, `conflict-not-found`, exit 3 on an aborted merger, imperative abouts.
- src/main.rs, src/cli/main.rs, src/cli/sync.rs: notices off a terminal for every command but a full sync, streaming for a full sync alone, the kit's `meet_bare_invocation`, the shutdown installed before `--init` and the lock wait, which a signal ends.
- src/cli/queue.rs: the store's code on a failing queue read, failing the store's accounts; every parked row of the account reported.
- src/sync/driver.rs, src/sync/state.rs: `account-uninitialized`.
- src/config/mode.rs: the mode sentence agrees with its endpoints.
- Cargo.toml: base64-codec; `[patch.crates-io]` pimalaya-cli on ../cli (the kit's unreleased account check and bare invocation), to become a release pin.

## Spec

- [sync](../spec/sync.md): see the change's delta; *A failing row carries its code and its causes* gave way to *A run's failures are one list*.

## Tests

Failing on HEAD cc64130 (a snapshot built in a scratchpad): tests/exit.rs `a_signal_during_the_lock_wait_prints_the_interrupted_result`, `a_bare_invocation_under_json_is_a_refused_command_line`, `a_standard_command_without_its_argument_is_a_refused_command_line`, `a_failed_check_prints_its_report_and_exits_3`, `an_undrained_account_prints_its_report_and_exits_4`; tests/queue.rs `a_malformed_row_fails_the_drain_coded`, `a_drain_that_parks_a_row_exits_3`, `an_unreachable_endpoint_is_named_in_the_report`; tests/accounts.rs `a_run_covers_every_account_and_reports_each_apart`; tests/check.rs `a_check_names_every_endpoint_that_did_not_answer`. Unit tests on the new types: `the_account_tls_is_inherited_field_by_field`, `the_mode_sentence_agrees_with_its_endpoints`, `a_queue_row_is_keyed_row_and_a_parked_one_reads_as_a_queue_row`, the conflict body pair, and the conflict id in `a_conflict_on_each_endpoint_is_named_once_across_a_convergence_loop`.

A real `account check` and a real sync and drain result validated against the regenerated schemas.

## Left

NEV-X2: a first signal still waits out a hung read; pimalaya-stream needs a cancellation hook its retry loop polls.
