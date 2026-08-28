---
cairn: log
change: a-pull-reports-what-it-fetched
landed: 2026-08-28
---

# A pull reports what it fetched, whatever the kind

A CardDAV run that downloaded an address book printed `already in sync`, while `--dry-run` against the same account listed the bodies it would fetch. Two runs, same work, opposite reports, and the store was the only place the truth showed.

The pull plan is `itemize_fetches`: the placements that carry no body yet. It was read after `upgrade_probed`, which resolves link ids. That ordering holds for mail, whose link id comes from an `ENVELOPE` at the `Meta` tier, leaving the bodies for the hydration phase and therefore in the plan. It does not hold for a kind whose link id comes from the body: a card is identified by its vCard `UID`, `Kind::parse_summary` is `None` for cards, so the probe downloads the whole card to resolve it. Every card was hydrated before the plan was read, and the plan came back empty. Under `--dry-run` the probe does not hydrate, which is exactly why the dry run reported what the real run did not.

The plan is now read before the probe. Both kinds report the same list, both run modes agree, and a first contacts sync says what it fetched. Mail is unaffected: its placements carry no body at either point, so the list is identical.

Capabilities moved: sync, one new requirement on the report.
