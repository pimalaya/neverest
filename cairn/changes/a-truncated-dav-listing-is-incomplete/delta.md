---
cairn: change
change: a-truncated-dav-listing-is-incomplete
---

# Delta

## ADDED Requirements

### Requirement: A truncated `sync-collection` listing is incomplete
A `sync-collection` answer the server truncates (RFC 6578 §3.6, a `507` row) SHALL be followed from the sync token it carries until an answer is not truncated, a bounded number of times. A listing whose last answer was still truncated, the bound reached or the answer carrying no token, SHALL be returned incomplete, read as a delta: what it lists lands and no member is inferred removed from its absence. A truncated answer without a token SHALL end the listing, never restart it from its start, and the checkpoint SHALL stay the one the listing started from.

#### Scenario: A first listing the server keeps truncating
- **GIVEN** a CardDAV server truncating every `sync-collection` answer of a new address book
- **WHEN** the account is synced
- **THEN** the cards it listed reach the store, and no card is inferred deleted on either endpoint

## MODIFIED Requirements

None.

## REMOVED Requirements

None.
