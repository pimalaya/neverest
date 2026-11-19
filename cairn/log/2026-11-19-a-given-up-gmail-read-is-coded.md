---
cairn: log
change: a-given-up-gmail-read-is-coded
date: 2026-11-19
---

# A given-up Gmail read is coded

Left open by the coordinated round of 2026-11-18: the rows of the collections a throttled Gmail source was reading carried `code: null`, where sync.md's "A server's refusal carries its code" names `rate-limited`. The throttled requests themselves were coded (the throttle builds a 429 `GmailApiError`), but the batched reads give up in their own words, as plain errors.

## What landed

- src/gmail/batch.rs: `read_all`'s two give-ups, a daily quota and a read refused `MAX_TRIES` times, fail as a `CodedError` `rate-limited`. `Inner::Failed` carries the code `google_code` reads from the inner answer, so a batched read refused for rights reads `permission-denied`.
- src/gmail/client.rs: `LiveBatcher::pause`'s two give-ups (the source given up already, a pause longer than `--max-wait`) fail coded `rate-limited` too.
- Tests, both failing on the code before with `None`: src/gmail/client.rs `a_batched_read_refused_past_the_waits_is_coded` (a scripted batch answering a daily quota, a per-minute quota over a one-second `--max-wait`, and a rights refusal), src/gmail/batch.rs `a_message_refused_without_end_gives_the_source_up` (now asserting the code).

## Spec

- [sync](../spec/sync.md): *A server's refusal carries its code*, scenario "A source given up on a quota".
