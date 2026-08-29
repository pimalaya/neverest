---
cairn: log
change: a-refused-write-is-not-an-applied-hunk
landed: 2026-08-29
---

# A refused write is not an applied hunk

The item patch is the plan a run derives from the projection before it pushes anything, and nothing revisited it afterwards. A write the server refused was therefore counted among the hunks the run applied. Against a real CardDAV account, on every run for three hours: a warning at `warn` level that the `PUT` came back `403`, then `update item nvt-delta.vcf` in the patch, then `synchronized: 1 hunks`, then exit 0. At the default log level there is no warning at all, so the only thing a cron job or a wrapper script could see said the run had succeeded over a change that never left the store.

The remote seam now collects the writes a side would not take, beside the refused duplicates it already collected, with the collection, the handle, what was attempted and why it failed. Both halves are covered: the server's refusal, and the write that never reached one because the blob tree no longer holds its body. The driver reports them as warnings and takes back the hunk it had derived for that item, so the count is what reached a server and `already in sync` keeps meaning the run wrote nothing. A create refused with the no-uid-conflict precondition keeps its own entry, which names the identity and the remedy, and gains no second one.

**The judgement to review is the exit code.** A run that could not deliver a write now exits 2, and so does a run holding a refused duplicate, which used to exit 0. Exit 2 was defined for a run that "reconciled its collections and left conflicts behind", chosen so that parking something a person must settle does not read as a crash. An undelivered write is the same class: item-wide, unresolved, re-reported every run, and unchanged by a rerun. Leaving it at 0 would have left the finding's actual complaint standing, since the exit code is the only signal at the default log level. A wrapper reading 2 as "conflicts, specifically" now also sees it for a refusal, and the report says which, in text and in `--json`.

Only the outcome the run learns is reported. A create is itemized by link id rather than by handle, so a refused create matches no hunk and stays in the patch beside its refusal; the two vocabularies do not meet there, and inventing a match would be worse than saying both.

Capabilities moved: sync, one new requirement on the report, one modified on the exit code.
