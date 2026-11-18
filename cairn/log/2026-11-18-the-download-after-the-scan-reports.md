---
cairn: log
change: the-download-after-the-scan-reports
date: 2026-11-18
---

# The download after the scan reports

The owner's first run on their own IMAP account (2026-11-18, about 2 GB, 2m37s) printed `Scanning imap (16/16)` and then nothing until the result. Over several connections the free ones download each landed page's bodies during phase 1, and those downloads ticked no progress: phase 2, the only one printing `Downloading`, found nothing owed and printed no line.

## What landed

- src/sync/driver/spine.rs: phase 1's landing counts each body it settles (`Tally`), and `Work` counts the bodies it was fed. When the last collection is listed over several connections with bodies not yet settled, the run prints `download-progressed` from 0 over those left, then once per percent to the end. Phase 2 keeps its own count for what it still owes. A single connection queues only the primed bodies in phase 1 and prints as before.
- tests/hydration.rs: `the_download_after_the_scan_prints_its_progress`, two folders of 1,100 messages over `-j 4` against the local Stalwart, text mode. It failed on the code before the fix with `Scanning imap (2/2)` as the last line; it passes after. The suite passes serially (`--test-threads=1`); in parallel its older tests fail on their own seeding (`seed append failed`, an expunge outrun), before and after the fix.

A batch that fails in phase 1 is warned about and never settles, so its download line stops short of its end; phase 2 owes those bodies again and prints a count of its own.

## Spec

- [sync](../spec/sync.md): the two-phase progress line and *A sync prints its progress as events* (a download starting as the scan ends, scenario *A first run over several connections reports its download*).
