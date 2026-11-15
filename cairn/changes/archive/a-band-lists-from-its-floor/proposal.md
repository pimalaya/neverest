---
cairn: change
id: a-band-lists-from-its-floor
status: landed
created: 2026-11-15
---

# A band lists from its floor

## Why

Live testing of 2026-11-15 (ledger line N17 of pimalaya/.github cairn/changes/cli-alignment-2026-11-15) lost mail from the store, the servers untouched:

- Gmail: a full sync bound INBOX 319 and SENT 91; `sync --since 30d` changed nothing, its coverage since 2026-10-16; the unscoped sync then deleted 13 messages in INBOX and the same 13 in SENT, which Gmail still held, and no later sync brought them back. They are dated 2026-01-01 and arrived recently. The widening round over the band `since: None, until: 2026-10-16` listed by `before:` (2026-10-18), which reads the arrival, so it never listed them.
- Microsoft Graph: the same two runs on Sent Items deleted 24 messages (store 422, server 446). Their `sentDateTime`, Exchange's submission, is 2026-10-16 or 2026-11-02, their `Date` older. The band listed by `sentDateTime lt 2026-10-16`.

The engine finds absent in a narrowed round every member dated in its scope before the round opened (io-pimdir list_unstamped_bindings.sql: `s.date < r.round_started_at`), whatever the enumeration says: it holds a filter lists every member dated before it opened. That holds for a filter on the `Date` (IMAP's `SENTSINCE` and `SENTBEFORE`, a margin of days around), not for one on another date that may lie long after it: a ceiling on the arrival or the submission misses every member its `Date` puts in the band and that arrived or was sent above the band (a delayed delivery, a draft sent weeks after it was written, an import). Graph's `sentDateTime` was believed to be Graph's reading of the `Date` header; on Sent Items it is not.

## What

One rule, `client::floor_of`, taken by the two connectors whose filter reads another date than the `Date`: a round lists by the scope's floor alone, never by a band's ceiling. Gmail drops `before:` (`after:` keeps its two-day margin); Graph lists `sentDateTime ge <floor>` and pages below the second a page ended at, as before, never below the band's ceiling. The listing reaches above the band, and the page keeps what its `Date` puts in the band (`keep_in_scope`, unchanged). IMAP keeps both bounds: `SENTSINCE` and `SENTBEFORE` read the `Date`.

## Decisions

- **In the connectors, not in the engine.** The absence rule is io-pimdir's and sound for a filter on the `Date`; what was wrong is two connectors stating a narrowed listing that could miss what it claimed. Listing a superset keeps the engine's rule sound without editing the library.
- **The ceiling goes, the floor stays.** A message is dated before it is submitted or received, at times long before, never long after: a floor on those dates misses only a member dated after its submission beyond the margin (Gmail two days, Graph none), which the main scoped round risks as well. Dropping the floor too would make a widening list the whole mailbox, reading the summary of every unbound message below the band.
- **The cost**: a widening lists the coverage above its band again (Gmail ids and three flag listings, cheap for a bound member; Graph summaries). A widening is rare and the coverage is the recent part.
- **An enumeration would not do**: the engine finds absent a member dated before the round opened even when the enumeration names it.
