---
cairn: change
id: a-refused-write-is-not-an-applied-hunk
status: landed
created: 2026-08-29
---

# A refused write is not an applied hunk

## Why

The `Item patches` section is the plan a run derived from the projection, itemized before anything is pushed. Nothing revisited it afterwards, so a write the server refused was still counted among the hunks the run applied, and the run exited 0.

Observed against a real CardDAV account, on every run, indefinitely:

    WARN neverest::offline::remote update nvt-delta.vcf in Default rejected:
      WebDAV server returned HTTP 403: Resource is not a vCard object
    Item patches (1):
     - update item nvt-delta.vcf in Default on carddav
    Account fastmail synchronized: 1 hunks

Exit code 0. The server still held the previous body, the placement stayed dirty, and the next run reported the identical phantom. At the default log level there is no warning at all, so the only signal a wrapper script or a cron mail has is the exit code, which says the run succeeded.

The spec already answers this in principle: "A run that wrote to a remote SHALL report it. `already in sync` SHALL mean the run wrote nothing", so a report reads as the record of what happened. A hunk for a write that did not land breaks exactly that reading.

## What

- The remote seam collects the writes a side would not take, beside the refused duplicates it already collects: the collection, the handle, what was being attempted and why it failed. Both halves of a rejection are covered, the server's no and the write that never reached one because its body was missing from the blob tree.
- The driver drains them into the report as `RejectedWrite` entries, counted as warnings, and **takes back the hunk it had derived for that item**, so the total is what reached a server. A create is itemized by link id rather than by handle and therefore matches no hunk; it stays in the patch beside its refusal, which is the honest half of what can be known where the two vocabularies meet.
- A create refused with the `no-uid-conflict` precondition keeps its own entry and gains no second one: it names the identity and the remedy, which the generic refusal cannot, and one write is one line.

## The judgement to review: the exit code

**A run that could not deliver a write now exits 2 rather than 0, and so does a run holding a refused duplicate, which used to exit 0.**

This turns out to be less of a judgement than it first looked: the shipped help text already promised it. `sync --help` reads "Exit code 0 means the run reconciled everything and left nothing waiting", and a run that could not deliver a write has left something waiting. So the generalisation makes the code keep the contract it documents, rather than inventing a new one; what follows is why that contract is the right one to keep.

The alternative was to leave the code at 0 and let the report carry it. That loses the finding's whole point: at the default log level the exit code is the only thing a cron job sees, and a run that has been failing to deliver the same write for three hours should not keep saying it succeeded.

Exit 2 is defined as "reconciled its collections and left conflicts behind", chosen so a run that parked something a person must settle does not pretend to break. A refused write is the same class of outcome, item-wide rather than fatal, re-reported until somebody acts, and unchanged by a rerun: it is the state the code exists for. `RefusedDuplicate`'s own spec text already says as much, "the run wrote nothing, the state is unresolved", and it is now folded into the same answer for consistency rather than left as the one refusal that reports success.

What this costs: a wrapper distinguishing "conflicts pending" from "everything delivered" keeps working, and a wrapper reading 2 as "conflicts, specifically" now also sees it for a refusal. The report says which, in both text and `--json`. A transient rejection that the next run delivers exits 2 once and then stops, which is the same shape as a conflict resolved between two runs.
