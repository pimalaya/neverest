---
cairn: change
change: a-source-drains-its-own-namespace
---

# Delta

## ADDED Requirements

### Requirement: A source drains the collections of its own namespace
The pre-sync drain SHALL narrow the store's queued collections to the ones the
draining source's namespace owns, a hub collection id being
`<namespace>/<name>`. The queue is the whole store's and records no source, so
the narrowing cannot come from it.

A source SHALL NOT drain another's collections. Staging an existing item's
action resolves that item's binding for the draining source, and a source
holding no binding for it cannot place the action: at best the drain does
nothing, and the owner it robbed of its turn is the one that could have applied
it. Sources run in name order, so an unnarrowed drain is not an occasional race
but a rule: the first source alphabetically answers for every frontend write on
the account.

The drain SHALL report what it skipped beside what it applied and parked, a
skipped action being one left for another source rather than one done.

#### Scenario: A calendar source leaves the mail queue alone
- GIVEN an account declaring `caldav`, `carddav` and `imap`, with an action
  queued against `imap/INBOX`
- WHEN the run drains, `caldav` sorting first
- THEN `caldav` drains nothing and `imap` applies the action
