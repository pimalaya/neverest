---
cairn: log
change: a-check-opens-the-send-channel
date: 2026-10-04
---

# A check opens the send channel

`neverest check` opened every source and listed its collections, but never the SMTP channel a source declares, so a wrong submission server or password surfaced only at the first send. MOA checks a mail box with `neverest check` before saving it, and could not tell its user that sending would fail.

## What landed

- src/cli/check.rs: after listing a source's collections, the check opens its SMTP channel through `connect_smtp` (upgrade and authentication, nothing sent), quits, and reports `smtp: true` on the endpoint; a channel that does not open fails the check, naming it.
- tests/submit.rs: `check_opens_the_smtp_channel_a_source_declares`, against Stalwart.

## Capabilities moved

- sync: a check opens the send channel too.
