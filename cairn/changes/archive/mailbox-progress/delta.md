---
cairn: change
change: mailbox-progress
---

# Delta

## ADDED Requirements

### Requirement: The spinner reports in-mailbox progress
While syncing a mailbox, the spinner SHALL report progress through the slow inner
phase — body hydration (and, under relay, the relayed messages) — as a percentage
appended to the mailbox line (`[2/7] Syncing INBOX 66%`), updated per streamed
`Full` body. Fast phases (enumerate, the `Meta` upgrade) stay silent. The progress
tick is invoked from the concurrent fetch pool and MUST be safe to call from
several threads at once.

## MODIFIED Requirements

## REMOVED Requirements
