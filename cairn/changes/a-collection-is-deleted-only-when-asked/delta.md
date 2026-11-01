---
cairn: change
change: a-collection-is-deleted-only-when-asked
---

# Delta

## ADDED Requirements

### Requirement: A collection listing names every collection the source holds
A source's collection listing SHALL name every collection it holds, whatever the provider leaves out by default: hidden Google calendars, every page and every depth of Graph contact folders. A collection left out of a listing reads as deleted where the store knows both endpoints held it.

## MODIFIED Requirements

### Requirement: A collection deleted on one endpoint is deleted on the other
A collection one endpoint of a pair lists and the other does not SHALL be read three ways, the store being the base: where the store knows both endpoints held it (a binding or a checkpoint of each, pimdir SYNC §5), the other endpoint deleted it, and the delete SHALL cross to the one still holding it only when that endpoint's `collection.delete` allows it, which it does not unless configured; otherwise it is new on the endpoint listing it, and SHALL be created on the other, as that endpoint's `collection.create` allows. A delete the remaining endpoint forbids SHALL leave the collection where it is, and SHALL NOT create it again where it was deleted.

Absence is the only signal a collection deletion leaves on IMAP and DAV, and a listing may drop a collection nobody deleted, so a server collection is deleted from an absence only where the user asked for it (pimdir SYNC §5).

Once the delete has crossed, the collection SHALL be dropped from the store with what it holds, the operator having removed it from the account: a collection made again later under its name then reads as new rather than as deleted on the other side. A dry run reports the hunk and changes nothing.

Without a base, a collection deleted on one endpoint was created again from the other on every run, which no deletion could outlast.

#### Scenario: A deleted address book stays on the other endpoint by default
- **GIVEN** an address book both endpoints held, deleted on one of them, the other configured without `collection.delete`
- **WHEN** the account is synced
- **THEN** it stays on the other endpoint, and the next run does not create it again

#### Scenario: A deleted address book does not come back
- **GIVEN** an address book both endpoints held, deleted on one of them, the other configured with `collection.delete = true`
- **WHEN** the account is synced
- **THEN** it is deleted on the other, the report naming the delete, and the next run creates nothing

#### Scenario: An address book made again is new
- **GIVEN** an address book deleted on both endpoints by a crossed delete, then made again on one
- **WHEN** the account is synced
- **THEN** it is created on the other

## REMOVED Requirements

None.
