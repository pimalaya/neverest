---
cairn: log
change: a-refused-append-names-its-item
landed: 2026-09-02
---

# A refused append names its item and takes its copy back

Syncing into a calendar iCloud serves read-only, which answers every create with HTTP 403, printed this:

```
icloud refused the append of pimalaya-releasehub in 0B43E653-…: HTTP 403
Account icloudcal synchronized: 3 hunks
```

The item is `pimalaya-release`, and the run wrote nothing.

## One handle, two symptoms

The hub stages an append under a handle of its own making, the item's link id with `\u{1}hub` appended, the item having no handle on the side it is being created on (io-replica, `created_placement`). The rejected-write path printed it raw, and the control character is invisible in a terminal, so the marker read as part of the name.

The same handle is why the copy stayed in the patch. A rejected write takes back the hunk it came from by matching side, collection and handle, and `names` answered `false` for every `Copy` because a copy has no handle on the side it is being created on. That is true, and it is named by the link id instead, which is what the staged handle carries. So a run whose appends a server refused one and all still counted them as applied, which is the phantom write the refusal reporting exists to prevent.

## What landed

**[itemize_rejected](../../src/offline/driver.rs) reads the item's own name** out of the staged handle, taking what precedes the marker, and uses it both for the warning and for the retraction.

**`names` matches a `Copy`** by its `target_side` and its `source_id`, so a refused append takes its copy back exactly as a refused update takes its update back. A `Fetch` still matches nothing: it reaches no server and is never rejected.

## Verification

Against the same read-only iCloud calendar: the run reads `already in sync (4 warnings)`, exits 2, names each item plainly, and its `--json` carries an empty `item.patch` beside four `rejected` entries. `a_refused_append_is_named_by_its_link_id_and_its_copy_is_taken_back` pins both halves, including that the marker never reaches the rendered text.

## Capabilities moved

- sync: a refused write names the item as a person knows it, never an engine-minted handle
- sync: a refused append takes its copy hunk back, so a run that wrote nothing reports nothing applied
