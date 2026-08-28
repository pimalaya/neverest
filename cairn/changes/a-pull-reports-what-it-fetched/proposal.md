---
cairn: change
id: a-pull-reports-what-it-fetched
status: landed
created: 2026-08-28
---

# A pull reports what it fetched, whatever the kind

## Why

A CardDAV run that downloaded an address book reported `already in sync`. The
same account under `--dry-run` reported the bodies it would fetch, so the two
disagreed about the same work, and the only way to tell the real run had done
anything was to read the store.

The pull plan is derived from the placements that carry no body yet. It was read
*after* the probe that resolves link ids, and a kind with no cheap `Meta` tier
resolves its link id from the body itself: a card's link id is its vCard `UID`,
and `parse_summary` is `None` for cards, so the probe downloads the whole card.
Every card is therefore hydrated by the time the plan is read, and the plan is
empty. Mail resolves its link id from an `ENVELOPE` at the `Meta` tier, leaves
its bodies for the hydration phase, and reports them, which is why the two kinds
disagreed.

A dry run does not hydrate during the probe, which is why it reported the
fetches the real run stayed silent about.

## What

- The pull plan is read before the probe rather than after it, so it names the
  same bodies whether or not the run is a dry one, and whatever tier the kind
  resolves its identity at.

## Not in scope

**No new hunk kind.** `ReplicaEvent::Added` would name a pulled member directly,
and reporting both it and the pull plan would count every first-sync item twice.
The plan already says what the run does; it was only read at the wrong moment.
