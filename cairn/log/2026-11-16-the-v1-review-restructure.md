---
cairn: log
change: the-v1-review
date: 2026-11-16
---

# The v1 review, phase 5: restructure

- The configuration is a plain model split by block under src/config/ (no feature gate there); its conversions moved to src/tls.rs, src/proxy.rs, src/sasl.rs, src/server.rs and src/dav/auth.rs, each gated once in src/main.rs.
- src/item/ dissolved into src/collection.rs and src/flag.rs; the three-way merge moved to src/conflict/merge.rs and the event helpers to src/calendar.rs; src/kind/ keeps the kinds and mail.
- src/offline/ folded into src/sync/, and the 15,000-line driver split by phase under src/sync/driver/ (pair, local, spine, hydrate, itemize, relay, reconcile, collections, queue, send, upkeep), its tests in sibling files.
- src/offline/pipe.rs is replaced by `std::io::pipe` in the relay: the buffer is the kernel's (64 KiB on Linux) instead of 256 KiB. A fetch whose append failed now ends with a broken pipe instead of blocking on a full buffer, and a server sending more than the size it stated fails the relay of that item instead of having the excess dropped. The relay live cell is the owner's to run.
- Tests: 647 with every feature (the hand-rolled pipe's two tests went with it), 373 in the minimal build; no assertion changed.
- The files and functions still over the A7 limits are listed in tasks.md with their reason.
- Capability moved: none (no behaviour change but the relay pipe's).
