---
cairn: log
change: the-rest-of-nev-12
date: 2026-11-17
---

# The rest of NEV-12

Finding NEV-12 of the CLI family analysis of 2026-11-15, the parts the first pass (2026-11-17-every-failure-reaches-the-result and the throttle and resolution fix) left.

## What landed

- A Google Calendar or People delete of an item gone already (404 or 410 on the read or the delete, or a Google event kept as a cancelled tombstone) counts as done, as on DAV, Graph and Gmail, where draft-13 held it as refused for good.
- `calendar-reply` and `calendar-cancel` intents are marked in flight (local/inviting) before their first request; one a killed run left marked parks unconfirmed rather than tell the attendees twice, and an outcome that cannot be recorded keeps the mark.
- The conflict auto-merge judges each merge by its own queue row: another app's row parking in the same drain no longer reads as a merge parked, and the account's other rows that drain applies are reported under `drained`. The spec now states the two mid-run drains (merge and sent copy) beside the one at the start.

## Left

- `remove` on Gmail: himalaya's direct backend deletes the message, neverest's store archives it (a delete from `INBOX` removes the label, sync.md). Which one `remove` means is a cross-app ruling.

## Spec

sync.md: "A delete of a member already gone is accepted", the drain paragraph of the queue requirement, the calendar intents requirement.
