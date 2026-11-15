---
cairn: change
id: the-report-shapes-read-as-the-family
status: landed
created: 2026-11-15
---

# The report shapes read as the family

## Why

The pre-major round of the CLI alignment of 2026-11-15 (pimalaya/.github cairn/changes/cli-alignment-2026-11-15, line X10) takes the JSON shape review's open neverest lines (reports/schema-review-2026-11-14.md, status in reports/schema-review-status-2026-11-15.md):

- A17: `conflict list`, `show` and `resolve` printed the item's public id as an integer, where himalaya, cardamum and calendula print a store id as a string.
- B9 (part): `init`'s account row kept the whole chain in `error` with no causes apart, and its `sources` key named the endpoints it opened, clashing with the failure object's `sources`.
- B10: `conflict resolve`'s `partial` was a list of sentences the binary composed.
- B11: a sync report wrapped each patch in a single-key object (`collection: {patch}`, `item: {patch}`), schemars naming the item ones `PatchOutcome2` and `PatchEntry2`; `report.account` repeated the account row's `account`.

neverest is 0.x: the breaks go in CHANGELOG and MIGRATION.md.

## What

- `ConflictSummary.id` and both `ConflictResolveOutput` ids are strings; the commands still parse the id they take as before.
- `conflict resolve`'s `partial` is `[{capability, source, detail}]` (`PartialCapability`), read off io-pimdir's `PimdirPartial`, `source` named as the configuration names it.
- A sync report holds `collectionPatch` and `itemPatch`, arrays of entries named `CollectionHunkEntry` and `ItemHunkEntry` in the schema; `PatchOutcome` is gone; `SyncOutput.account` stays in the code for the text form and is not serialized.
- `InitOutput` carries `code`, `error` (the outermost message) and `sources` (its causes); the endpoints it opened are `opened`, those opened before a failure included.

## Decisions

- **`partial` keeps `source`.** The review suggested `{capability, detail}`; an edit may rely on the partial support of more than one source, so the row names which, as the store's row does.
- **`opened` rather than dropping the list.** On success it is the mode's sources and targets; a failure now fills it with the endpoints reached before the one that failed, which the mode does not say.
- **`collectionPatch` and `itemPatch`** rather than `collections` and `items`, which read as listings of collections and items (the review's B2 collection row).
