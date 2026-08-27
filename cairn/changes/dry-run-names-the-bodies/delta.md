---
cairn: change
change: dry-run-names-the-bodies
---

# Delta

## MODIFIED Requirements

### Requirement: The report shows the one-source pull plan
A one-source sync SHALL report its pull plan, each non-tombstone item whose body
it would download into the store, as `Fetch` hunks, in both a dry run (which
stops there) and a real run (which then hydrates them). A dry run SHALL fetch no
body to produce that report.

The plan SHALL read the source's own placements, its hub projection **plus its
residual**, never the projection alone. The residual holds a freshly probed item
until its link id is known, and a kind whose identity lives in the body (a card,
whose `sync-collection` REPORT carries no `UID`) has none before it is fetched,
so a projection-only read reports nothing at all for a first sync of that kind.

Within that read the plan SHALL select an item by the absence of a stored object,
never by its detail level: a remote content change drops the stale object while
the hub keeps the level the item had reached, so an item about to be re-fetched
would otherwise read as complete. Selecting on the object matches the hydration
pass the plan is a preview of, so the two cannot disagree.

Raising a fresh probe to the tier its kind resolves at SHALL be skipped in a dry
run where that tier is `Full`. A cheaper tier MAY still resolve, so a mail
preview keeps naming messages by their `Message-ID`; an item left probed SHALL be
named by whatever handle it has. A preview that downloads an entire address book
to print a plan is not a preview.

#### Scenario: A first dry run over a DAV account names its items
- GIVEN an initialized account with a CardDAV source and cards on the server
- WHEN `sync --dry-run` runs before any real sync
- THEN the report names each card, rather than reporting the account already in sync

#### Scenario: A dry run after a server-side edit names the re-fetch
- GIVEN a synced card edited on the server
- WHEN `sync --dry-run` runs
- THEN the report names the card whose body would be re-fetched
