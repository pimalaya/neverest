---
cairn: log
change: the-pair-driver-takes-its-run
date: 2026-11-17
---

# The pair driver takes its run

The rest of finding NEV-14 of the CLI family analysis of 2026-11-15. No behaviour moved.

## What landed

- `run_pair` took 20 parameters. The run's options travel as one `AccountRun` (account, configuration, mode, scope, store directory, dry run, absences, connections, progress), a pairing as one `Pairing`, and the opened pair as `OpenPair`, whose methods are the steps `run_pair` held inline: open, drain the queues, plan the collections, sync each one.
- `sync_collection`, `sync_collection_into`, `propagate` and `reconcile_pass` became `OpenPair` methods. pair.rs goes from seven `clippy::too_many_arguments` allowances to one (`reconcile_side`), the crate from 16 to 10.

## Left

- `run_local` and the single-source path keep their long parameter lists; they could take `AccountRun` too.
