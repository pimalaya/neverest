---
cairn: change
id: every-store-outcome-is-reported
status: landed
created: 2026-10-30
---

# Every store outcome is reported

## Why

io-pimdir wave 3c (cairn/log/2026-10-30-what-the-app-ports-found.md, 2026-10-30-sync-audit-2026-10-30.md, 2026-10-30-store-audit-2026-10-30.md in io-pimdir) gives its callers outcomes neverest dropped: the bodies a collection's policy kept aside while the drain applied a row (`PimdirDrainReport::overruled`, nothing else reports them), the pushes a connector answered nothing for (`PimdirSyncReport::unanswered`), the creates a server rejected (`PimdirSyncReport::rejected_creates`), the `Overruled` events of a mutation (`PimdirMutateReport::events`, which `keep_store_bodies` discarded), the partial supports the gate passed an action with (`enqueue_with_body_checked`), and the `store-foreign` code a reader now raises for a database another program wrote.

## What

- A drain's overruled bodies are reported under `overruled`, in the sync report and the `drain` output, named by the queue row (`action`), not by a side and a handle; `side` and `id` become optional. A warning: the policy decided as configured, no exit code changes.
- `unanswered` (side, collection, count): the pushes the last pass of a collection left without an outcome. Exit 3: they stay pending and the next run pushes them again, so a rerun clears it, which is the incomplete code's class, not the conflict code's.
- A create the engine names in `rejected_creates` that the connector recorded no rejection or refusal for is reported under `rejected` as a `create`. Exit 2, as every rejected write: a rerun meets the same refusal. The connector records every rejection it answers today, so this only keeps a later path from pushing a create for ever in silence.
- The one-way overwrite reads its mutation's report: an `Overruled` it answers is reported.
- `conflict resolve` stages through `enqueue_with_body_checked` and prints the partial supports the next push relies on (`partial` in its JSON).
- `store-foreign` relayed, and `index-fts5-missing` for the indexer (neverest-builds-the-search-index); `index-busy` is answered as `busy`, exit 3, by `neverest index`. `index-missing`, `index-outdated` and `index-query-only` answer a query client, which neverest never is, and `cutoff-invalid` a purge cutoff neverest writes as the store stamps: neither is relayed.
