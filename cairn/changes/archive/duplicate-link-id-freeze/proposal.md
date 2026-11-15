---
cairn: change
id: duplicate-link-id-freeze
status: landed
created: 2026-08-25
---

# Report an identity a collection holds twice

> Cross-repo change, same id in three repos, in this order:
> **io-replica** (the invariant, the detection, the rules) → **io-pimdir**
> (persistence, and the write that destroys the evidence) → **neverest** (here:
> the report, and the end-to-end proof). Both halves upstream must land first;
> this one only surfaces what they mark.
>
> An earlier draft of this proposal put the whole mitigation in neverest. That
> was wrong twice over, and the reasons are worth keeping: the evidence is
> destroyed by a storage write, so a connector cannot see it reliably, and a
> per-run check in a connector expires on the next run, because with QRESYNC the
> second copy appears in exactly one enumeration. The freeze must be sticky, and
> sticky means persisted, which is the store's job and the engine's rules.

## Why

One collection may hold two messages with the same `Message-ID`. Reproduced
against two IMAP servers (`tests/stalwart2.sh`, A on :143, B on :144), one copy
on A and two on B, synced two-sided:

1. The first sync pairs A's copy with **one** of B's; the other is unbound and
   invisible.
2. Deleting the bound copy on B propagated a delete to A and **removed the only
   copy there**, while B still held the message. The local copy survived only
   because retention keeps the row.
3. After a checkpoint loss on B (a UIDVALIDITY bump, a server without QRESYNC, a
   reset), the full enumeration revived the retained row and **re-appended the
   message to A**. The run reported `Account dup is already in sync`.

With the two upstream halves landed, the engine marks such an identity ambiguous
and derives nothing for it, so neither step 2 nor step 3 can happen. What is
left is the part only this crate can do: **telling the user**, in the language of
mailboxes and UIDs rather than of placements, and proving the whole chain
against real servers.

Step 3 also exposed a defect of this crate's own: a run that appended a message
to a server reported no hunk at all. A report that denies a write is worse than
a missing warning, and it is fixed here.

## What

- **A warnings section in the run report**, text and `--json`, naming the
  collection and every handle involved (`INBOX: 2 copies of one message, UIDs
  145 and 146 — not synced until one is removed`), re-reported on every run the
  way a conflict is, since a warning the user cannot act on twice is a warning
  they will not act on once.
- **Wording that blames nothing.** RFC 5322 §3.6.4 binds the *generator* of a
  `Message-ID` and says nothing about what a store may hold, and a copy
  legitimately carries the identifier of the message it copies. Duplicates
  commonly arrive from a migration, which is this tool's own use case. So the
  report says neverest cannot tell the copies apart, never that the mailbox is
  invalid.
- **The run report accounts for every write.** An append the sync performed
  appears as a hunk; `already in sync` means nothing was written.
- **The end-to-end proof**, replaying the three steps above against two Stalwart
  instances and asserting the other side's copy survives step 2 and is not
  re-appended in step 3.

## Scope / non-goals

- **Detection, policy and state are upstream** (io-replica and io-pimdir, same
  change id). This crate adds no duplicate detection of its own: doing it here
  again would be a second, weaker answer that expires with the checkpoint.
- **Deriving the link id stays here**, being kind-specific, and the engine never
  derives one.
- **A frozen item is mirrored zero times, not once**, which is right for
  propagation and wrong for backup. Only 1:N bindings serve that case, and they
  are the successor to this change, not part of it.
- **No repair verb.** Neverest reports coordinates; removing a duplicate is the
  user's, with their own client.
