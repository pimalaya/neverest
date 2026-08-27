---
cairn: log
change: dry-run-names-the-bodies
date: 2026-08-28
---

# A dry run names the bodies it would fetch, and fetches none

A freshly initialized account holding an IMAP source and a CardDAV source
reported `8840 hunks` for `sync -d -s imap` and `already in sync` for
`sync -d -s carddav`, over an address book holding cards and a store holding
nothing. Two bugs, both in the two lines that decide what a plan contains.

First-time discovery reaches the report through `itemize_fetches` alone, and it
read `projection_view`, documented as the hub projection "without the residual
probes". The residual is where a freshly probed item sits until its link id is
known, and a card has none before its body arrives, a `sync-collection` REPORT
returning hrefs and ETags but no `UID`. Every item of a first DAV sync was
therefore invisible to the plan. Mail escaped only because its probe resolves at
`Meta`, which moves it into the hub. It now reads the side, projection plus
residual, which is what the coroutines themselves see.

The itemizer was wrong a second way, and that one outlives the dry run: it
skipped a placement at `level >= Full`, and a remote content change drops the
stale object while the hub keeps the level the item had reached, so an item whose
body was about to be re-fetched read as complete. `collection_spine` already knew
that, its hydration targets keying on the object with a comment saying why, and
the itemizer beside it kept keying on the level. It now keys on the object, which
is what it was always a preview of.

The second bug is that `upgrade_probed` ran before the dry-run return, so
previewing a DAV account downloaded its whole address book to print a plan and
then printed nothing. It now takes the run's dryness and returns early where the
tier is `Full`. A cheaper tier still resolves, so a mail preview keeps naming
messages by their `Message-ID`; a card stays probed and is named by its href,
which is the only name it honestly has before the body it exists to avoid
downloading.

Both are covered in the live CardDAV suite: a dry run before any real sync names
both cards, and a dry run after a server-side edit names the re-fetch.

Capabilities moved: **sync** (the one-source pull plan).
