---
cairn: delta
change: a-capped-source-says-so
---

# Delta

## ADDED Requirements

### Requirement: A capped source says so, and keeps what it leaves unlisted
A source whose server lists at most a number of the items a collection holds SHALL be declared by `<backend>.collection.capped = true`, false by default. Every page of every round of its collections SHALL state `capped` (pimdir SYNC §4), and no delta nor head SHALL. A round closing on a capped last page finds no member absent, and only a removal the source states drops one. The report SHALL list under `unlisted`, per collection, the count of based bindings the round did not list, printed under `Unlisted`, no warning and no exit code. A rekey of a capped collection is guarded by its own park alone (pimdir SYNC §8).

#### Scenario: Ten messages past the cap
- **GIVEN** an IMAP source set `collection.capped = true`, 25 messages of its inbox bound, and a server listing the 15 newest
- **WHEN** the account syncs and the round closes
- **THEN** the 25 stay bound, and the report names 10 unlisted in INBOX with no warning, where the same listing of a source not capped drops the 10

## MODIFIED Requirements

## REMOVED Requirements
