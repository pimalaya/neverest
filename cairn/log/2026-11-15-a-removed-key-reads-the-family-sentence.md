---
cairn: log
change: a-removed-key-reads-the-family-sentence
date: 2026-11-15
---

# A removed key reads the family sentence

Audit finding F41 of the CLI alignment round of 2026-11-15.

## What landed

- src/config.rs: every removed key declared by `pimalaya_config::removed_key!`, one type a key (`LeftRemoved`, `RightRemoved`, `AccountCollectionRemoved`, `FolderRemoved`, `EnvelopeRemoved`, `NamespaceRemoved`, `StoreRetentionRemoved`, `StoreHydrationRemoved`, `SmtpLoginRemoved`, `SmtpPasswordRemoved`); the local macro and `MIGRATION_NOTES` gone.
- Tests read "The `x` key is removed, …, see MIGRATION.md".

## Capabilities moved

- [sync](../spec/sync.md): *The configuration refuses every key it does not read* amended.
