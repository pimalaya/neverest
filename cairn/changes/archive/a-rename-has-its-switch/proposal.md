---
cairn: change
id: a-rename-has-its-switch
status: landed
created: 2026-11-15
---

# A rename has its switch

## Why

Live testing of 2026-11-15 (neverest/cairn/spec/testing/fastmail.md; ledger line N14, owner decision D14): a one-way source declared `collection.rename: full` in `neverest check` while every other write was `none`. The spec said "a one-way source SHALL support no write"; src/offline/capability.rs let the collection intents through on a one-way source on purpose, and unlike create and delete a rename had no configuration switch, so nothing stopped a frontend renaming a folder on a pull-only server. The owner ruled on 2026-11-15: "go with the collection.rename switch".

## What

- `collection.rename` beside `collection.create` and `collection.delete` on every backend table, optional: unset, it reads the account's `one-way`, off on a one-way account and on otherwise; set, it decides whatever the account.
- Off, a sync and a check declare `collection.rename: none`, the detail naming the key (`one-way copy: collection.rename is off unless set to true on the source`, or `collection.rename is disabled`), so a frontend's rename is refused at enqueue and the owner's backstop parks one queued before.
- The spec: a one-way source supports no item or flag write; a collection action happens only where its switch allows it; the scheduling intents stay allowed.

## Decisions

- **The default reads `one-way`** rather than a fixed value: a two-way account renamed folders before and still does; a one-way account, whose source the copy treats as authoritative, renames none unless asked. `CollectionPermissions::renames(one_way)` is the one reading.
- **Additive key**: no key renamed or removed; allowed during live testing.
