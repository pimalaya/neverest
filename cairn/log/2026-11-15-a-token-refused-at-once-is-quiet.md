---
cairn: log
change: a-token-refused-at-once-is-quiet
date: 2026-11-15
---

# A token refused at once is quiet

Ledger line N16 of the CLI alignment round of 2026-11-15, from live testing: a public Google holiday calendar refused its sync token seconds after issuing it, every run.

## What landed

- src/gcal/client.rs: the resumed listing already asked what the issuing one asked (`maxResults`, `showDeleted`), the token aside; `GcalClient::issued` remembers this run's tokens, and one refused that it issued is said at debug, an older one at warn.
- Test `a_resumed_listing_asks_what_issued_its_token`, over a session answering each request in turn.

## Left

- The calendar is still listed whole twice a run: Google's doing, the request being the right one.

## Capabilities moved

- [sync](../spec/sync.md): *Google contacts and calendars are native sources* amended.
