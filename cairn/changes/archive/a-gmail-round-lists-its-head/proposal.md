---
cairn: change
id: a-gmail-round-lists-its-head
status: landed
created: 2026-10-13
---

# A Gmail round lists its head

## Why

A first sync of a large Gmail Inbox is paced by its quota and keeps its round open for many runs, a quarter of an hour on a big mailbox. Its later pages list older mail, so a message arriving meanwhile showed only at the first delta after the round closed. pimdir's `round-head` change (SYNC §4, §5, vectors 62 to 69, vector 62 being this very case) lets a run resuming an open round list the round's head first, and io-pimdir `07bd316` asks for it; neverest `a3deafd` plumbed the heads through but no backend listed one.

## What (design)

- **Gmail lists delta heads** (`Client::heads`). The head is the delta from the round's history id (the profile's, read before the round's first page; for a band round, the source's): the mailbox history read once a run, projected on the label as a delta is, each touched message's labels read in batches, a member that left the label or was deleted stated vanished, and the history id Gmail holds now as the checkpoint the round closes on. An expired history id (404), or a checkpoint that is no history id, rejects the head and the round restarts. Gmail dates no label change, so a band head is answered none.
- **Pages after a head read what it touched anew.** The run records, per label, when its head began and what it touched (`GmailRun::headed`). A later page of the label reads those messages again unless the run read them since the head (the head's own reads serve), and their labels decide membership and flags, whatever Gmail's page token still lists. Flag listings cached before the head are read again. Every message a page reads that no longer carries the label is left out, as a 404 already was. What the head did not touch keeps its cheap path (bound by binding and flag listings, kept from another label's read).
- **The driver asks for heads.** `sync_side` tells the sync verb the heads the source lists, and the pager counts a head as the start of a resumed round, so a resumed round's earlier pages are fed to idle connections as before. Under `--prime`, a round an earlier run left open lists its head before it stops, so an arrival is readable in the first pass.

## Out of scope

- IMAP (QRESYNC delta heads, band heads otherwise) and Graph (band heads): their own changes.
- Reading the history once from the lowest position of all collections: the run reads it again when a later start comes first, as deltas already do.
