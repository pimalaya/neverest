---
cairn: change
id: every-key-is-printed
status: landed
created: 2026-11-03
---

# Every key is printed

## Why

pimalaya-cli's every-output-is-data review of 2026-11-03 left neverest two rules it broke, the rest of its ledger line beside the sync events:

- **`mode` was prose.** `check` and `init` printed `mode` as `AccountMode`'s sentence (`"imap and maildir sync both ways, keeping a local copy"`): a caller could not read the direction or the endpoints back out of it.
- **Absent keys were left out.** 33 `skip_serializing_if` on printed types: 18 lists of the sync report vanished when empty (`conflicts`, `parked`, `refused`, `unreached`, …), with `purged`, `index` and the optional fields of its rows, a drain's `overruled`, `index` and `seq`, a check's `role`, `takesTasks`, `detail`, and `default` when false. A caller wrote `.conflicts // []`, and a schema could not mark the keys required.

## What

- `mode` prints `AccountMode` as data: `{sources, targets, oneWay, retain}`. Its `Display`, the sentence, stays for text mode.
- Every `skip_serializing_if` on a printed type goes: a list prints `[]` when empty, an absent value `null`, a bool `false`. The schemas mark those keys required.
- Persisted state (src/offline/state.rs) keeps its omissions: it is a file neverest reads back, not output.

## Compatibility

Breaking for a caller testing a key's presence for content (`has("conflicts")`); one reading `.conflicts // []` or `.conflicts | length` reads the same. Lands in the next release with the sync events.
