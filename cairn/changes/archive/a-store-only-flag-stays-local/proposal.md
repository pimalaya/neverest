---
cairn: change
id: a-store-only-flag-stays-local
status: landed
created: 2026-10-27
---

# A store-only flag stays local

## Why

io-pimdir's `pimdir tag` lets a flag land on a card or a calendar item and reports it "kept in the store only: its source carries no flags for this kind". The engine still sees the placement's flags differ from its base and derives a `SetFlags` push; DAV, Google People, Google Calendar and Graph contacts or calendars reject it (`store_flags` bails), so `RemoteSource::reject` warned on every run and the change stayed pending.

## What

`SourceCtx::push_rights` withholds the flag right for a source whose primary connection syncs a kind other than `message/rfc822` (`Pool::media_type`). The engine then keeps the flag local, pushing and warning nothing.
