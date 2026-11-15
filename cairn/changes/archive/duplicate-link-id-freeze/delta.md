---
cairn: change
change: duplicate-link-id-freeze
---

# Delta

## ADDED Requirements

### Requirement: An ambiguous identity is reported, never judged
An identity the engine marked ambiguous (a collection holding two items with one
link id, two messages with the same `Message-ID`) SHALL appear in the sync
report, text and `--json`, naming its collection and every handle involved, and
SHALL keep appearing on every run until the collection holds the identity once.

Neverest SHALL NOT repair a duplicated collection, and SHALL NOT report it as an
invalid mailbox: RFC 5322 §3.6.4 binds the generator of a `Message-ID` and says
nothing about what a store may hold, so the report states what neverest cannot
tell apart rather than what the user did wrong. Detection, policy and state
belong to the engine and the store; this crate surfaces them and derives no
duplicate rule of its own.

### Requirement: The report accounts for every write the run made
A run that wrote to a remote SHALL report it. `already in sync` SHALL mean the
run wrote nothing, and an append performed by the sync SHALL appear as a hunk,
so a report can be read as the record of what happened rather than a summary
that may omit it.

#### Scenario: A duplicated message is reported and left alone
- GIVEN a collection holding two messages with one `Message-ID`
- WHEN the account is synced
- THEN no hunk is derived for that identity on either side, and the report names the collection and both UIDs

#### Scenario: A resurrected append is not silent
- GIVEN a run that appends a message to a side
- WHEN the report is rendered
- THEN the append appears as a hunk, and the run does not read as already in sync

## MODIFIED Requirements

## REMOVED Requirements
