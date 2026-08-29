---
cairn: log
change: a-decision-does-not-own-the-store
landed: 2026-08-29
---

# A decision does not own the store

`conflict resolve` took neverest's own `sync.lock` around the apply rather than the whole command, with a comment saying why: a sync must be free to run while a person is in a merger, and what that costs is exactly what the staleness guard answers. The comment was right and the code did the opposite, because `sync.lock` is not the only lock. The command opened the store with `PimdirStore::open`, which takes io-pimdir's owner lock for the lifetime of the handle, and that handle lived for the whole command. A sync of the same store was refused outright: `pimdir store at … is owned by another process`.

So the one thing that can move a placement's conflict revision could not run while a decision was being made, and the guard was unreachable in ordinary use. `Applied::Moved`, the `MAX_ATTEMPTS` retry loop, the "exporting it again" warning and both refusals about the remote having moved were dead code, proved dead against a real account by a merger that tried to sync its own store and was refused. Nothing was lost while they were dead, the push still being conditioned at the wire, but the second line of defence was never running.

Every conflict command now reads through `PimdirReader`, which owns nothing and takes no lock, so listing, showing and exporting the three bodies never keep a sync out. The resolution opens one per attempt and drops it before the merger runs; the store is taken again, under the run lock, only to apply what came back.

The retry path is now driven by a test rather than reasoned about. A merger blocks; another thread takes the owner lock's file, opens the store, records a newer conflict revision the way a sync would, and releases the merger; the decision comes back stale, is exported a second time and settles against what arrived. It asserts the file lock itself and not merely that a second `open` succeeds, because io-pimdir counts owning handles per process: in one process a second open succeeds off that count whether or not another process could get in. Holding the store open across the merger makes the test fail in twelve seconds with the lock reporting `WouldBlock`, which is the bug it exists to catch.

Capabilities moved: sync, one new requirement on deciding.
