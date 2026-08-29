---
cairn: change
id: a-source-drains-its-own-namespace
status: landed
created: 2026-08-28
---

# A source drains its own namespace, not the store's

## Why

The pre-sync drain listed the collections with pending work and drained all of
them, once per source, through that source's handle. The listing is the whole
store's: `queued_collections` takes no source, the queue recording none. So
every source drained every collection.

Draining another source's collection is not harmless. Staging an existing item's
action resolves that item's binding **for the draining source**, and a contacts
source holds no binding in a mail collection, so the action could not be placed.
io-pimdir parked it, and a parked row is terminal: skipped by every later drain,
cleared by no verb, its only exit `queue cancel`, which throws the action away.

Sources run in name order, so this was not a race that sometimes bit. On a
posteo account declaring `caldav`, `carddav` and `imap`, caldav drains first and
reached every action himalaya queued against `imap/INBOX` before imap did. The
item was there with a live `imap` binding; the flag change was destroyed anyway,
and the report said `seq 6951 projects no placement` three times over.

## What

- `drain_queues` takes the source's namespace and drains only the collections
  under it, a hub collection id being `<namespace>/<name>`.
- The drain's info line reports what was skipped beside what was applied and
  parked.

io-pimdir is fixed in the same breath, so that a consumer draining another
source's collection leaves the row pending instead of parking it. This change is
what stops neverest doing it at all; that one is what keeps the damage from
being permanent when something does.

## Not in scope

**The rows already parked.** They stay until `pimdir queue cancel <id>` drops
them, and the action they carried has to be redone in the frontend.
