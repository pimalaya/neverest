---
cairn: log
change: a-download-reads-as-it-goes
date: 2026-11-18
---

# A download reads as it goes

The owner's second first run on their own IMAP account (2026-11-18) printed `Downloading imap (0/8433)` and nothing more for minutes: text mode printed a download's first and last count alone, which reads as a hang. The owner's ruling the same day: print it once per percent, as `--json` does.

## What landed

- src/sync/event.rs: `SyncEvent::is_coarse` leaves out `collection-progressed` alone, so every `download-progressed` the percent gate passes prints, at most 101 lines a count. A pair's per-collection count stays out, many collections each printing 101 lines.
- src/sync/driver/gmail_bodies.rs: `a_download_prints_its_progress_as_events` expects the middle count in text mode too.
- CHANGELOG.md: the Fixed line, and the Changed line still saying text goes to stderr, which the stdout entry had replaced.

## Spec

- [sync](../spec/sync.md): *A sync prints its progress as events*, text mode leaving out `collection-progressed` alone.
