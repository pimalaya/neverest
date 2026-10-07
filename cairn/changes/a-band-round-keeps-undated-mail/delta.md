---
cairn: change
change: a-band-round-keeps-undated-mail
---

# Delta

## ADDED Requirements

## MODIFIED Requirements

### Requirement: A mail sync lists within a scope on the `Date`
An account MAY bound its mail by `item.filter.since`, and a run by `sync --since`, which overrides it: a duration back from today (`30d`, `12w`, `6mo`, `1y`), taken to the start of its UTC day so the runs of one day list one scope, a date at midnight UTC, or an RFC 3339 instant. A mail collection SHALL then sync under the scope from that floor (pimdir SYNC §5): a listed message whose `Date` lies below it SHALL be left out of the page, a message with no usable `Date` SHALL be in every scope, and an explicit removal SHALL apply whatever the date. What a scope leaves out SHALL NOT be deleted, neither in the store nor on the server. A round over the band a coverage lacks SHALL infer no delete of a member with no usable `Date` it did not list, the provider's date filter (a garbled `Date` to IMAP `SENTSINCE`, Gmail's `after:` on the arrival, Graph's `sentDateTime`) being free to miss it (io-pimdir `ff28408`, pimdir SYNC §5). A scope SHALL be refused, by the key or flag it came from, when an endpoint the run syncs holds contacts or calendars.

#### Scenario: An old message received today
- **GIVEN** a mailbox holding, all appended today, a message dated 2020, one dated in the future and one with no `Date`
- **WHEN** it is synced under `item.filter.since = "30d"`
- **THEN** the store holds the future and the undated messages and not the one dated 2020

#### Scenario: A widening keeps the undated mail
- **GIVEN** a mailbox synced under `--since 2026-09-07`, holding a message with no `Date` and one whose `Date` is garbled
- **WHEN** it is synced under `--since 2026-01-01`, the band listing neither
- **THEN** both stay in the store, beside the band's messages

#### Scenario: A narrower scope deletes nothing
- **GIVEN** that mailbox synced with `--since 2019-01-01`, then under `30d` again
- **WHEN** the run ends
- **THEN** the message dated 2020 is still in the store and on the server, and once deleted on the server the next run removes it from the store

### Requirement: A Graph mail folder keeps one delta link over the whole folder
(Only the band round's last sentence changes.) A round over the band a coverage lacks SHALL list that band alone and keep the link; the messages with no date, which a `sentDateTime` filter never lists, SHALL stay bound, the band round inferring no delete of them.

## REMOVED Requirements
