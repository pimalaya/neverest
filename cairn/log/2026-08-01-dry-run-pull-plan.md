---
cairn: log
change: dry-run-pull-plan
landed: 2026-08-01
---

# Dry run (and the report) shows the pull plan

Fixed `sync --dry-run` printing "already in sync" with new messages to download.
A one-source local sync's main action is pulling the remote into the retained
store, but the report only itemized local→remote pushes (`itemize_single`): pulled
items are `Clean` from the remote's projection, and the download runs after the
dry-run early-return, so the plan was invisible. (The real run had the same gap —
it downloaded N bodies but reported "already in sync".)

Added `EmailHunk::Fetch { side, mailbox, id }` ("fetch message `<id>` in
`<mailbox>` from `<side>`") and `itemize_fetches`, which reports each
not-yet-`Full`, non-tombstone item the sync would hydrate. It runs in
`sync_mailbox_single` for **both** dry and real runs (dry stops there; real
hydrates below), so dry-run and real reports are consistent and reflect the local
sync's main work.

Verified on live Stalwart: a fresh one-source `--dry-run` with two remote messages
prints `Message patches (2): fetch mid:d1@… / mid:d2@… … would apply 2 hunks` and
downloads nothing (real store untouched); a real run reports the same two fetches
and downloads the bodies; an already-synced run reports nothing. 14 unit tests
green, fmt/clippy clean, relay integration unregressed.

Spec updated: `sync` (ADDED: the report shows the one-source pull plan).
