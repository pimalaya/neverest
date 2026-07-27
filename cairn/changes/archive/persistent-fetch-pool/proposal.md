---
cairn: change
id: persistent-fetch-pool
status: landed
created: 2026-07-31
---

# Persistent, configurable fetch pool

## Why

The first-cut fetch pool (`concurrent-size-ordered-fetch`) was **ephemeral**:
each `Full` batch opened its worker connections and dropped them, re-paying
TCP + TLS + SASL auth per batch — wasteful for an account with many mailboxes.
The worker count was also a hardcoded constant.

## What

- The pool is now **persistent**. A `Pool` holds a primary connection (opened up
  front for the sequential verbs) plus lazily-opened extras up to a budget, kept
  for the whole run so auth is paid once. `EmailRemote` wraps `&mut Pool` and
  distributes the pool's own `&mut Client` connections into the fetch workers
  (`Client` is `Send`), instead of opening fresh ones per batch.
- The budget **defaults to 4**, is **configurable per account** (`connections`)
  and **overridable** by `sync --connections/-j N`, and should stay under the
  backend's per-account connection cap.

## Scope / non-goals

- Reconnect-on-drop of a dead pooled connection is not yet implemented (an
  errored connection fails its op; the next run reopens). Per-side (rather than
  per-account) budgets are a follow-up, as is a native chunked m2dir stream.
