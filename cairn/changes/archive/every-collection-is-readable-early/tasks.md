---
cairn: tasks
change: every-collection-is-readable-early
---

# Tasks

- [x] `offline`: a `Pager` answers `Rest::Pause` at rest; `run_verb_paged` hands the coroutine back (`Paged::Paused`), its partial report read.
- [x] `offline/driver`: collections ordered by role then name (`run_order`), roles read before the filter declares them.
- [x] `offline/driver`: per-backend budget (`connection_budget`: IMAP `connections`, Graph `pool-size` or 3, others 1).
- [x] `offline/driver`: phase 1 as one work scheduler (`Work`): primed batches, first pass, fill once the first pass and its primed bodies are over, other batches; ties by collection rank.
- [x] `offline/driver`: a paused spine itemizes what it pulled and carries what it owes to the spine that fills it; no push, staged work or coverage before.
- [x] `offline/driver`: phase 2 batches the N newest of each collection first (`hydrate_batches`).
- [x] `offline/driver`: `--bodies listed` keeps the landed handles (`Listed`) and downloads those alone, in both phases.
- [x] `throttle`, `account`, `msgraph`: `--max-wait` resolved into each source's throttle, read by Graph's batch rounds.
- [x] `cli/sync`: `--prime`, `--bodies`, `--max-wait`; `Pacing` with the prime default.
- [x] Rebased on the Graph, Gmail and intents changes: `--max-wait` reaches Graph's per-get batch waits and Gmail's pauses (`waits_in_process`); pacing flags refused beside `--intents-only` and `--declare-only`; Gmail kept at one connection.
- [x] Fake Graph (several connections over the session connector): three requests side by side and never four, each body asked once; a ten-minute throttle on one connection stops every connection; `--max-wait` bounds a batch's stated wait.
- [x] Unit tests: pause stands where a kill would; first pages before any second page (one and two workers); tie by rank; phase 2 tiers; role order; budgets; prime default; listed bodies; max wait through the account and the throttle.
- [x] Stalwart: three folders of 1,100 mails over one connection, killed once primed, each folder's 50 newest readable, every first page before any second, the rerun closing every round with each message once; a primed run reporting each fetch once over one and four connections, with timings; a move synced by runs naming its source alone then its target alone, and the other way round.
- [x] Spec folded, log written, CHANGELOG, sample configuration.
- [x] Verify: `cargo test`, the ignored Stalwart suites touched, `cargo clippy` (default, `--all-features`, MOA's set).
