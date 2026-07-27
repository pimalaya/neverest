---
cairn: change
change: report-pulled-changes
---

## ADDED Requirements

### Requirement: The report shows remote-originated changes a pull applied
A sync SHALL report the remote-originated changes a pull applied to already-synced
items — flag changes and removals — not only the local→remote pushes and the
download plan. Because the pull applies them silently (the item reads `Clean`
afterwards), they SHALL be recovered from the sync's per-item `events`: a
`FlagsChanged` diffed against a pre-pull `handle → flags` snapshot into precise
add/remove flag hunks, and a `Vanished` into a delete hunk. A newly-pulled message
(`Added`) is already reported by the pull plan (a `Fetch` hunk) and is not
re-itemized.
