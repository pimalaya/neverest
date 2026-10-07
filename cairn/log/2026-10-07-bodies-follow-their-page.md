---
cairn: log
change: bodies-follow-their-page
date: 2026-10-07
---

# A landed page's bodies download while the next pages list

Measured on MOA's Microsoft 365 test box after `scoped-mail-sync`: headers came page by page, but the bodies waited for every listing to end, the first readable after 17 to 37 s. They now follow the pages.

## What landed

- Cargo.toml: io-pimdir `f9b13f8` (an empty mail date stored as no date).
- src/offline/mod.rs: `run_verb_paged` and the `Pager` trait: told each time a page has landed, at a point the store is at rest, once more at completion for the last page, a resumed round flagged before its first page; the listing's waits on the server bracketed by `listing` and `listed`.
- src/offline/driver.rs: phase 1 gives each collection a gate its spine holds but while it waits on the server (`PageFeed`), and a `HydrateFeed` its landed pages fill with their bodiless members, batched in the download order; workers with no collection left take the feed's best batch, fetch it on their connection and apply it under the gate and one apply lock. Phase 2's targets are read after phase 1 (`hydrate_plans`, `bodiless`). `itemize_fetches` also names what was bodiless before the pull. With one connection nothing is fed.
- Tests: three unit tests (a landed page told at rest keeps its bodies through later pages; a resumed round feeds what its earlier pages owe, newest first; the feed waits for the spines); tests/scope.rs (Stalwart): 1,600 messages, bodies readable while the round is open, a kill keeps pages and bodies, the next run fetches exactly the rest.
- Measured on the local Stalwart, 1,600 mails over four connections, `--download-order newest`: first body readable after 0.45 s instead of 1.58 s (the round used to close at 1.24 s, before any body), whole folder with bodies after 2.38 s instead of 2.68 s.

## Capabilities moved

- sync: a landed page's bodies download while the next pages list (added); the one-source sync's two phases, the download order and concurrent hydration (modified).
