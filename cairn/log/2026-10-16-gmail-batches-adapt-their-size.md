---
cairn: log
change: gmail-batches-adapt-their-size
date: 2026-10-16
---

# Gmail batches adapt their size

A Gmail batch now carries a size that adapts after lieer's (`lieer/remote.py`, `get_messages`), within the measured `gmail.batch-size`, which becomes the ceiling. Gmail's concurrency limit is shared by every API client of the user, so the size that drew no refusal on a quiet account can draw them on a busy one.

## What landed

- src/gmail/batch.rs: `BatchSize`. A batch with a read answered `Inner::Busy` (refused for concurrency or rate, failed transiently, or missing from the answer) cuts the size to half the reads it carried, never raising it and never below one read; a full batch sent at the size in force and answered with no refusal doubles it, never above the ceiling; a per-minute quota refusal leaves it alone, unlike lieer, since the pacing charges that quota by the read. `read_all` takes the size shared.
- src/gmail/run.rs: `GmailRun::batch_size`, started at `gmail.batch-size` and shared by the source's connections for the run; src/gmail/client.rs passes it.
- Adapting from the size a batch was sent at keeps the shared size sound with several connections: batches built before a refusal and answered after it cut it to half their own at most instead of halving it once each, and only a batch sent at the size in force grows it. lieer, serial, never meets this.
- src/config.rs, config.sample.toml: `gmail.batch-size` documented as the ceiling.
- Tests on the fake batcher: halving to one and doubling back, adapting from the size sent at, refusals shrinking the batches until one lands (10, 5, 2, 1, 2, 3 for twenty reads, the first refused three times), a minute refusal leaving a reduced size alone, a batch with both refusals halving, two refusals in one batch halving it once, a batch cut short by its bytes not growing the size and, refused, halving from what it carried, the size carried to the next read, and, through a fake second connection acting on the shared size mid-batch, a concurrent cut neither halved again nor undone by a late landing. On the scripted client, a refused body read cuts the size the run holds, and the next read starts from it. The existing ten-a-batch test now expects 10, 5, 10, 10, 10, 1. Twelve mutations, one per rule (a fresh size per read, and each read starting at the ceiling, among them), each made a test fail.

## Left

- A live run on a busy account, counting the sizes the batches settle on.
- Adapting `gmail.batches-in-flight`, and raising the default ceiling, both described as out of scope in the proposal.

## Capabilities moved

**sync**: Gmail reads in paced batches (modified: the batch size adapts).
