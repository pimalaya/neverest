---
cairn: tasks
change: gmail-batches-adapt-their-size
---

# Tasks

- [x] `src/gmail/batch.rs`: `BatchSize`, cut to half the reads a batch carried when it holds a busy read (concurrency, rate, transient failure, missing answer), doubled by a full batch with no refusal sent at the size in force, left alone by a per-minute quota refusal; `read_all` takes it shared.
- [x] `src/gmail/run.rs`: `GmailRun::batch_size`, started at `gmail.batch-size`; `src/gmail/client.rs` passes it.
- [x] Tests on the fake batcher: halving to one and doubling back to the ceiling, adapting from the size a batch was sent at, refusals shrinking the batches until one lands, a minute refusal leaving a reduced size alone, a batch with both refusals halving, two refusals in one batch halving it once, a batch cut short by its bytes not growing the size, the size carried to the next read of the run, another connection's cut meanwhile neither halved again nor undone, a refused batch cut short by its bytes halving from what it carried; on the scripted client, a refusal cutting the size the run shares; the existing ten-a-batch test updated to the halved second batch.
- [x] `gmail.batch-size` documented as the ceiling, in the config docs and config.sample.toml.
- [x] `cargo test`, clippy, fmt.
- [x] Spec folded, log written, CHANGELOG.
- [x] Live: a first sync of a busy account, counting the batch sizes it settles on (2026-10-19, see the log).
