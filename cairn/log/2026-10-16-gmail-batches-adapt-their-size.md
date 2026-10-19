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

## Live run (2026-10-19)

First syncs of the Workspace test account (473 messages, 892 hunks), one-way into a fresh store with every Gmail write off, this change against 19cb351. The test account being quiet, a busy one was stood in for by a ceiling of 50 (19 % refused when measured fixed) and by two batches in flight.

| settings | refused reads (adaptive / fixed) | seconds (adaptive / fixed) |
|---|---|---|
| `batch-size = 50`, 3 runs | 71, 69, 55 / 51, 114, 87 | 45, 49, 51 / 47, 53, 46 |
| `batch-size = 50`, `batches-in-flight = 2` | 41 / 102 | 37 / 51 |
| default (`batch-size = 10`) | 0 / 0 | 33 / 30 |

The size oscillates between the ceiling and about half of it (50 to 25, or about 21 from a batch cut short by its bytes, then doubled back): doubling straight back to the ceiling after one clean batch meets the limit again, which is where the remaining refusals come from. Refusals fall by about a quarter at 50 and by more than half with two batches in flight; at the default nothing is refused and the size never moves.

## Left

- Adapting `gmail.batches-in-flight`, and raising the default ceiling, both described as out of scope in the proposal.

## Capabilities moved

**sync**: Gmail reads in paced batches (modified: the batch size adapts).
