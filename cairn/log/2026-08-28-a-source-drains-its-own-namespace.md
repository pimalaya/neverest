---
cairn: log
change: a-source-drains-its-own-namespace
landed: 2026-08-28
---

# The drain stopped answering for other sources

`drain_queues` listed the store's queued collections and drained all of them
through the running source's handle. The listing is store-wide, the queue
recording no source, so every source drained every collection: on the posteo
account that surfaced this, `caldav` sorts first and reached every action
himalaya had queued against `imap/INBOX` before `imap` did.

That was not a wasted pass. Staging an existing item's action resolves the
item's binding for the draining source, caldav holds none in a mail collection,
and io-pimdir parked what it could not place. A parked row is terminal, so the
flag change was gone before the source that could apply it looked: item 6951 was
in the store the whole time, with `binding imap: 26702` and its `\Seen` intact.

**Narrowing** (`offline/driver.rs`): `drain_queues` takes the source's namespace
and skips collections outside `<namespace>/`. `run_pair` passes the pairing's
namespace, `run_local` the source name, which is the same value: a hub
collection id is built by `hub_id(namespace, name)` and the namespace defaults
to the source's own name. The drain's info line now names skipped actions beside
applied and parked ones.

**In io-pimdir**, in the same breath: a missing binding for the draining source
now leaves the row pending instead of parking it (`not-mine-is-not-broken`).
This change is what stops neverest reaching another source's collections; that
one is what keeps the damage from being permanent when a consumer does.

Covered by `a_source_drains_only_the_collections_of_its_own_namespace`: an
action queued against `imap/INBOX`, drained as `caldav`, applies nothing, and
the `imap` drain then applies it. The drain tests were moved onto namespaced
collection ids while I was there, which is what production has always enqueued.

The rows parked by the old behaviour stay parked. `pimdir queue cancel <id>`
drops one, and the action it carried has to be redone in the frontend.

Verified: 130 unit tests green, three runs, fmt and clippy clean, and the suite
green against both the released io-pimdir and the local one carrying its half of
the fix.

Spec updated: `sync` (ADDED: "A source drains the collections of its own
namespace").
