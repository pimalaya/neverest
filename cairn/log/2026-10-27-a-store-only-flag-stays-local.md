---
cairn: log
change: a-store-only-flag-stays-local
date: 2026-10-27
---

# A store-only flag stays local

A flag `pimdir tag` kept in the store only on a card or a calendar item was pushed on every run, rejected by the backend (`store_flags` bails for every kind but mail) and warned about each time. `SourceCtx::push_rights` now delegates to `push_rights(perms, media_type)`, which withholds the flag right for a kind other than `message/rfc822`, read through the new `Pool::media_type`; the engine keeps such a flag local (pimdir SYNC: a forbidden kind stays pending, nothing pushed).

- **sync**: added "A flag on a kind other than mail stays in the store", guarded by src/offline/driver.rs `a_store_only_flag_on_a_card_stays_local`, which fails with the full rights the source had before.
