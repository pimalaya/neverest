---
cairn: log
change: report-pulled-changes
landed: 2026-08-02
---

# Report the remote flag changes a pull applied

A flag changed remotely (`\Seen` added on the server) synced correctly but the
report said "already in sync" — it never mentioned the change. The report is built
from the post-pull projection, where a just-pulled flag change reads `Clean`, so
it made no hunk; and `sync_side` read only `ReplicaSyncReport.pulled`/`pushed`,
discarding the `events` io-replica already emits (which include `FlagsChanged`).

Fix: `mailbox_spine` snapshots `handle → flags` before the pull; `itemize_pulled`
consumes the pull's `events` and reports the remote-originated ones — a
`FlagsChanged` diffed against the snapshot into precise add/remove flag hunks, a
`Vanished` into a delete hunk. `Added` is skipped (already a `Fetch` in the pull
plan). Local→remote pushes and the download plan are unchanged.

Verified live (Stalwart): adding `\Seen \Flagged` server-side to a message that
already had `\Seen` re-synced to `Message patches (1): add [\flagged] to message
5 in INBOX` — precise (only the newly-added flag), where before it reported
nothing. 15 unit tests pass, fmt/clippy clean.

Spec updated: `sync` (ADDED "The report shows remote-originated changes a pull
applied").
