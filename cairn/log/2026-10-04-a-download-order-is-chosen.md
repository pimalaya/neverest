---
cairn: log
change: a-download-order-is-chosen
date: 2026-10-04
---

# A download order is chosen

MOA's staged first sync makes the Inbox readable first, but within a large Inbox the bodies still came largest first, so today's mails waited behind years of large attachments. Largest first stays the fastest whole run, so the caller now chooses.

## What landed

- src/cli/sync.rs: `--download-order <largest|newest>`, `largest` by default.
- src/offline/driver.rs: each body to hydrate carries its size and its date from the mail summary (`HydrateTarget`); `hydrate_batches` sorts each collection's bodies and the batches across the account with one comparison (`DownloadOrder::compare`), a batch standing for its first body. Under `newest`, a body without a date comes last.
- Unit tests: both orders over two collections, a dateless mail last; newest first interleaving collections batch by batch.
- tests/hydration.rs on Stalwart: over one connection, `--download-order newest` applies a recent folder of small mails before an older folder of large ones, then the next run completes both.

## Capabilities moved

- sync: the caller chooses the download order.
