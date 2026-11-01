---
cairn: change
change: a-one-way-overwrite-keeps-what-it-discards
---

# Delta

## ADDED Requirements

### Requirement: A one-way overwrite keeps the body it discards
A one-way account overwriting a target's own edit SHALL keep the target's body as a retained item and SHALL report it under `overruled`: `one-way` discards the target's change from its server, never from the store. A target body the store holds as wanted only SHALL be fetched before the overwrite settles it, the store refusing to overrule a body nobody read (`conflict-unread`); one still unread after its fetch SHALL stay parked, never failing the run.

#### Scenario: A target edited while the source moved
- **GIVEN** a one-way account whose source and target agreed on a card, the source then changing it and the target editing it
- **WHEN** the account is synced
- **THEN** the target holds the source's card, the store keeps the target's edit as a retained item, and `overruled` names it

#### Scenario: A target edit listed without its body
- **GIVEN** the same account, the target listing its edited card with no body
- **WHEN** the account is synced
- **THEN** the target's card is fetched first, then kept as a retained item and named under `overruled`, and the run does not fail

## MODIFIED Requirements

None.

## REMOVED Requirements

None.
