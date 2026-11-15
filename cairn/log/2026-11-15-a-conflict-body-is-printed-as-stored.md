---
cairn: log
change: a-conflict-body-is-printed-as-stored
date: 2026-11-15
---

# A conflict body is printed as stored

Finding A16 of the 2026-11-14 JSON shape review: `conflict show` built `base`, `local` and `remote` with `String::from_utf8_lossy` (src/conflict/report.rs), so a body that is not UTF-8 (a vCard 2.1 in Latin-1) printed with U+FFFD in place of the bytes a decision picks between.

## What landed

- `ConflictBody`, untagged: the text when the stored bytes are UTF-8, else pimalaya-cli's `Bytes` (`{data, size, mime}`), `mime` the collection's media type. The text form shows such a body with its replacements, under a line saying so.
- Two tests on the JSON value, the Latin-1 one failing to build against the old type.

## Capabilities moved

- [sync](../spec/sync.md): *A conflict body is printed as stored* added.
