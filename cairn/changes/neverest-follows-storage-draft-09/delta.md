---
cairn: delta
change: neverest-follows-storage-draft-09
---

# Delta

## ADDED Requirements

## MODIFIED Requirements

### Requirement: A store the format outgrew is refused with its remedy
A store opens under its own format version alone (pimdir STORAGE §4.2, §6): one any earlier neverest wrote, naming no `store_meta.format_version` or an earlier draft, is refused as `store-version` and never migrated. Opening one SHALL fail naming the reinstall in the order that keeps its queued writes, which live only in that store: sync it with the neverest that wrote it until nothing is queued, then `neverest sync --reset --force -a <account>`, which moves it aside and syncs a new one, rather than surfacing the raw refusal alone. The message SHALL never say to delete the store. What the old store held that no server holds (retained items, whatever was still queued) stays in the store moved aside. MIGRATION.md SHALL give the same order.

#### Scenario: A store an earlier neverest wrote
- **GIVEN** a store whose `store_meta.format_version` names `storage-draft-08`
- **WHEN** neverest runs `--json drain` on the account
- **THEN** it exits 1 with `code` `store-version`, the message naming the sync with the previous neverest until nothing is queued and `neverest sync --reset --force -a <account>`, and no delete

## REMOVED Requirements
