---
cairn: change
id: gmail-batches-adapt-their-size
status: landed
created: 2026-10-16
---

# Gmail batches adapt their size

## Why

`gmail-reads-in-paced-batches` fixed the batch at `gmail.batch-size` reads, 10 by default, chosen by measurement: on the test account, batches of 10 sent one after another drew no refusal in 2,250 reads, 25 drew 1.2 %, 50 drew 19 %. Gmail's concurrency limit is per user, though, and not every account sits alone on it. Google documents it as shared by every Gmail API client accessing the user: a phone's mail app on the API, or lieer during a migration, shares it, so the size that drew no refusal on a quiet account can draw them on a busy one. A refused read is sent again in the next batch of the same size, which meets the same limit.

lieer, which has synced Gmail into notmuch for years, adapts instead (`lieer/remote.py`, `get_messages`): a batch refused for the user's rate (403 or 429) halves the next, down to one read, and every batch answered whole doubles it back, up to its ceiling of 50.

## What

- **`BatchSize`** (`src/gmail/batch.rs`): the reads the next batch carries. It starts at `gmail.batch-size`, which becomes its ceiling. A batch with a read answered `Inner::Busy` (refused for concurrency or rate, failed transiently, or missing from the answer, as lieer also halves on a failed batch) cuts it to half the reads that batch carried, never raising it and never below one read; a full batch with no refusal at all, sent at the size in force, doubles it back, never above the ceiling. Unlike lieer, a per-minute quota refusal leaves it alone: the pacing charges that quota by the read, so a smaller batch would only slow the run, and the minute's waits already answer it. A daily quota still gives the source up.
- **Shared for the run** (`GmailRun::batch_size`): every connection of the source and every read of the run take and adapt the one size, and a later page starts where the last one left it. A batch adapts from the size it was sent at: batches built before a refusal and answered after it cut the size to half their own at most instead of halving it once each, and only a full batch sent at the size in force grows it, so a late landing does not undo a fresh halving, and a raw batch cut short by its bytes proves nothing. lieer, serial, never meets this.
- `read_all` takes the shared size instead of a number; nothing else of its pacing, waits, retries or raw size caps moves.

## Not in scope

- Raising the default: `gmail.batch-size` stays 10, now a ceiling. A user may set it higher and let refusals find the size, but the default keeps the measured value until a live run says otherwise.
- `gmail.batches-in-flight` stays fixed: one batch in flight is the default, and adapting concurrency across connections is a separate question.
- A live measurement: no Gmail account here; the fake batcher covers the arithmetic.
