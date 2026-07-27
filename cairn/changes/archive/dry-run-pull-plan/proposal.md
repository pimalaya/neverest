---
cairn: change
id: dry-run-pull-plan
status: landed
created: 2026-08-01
---

# Dry run (and the report) shows the pull plan

## Why

A one-source `sync --dry-run` printed **"already in sync"** even with new messages
to download: the report itemized only *local→remote* work (staged flag/add/delete
pushes), while a one-source local sync's main action — **pulling the remote into
the retained store** — was never reported. The pulled items are `Clean` from the
remote's projection, so `itemize_single` shows nothing; and the download (hydration)
runs after the dry-run early-return. So the plan was invisible. (The real run had
the same gap: it downloaded N bodies but reported "already in sync".)

## What

- New `EmailHunk::Fetch { side, mailbox, id }` — "fetch message `<id>` in
  `<mailbox>` from `<side>`" — the download of a body into the local store.
- `itemize_fetches` reports the pull plan: each not-yet-`Full`, non-tombstone item
  the sync would hydrate. It runs in `sync_mailbox_single` for **both** dry and
  real runs (a dry run stops there; a real run hydrates them), so the report is
  consistent and reflects a local sync's main work.

Verified against a live Stalwart: a fresh one-source `--dry-run` with two remote
messages now prints `Message patches (2): fetch … / … would apply 2 hunks` and
downloads nothing; a real run reports the same two fetches and downloads the
bodies; an already-synced run reports nothing.

## Scope / non-goals

- Two-source dry run already itemizes cross-side copies (its plan) and is
  unchanged.
- Fetch hunks report the *download* plan; hydration itself (the streaming) is
  unchanged.
