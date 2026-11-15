---
cairn: change
id: an-undated-search-may-be-refused
status: landed
created: 2026-11-15
---

# An undated search may be refused

## Why

Live testing of 2026-11-15 (ledger line N18 of pimalaya/.github cairn/changes/cli-alignment-2026-11-15): on iCloud IMAP every date-narrowed round failed. A round searched `UID SEARCH ... OR SENTSINCE d NOT HEADER Date ""`, the undated clause keeping the messages with no `Date`, which are in every scope; iCloud answers `NO [UNAVAILABLE] Unexpected exception` to `NOT HEADER Date ""`, alone or inside the `OR`, and `OK` to `SENTSINCE` and `SENTBEFORE` alone. Any scoped round (`--since`, `item.filter.since`, the widening after one) failed on every collection, exit 4, and once a round was open every later run failed too.

## What

The round searches its dated messages (`SENTSINCE`, `SENTBEFORE`, the UID ranges) and its undated ones (`NOT HEADER Date ""`, the same ranges) by two searches, their union the set the one search named. A NO or BAD to the undated search is the server saying it cannot tell: the round warns, lists the dated messages, and the connection, a reconnection included, sends it no more.

## Decisions

- **No absence is at risk, so the round need not prove less.** The engine finds an undated member absent in a narrowed round only when the enumeration (`UID SEARCH ALL`) lacks it, and never in a band (io-pimdir list_unstamped_bindings.sql): a round missing the undated messages concludes nothing wrong. What is lost on such a server is the listing of an undated message the store does not hold yet, while the scope holds; an unscoped round lists it.
- **Asked once a connection**: a refusal is a property of the server; asking at every round and page would log the same warning and spend a round trip each time.
- **One more search a round** on every server, the cost of the split.
