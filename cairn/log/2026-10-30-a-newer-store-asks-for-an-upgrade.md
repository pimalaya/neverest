---
cairn: log
change: a-newer-store-asks-for-an-upgrade
date: 2026-10-30
---

# A newer store asks for an upgrade

## What landed

- src/offline/driver.rs: `open_store` answers `PimdirError::Version` (a store above the version this io-pimdir knows) with "The store was written by a newer neverest; upgrade neverest to sync this account", no reset named; `Stale` keeps a reset remedy, now `--reset --force`.
- Test: `a_store_a_newer_neverest_wrote_asks_for_an_upgrade`, which on the old code read "The replica store predates this neverest; drop it with `neverest sync --reset -a imap`".

## Capabilities moved

- sync: a store a newer neverest wrote asks for an upgrade (added).
