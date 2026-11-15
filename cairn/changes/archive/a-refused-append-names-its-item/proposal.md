---
cairn: change
id: a-refused-append-names-its-item
status: landed
created: 2026-09-02
---

# A refused append names its item and takes its copy back

## Why

Syncing a calendar iCloud serves read-only, which answers every create with HTTP 403, produced this:

```
icloud refused the append of pimalaya-releasehub in 0B43E653-…: HTTP 403
Account icloudcal synchronized: 3 hunks
```

Two things are wrong with those two lines. The item is `pimalaya-release`, and the run wrote nothing at all.

The hub stages an append under a handle of its own making, the item's link id with `\u{1}hub` appended, the item having no handle on the side it is being created on (io-replica, `created_placement`). The rejected-write path printed that handle raw, and the control character is invisible in a terminal, so the marker reads as part of the name.

The same handle is why the copy stayed in the patch. A rejected write takes back the hunk it was derived from by matching side, collection and handle, and `names` answered `false` for every `Copy`, on the grounds that a copy has no handle on the side it is being created on. True, and it is named by the link id instead, which is exactly what the synthetic handle carries. So a run every one of whose appends the server refused still counted them all as applied, which is the phantom-write shape the refusal reporting exists to prevent.

## What changes

`itemize_rejected` reads the item's own name out of the staged handle, taking everything before the marker, and uses it for the report and for the retraction. `names` matches a `Copy` by its `target_side` and its `source_id`, which is that same link id, so a refused append takes its copy back exactly as a refused update takes its update back. A `Fetch` still answers `false`: it reaches no server and is never rejected.

## Verification

Against the same read-only iCloud calendar, the run now reads `already in sync (4 warnings)`, exits 2, names each item as `pimalaya-release`, `pimalaya-standup` and the rest, and its `--json` `item.patch` is empty while `rejected` carries the four refusals.

## Out of scope

No CHANGELOG entry: the defect only ever existed in code that has not shipped, so it is interior churn to the 1.0.0 section. The section's existing claim, that a write a remote rejected takes back the hunk it was derived from, is what this makes true for an append.
