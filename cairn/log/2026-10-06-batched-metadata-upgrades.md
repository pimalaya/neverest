---
cairn: log
change: batched-metadata-upgrades
landed: 2026-10-06
---

# Batched metadata upgrades

Meta-tier probes now fetch and commit through the existing engine in batches of at most 64 handles. A later failure keeps earlier batches, and a new invocation selects the remaining probes. The full probe list is sorted lexically before partitioning, preserving duplicate identity claims. Full-tier probes and their dry-run guard are unchanged.

## Verification

The unbounded implementation failed the 65-probe request regression. Six new store-backed regressions passed after the fix, covering boundaries, failure/reopen/resume, duplicate identities, eligibility, missing replies and dry runs. The all-feature suite passed 226 unit tests. A final Full-object fixture improvement was rechecked with all six affected regressions.

Default and reduced feature checks, formatting, clippy, strict broken-link rustdoc and the dependency audit passed. Rust 1.97 clippy reports eight existing formatting-borrow warnings in untouched code. The missing-docs checks are blocked by the existing undocumented build.rs crate, identical to the upstream base.

The 50 existing live integration tests were ignored. No live provider or IMAP wire test was run, and no production binary or configuration was changed. The initial collection load remains collection-sized.

## Capabilities moved

sync: bounded metadata upgrades, durable completed batches and lexical duplicate identity order.
