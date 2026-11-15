---
cairn: change
id: a-newer-store-asks-for-an-upgrade
status: landed
created: 2026-10-30
---

# A newer store asks for an upgrade

## Why

Pimdir stable-release review (2026-10-30), blocker B4. `open_store` (src/offline/driver.rs:211-223) maps io-pimdir's `PimdirError::Version` to the stale store's text, "The replica store predates this neverest; drop it with `neverest sync --reset`". `Version` is raised for a store above the version this io-pimdir knows (`user_version > VERSION`), written by a newer neverest: a user who downgraded is told to destroy a good store.

## What

- `PimdirError::Version` gets its own message: the store was written by a newer neverest, upgrade neverest to sync it; no reset is named.
- `PimdirError::Stale` keeps the reset remedy, `sync --reset --force` (a-reset-keeps-pending-work).

## Not done

- Error codes from io-pimdir's `PimdirError::code()`: later, once io-pimdir has it (pimdir-errors-carry-their-code).
