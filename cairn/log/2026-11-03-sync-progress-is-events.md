---
cairn: log
change: sync-progress-is-events
date: 2026-11-03
---

# Sync progress is events

`neverest sync --json` prints `SyncEvent`s (src/sync/event.rs) before its report: `opened`, `listed`, `collection-started`, `collection-progressed`, `collection-finished`, `scan-progressed`, `download-progressed`. The driver reports through `Progress` (src/offline/progress.rs), drawing the spinner in text mode and handing events to a sink over the printer under `--json`, where no spinner is drawn. Counts pass `PercentGate`, once per percent, a count of 1 reopening it for a collection's next phase. A failed full hydration now ends its collection on the error, where it said `done`.

The registry moved to `JsonSchemaRegistry::new` and `insert` (serialize contract), registers `neverest-sync.events`, and `the_registry_matches_the_command_tree` asserts pimalaya-cli's `check` is empty, bare `neverest` silent. The integration tests parse the last line of `sync --json`.

Capabilities moved: sync (cairn/spec/sync.md: "A sync prints its progress as events" added, "The spinner reports in-mailbox progress" and "A data command describes what it prints" modified). Guarded by `a_download_prints_its_progress_as_events`, `a_percent_passes_once`, `a_new_count_passes_again`. `cargo test --all-features` 486 passed, clippy clean.

Left from the pimalaya-cli every-output-is-data ledger line: `mode` prose and `null` vs omitted keys.
