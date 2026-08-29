---
cairn: change
id: a-decision-does-not-own-the-store
status: landed
created: 2026-08-29
---

# A decision does not own the store

## Why

`ConflictResolveCommand::execute` was written to leave the store free while a person is in the merger, and said so:

    // The lock is taken here rather than around the whole command, so
    // a sync is free to run while a person is in a merger. What that
    // costs is exactly what the staleness guard answers.
    let _lock = acquire_store_lock(&dir, LOCK_TIMEOUT)?;

That is true of neverest's own `sync.lock` and false of the other lock in play. The command opened the store with `PimdirStore::open`, which takes io-pimdir's owner lock (pimdir SPEC §8) for the lifetime of the handle, and that handle lived for the whole command, merger included. A sync of that store was refused outright:

    Caused by:
     - pimdir store at …/storeB is owned by another process

So the only thing that can move a placement's conflict revision could not run while the decision was being made, and the staleness guard was unreachable in ordinary use: `Applied::Moved`, the `MAX_ATTEMPTS` retry loop, the "exporting it again" warning and both `bail!` arms about the remote having moved were all dead code. Its unit test passes by constructing a `Conflict` carrying a stale revision, which never exercises another writer.

Nothing was lost while it was dead: the push is still conditioned on the revision the resolution was computed against, and a resolution computed against a revision the server had moved past was refused at the wire and re-conflicted. The guard is the second line of defence, and it was the one not running.

## What

- Every conflict command reads through `PimdirReader`, which owns nothing and takes no lock, so listing, showing and exporting the three bodies never keep a sync out.
- `conflict resolve` opens that reader per attempt, reads the divergence and its bodies, and drops it before the merger runs. The store is opened again, under `sync.lock`, only to apply what came back. `Conflict::apply` already opened its own handle, so the window narrows to the write itself.
- The retry loop is now reachable, and a test drives it: the merger blocks, another thread takes the owner lock's file and the store, records a newer conflict revision as a sync would, and lets the merger answer. The decision comes back stale, is exported a second time and settles against what arrived.

The test asserts the file lock itself rather than only that a second `open` succeeds, because io-pimdir counts owning handles per process: a second open in the same process succeeds off that count whether or not another process could get in. The lock is what a concurrent sync contends for, so the lock is what is asserted.

## Not in scope

**Applying without the owner lock.** The write still takes it, briefly, which is correct: it is a write, and pimdir's contract is one owner at a time. What was wrong was holding it across a decision that takes a person minutes.
