---
cairn: log
change: every-store-outcome-is-reported
date: 2026-10-30
---

# Every store outcome is reported

Phase A part 2b of the pimdir stable-release plan: the port to io-pimdir wave 3c (path pin unchanged), beside neverest-builds-the-search-index and a-one-way-overwrite-keeps-what-it-discards.

## What landed

- src/sync/report.rs: `OverruledBody` with an optional `side` and `id` and a new `action`, the queue row a drain applied; `UnansweredPushes` and `unanswered`, counted by `incomplete` (exit 3); `IndexRefresh` and `index` (neverest-builds-the-search-index).
- src/offline/driver.rs: `drain_queues` reports `PimdirDrainReport::overruled`; `Noted` keeps the last pass's `unanswered` per collection; `sync_side` reports under `rejected`, as a `create`, a create of `rejected_creates` no rejection or refusal of the connector names (`unnamed_creates`); `keep_store_bodies` reads its mutation's `Overruled` events into the side's notes.
- src/cli/drain.rs: `overruled` and `index` in the output.
- src/conflict.rs, src/cli/conflict.rs, src/conflict/report.rs: `conflict resolve` stages through `enqueue_with_body_checked`, `Applied::Resolved` carrying the partial supports, printed and under `partial`.
- src/error.rs: `store-foreign` and `index-fts5-missing` relayed. Not relayed: `index-busy` (`neverest index` answers it as `busy`), `index-missing`, `index-outdated` and `index-query-only` (a query client's; neverest only opens the index as its indexer), `cutoff-invalid` (neverest writes its cutoff as the store stamps; the sweep tests over a real store pass the check).
- `PimdirBlobs::reader` answering `Option<PimdirBlobReader>` needed no change: part 2a ported it. A body failing its name is withheld by the engine's verification before any push (`WantsVerify`); a connector reading a known length never reaches the end-of-stream read that would fail it, so a body corrupted between the verification and the push is not caught there (io-pimdir's reader checks at the end only).

## Exit codes

- `unanswered`: 3. The push stays pending and the next run pushes it again: a rerun clears it, the incomplete code's class.
- `rejected_creates`: 2, as every rejected write, through `rejected` or `refused`: a rerun meets the same refusal.
- A drain's `overruled`: none, a warning, as a sync's: the policy decided as configured.

## Evidence

On the old code `a_body_the_drain_kept_aside_is_reported` found no `overruled` (`left: 0, right: 1`), and `a_database_another_program_wrote_fails_as_foreign` printed `{"error":"Open caldav store","sources":["Pimdir store database is not a pimdir store: another program wrote it"]}` with no `code`.

## Capabilities moved

- sync: a push no server answered is pushed again, a rejected create is reported once, a resolution names what the push supports in part (added); a body a policy overruled is kept and reported, a store error carries its code, an incomplete run has its own exit code (modified).
