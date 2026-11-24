---
cairn: log
change: the-live-night-findings
landed: 2026-11-24
---

# A collection new on the server is reported

Task N11 of the-live-night-findings. On the live night of 2026-11-23 (journal 22:33), a collection created on the server landed in the store while the run said `collectionPatch: []` and "already in sync".

`record_listing` (src/sync/driver/local.rs) now answers the kept collections the store did not hold, and a one-source run reports each as a new `CollectionHunk::Fetch` (JSON `kind: "fetch"`, text `fetch collection <name> from <source>`), in a dry run and a real run alike. Capability moved: sync.md, "The report shows the one-source pull plan".

Live on Stalwart (scratch store, folder `lane-a-n11` created by IMAP `CREATE`), after the fix:

```
Collection patches (1):
 - fetch collection lane-a-n11 from imap

Account stalwart synchronized: 1 hunks
```

and the next run's `collectionPatch` is empty again.

## Checked

- src/sync/driver/tests.rs `a_listing_names_the_collections_new_to_the_store`: with the listing answering no new collection (the old behaviour) it fails (`left: [] right: ["work"]`), with the fix it passes, and the text names the hunk.
