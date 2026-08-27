---
cairn: change
id: dry-run-names-the-bodies
status: landed
created: 2026-08-28
---

# A dry run names the bodies it would fetch, and fetches none

## Why

An account holding an IMAP source and a CardDAV source, freshly initialized,
reported `8840 hunks` for `sync -d -s imap` and `already in sync` for
`sync -d -s carddav`, over an address book holding cards and a store holding
nothing. Both bugs behind it are in the same two lines.

**The plan read the hub projection, which drops the residual.** First-time
discovery reaches the report through `itemize_fetches` alone, and it read
`projection_view`, documented as the hub projection "without the residual
probes". The residual is where a freshly probed item sits until its link id is
known, and a card has no link id before its body arrives, a `sync-collection`
REPORT returning hrefs and ETags but no `UID`. Every item of a first DAV sync was
therefore invisible to the plan, and an account that had never synced was told it
was in sync while a mail source beside it reported thousands of hunks. Mail
escaped only because its probe resolves at `Meta`, which moves it into the hub.

The itemizer was wrong a second way, and this one survives a real run: it skipped
a placement at `level >= Full`, and a remote content change drops the stale object while the hub keeps the level the
item had reached. An item whose body is about to be re-fetched therefore reads as
complete. `collection_spine` already knew this, its hydration targets keying on
the object with a comment saying why, and the itemizer beside it kept keying on
the level.

**A dry run downloads every card.** `upgrade_probed` runs before the dry-run
return, so previewing a DAV account fetches its whole address book to print a
plan, then reports nothing. The one thing `--dry-run` promises is a cheap look.

## What

- `itemize_fetches` reads the side rather than the hub projection, so an item
  still in the residual is named, and keys on the stored object rather than on
  the level, matching the hydration pass it is the preview of.
- `upgrade_probed` takes the run's dryness and returns early when a dry run would
  have to reach `Full`. A cheap tier still resolves, so mail previews keep naming
  messages by `Message-ID`; a DAV kind stays probed, carries no object, and every
  item is named by the fetch itemizer.

## Not in scope

**No link ids in a DAV preview.** Left probed, a card has no `UID` yet, so the
plan names it by its href. Resolving it is the download this change exists to
avoid, and a preview that says which items will be fetched, by the only name it
honestly has, is what a dry run is for.
