---
cairn: change
change: a-newer-store-asks-for-an-upgrade
---

# Delta

## ADDED Requirements

### Requirement: A store a newer neverest wrote asks for an upgrade
Opening a store whose version is above the one this neverest knows SHALL fail saying a newer neverest wrote it and naming the upgrade, never a reset: the store is good, and a reset would drop what it holds that no server has.

#### Scenario: A downgrade
- **GIVEN** a store written by a newer neverest
- **WHEN** an older neverest syncs the account
- **THEN** the run fails asking for a newer neverest, and the store is untouched

## MODIFIED Requirements

None.

## REMOVED Requirements

None.
