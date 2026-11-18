---
cairn: log
change: an-expired-calendar-token-is-quiet
date: 2026-11-18
---

# An expired calendar token is quiet

Found by the live run of 2026-11-17: Google's "Holidays in France" calendar logged "calendar sync token … expired" at warn every run. The quieting of 2026-11-15 (a-token-refused-at-once-is-quiet) only covered a token the same run issued, and Google also refuses the token an earlier run stored.

## What landed

- src/gcal/client.rs: an expired sync token (410) is logged at debug whoever issued it, and the calendar is listed whole as before. `GcalClient::issued` is gone. A warn is for what the user can fix, and an expired token is the provider's to give.

## Spec

- [sync](../spec/sync.md): the Google Calendar enumeration requirement says an expired token at debug.
