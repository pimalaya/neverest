---
cairn: log
change: a-meeting-series-is-announced-once-whole
date: 2026-11-17
---

# A meeting series is announced once whole

Finding NEV-15 of the CLI family analysis of 2026-11-15.

## What landed

- A series with occurrences set apart, created on Google Calendar or Graph, now gets its attendees last: the master is created without them (Google: `sendUpdates=none`), the overrides written (Google: `sendUpdates=none`, an override naming the series' attendees leaving them to the series), then a patch of the master adds the attendees (Google: `sendUpdates=all`). A series taken back because an occurrence did not land so mails nobody, where it mailed every attendee an invitation and a cancellation on every run that pushed it again.
- A series whose attendee patch fails is deleted again (Google: `sendUpdates=all`, which tells nobody the patch did not reach).

## Evidence

Live check on the Google and Microsoft test accounts, 2026-11-17 18:20 to 18:37 UTC, in throwaway calendars, cleaned up after (scripts and run.log in the session scratchpad, not kept):
- Google: insert, instance patch and delete all with `sendUpdates=none` mailed nobody and put nothing in the attendee's calendar. Insert without attendees, override, then a master patch adding the attendee with `sendUpdates=all` sent one invitation for the series and one for the modified occurrence, and the attendee's calendar matched the final series; the modified instance took the attendee from the series. A master patch that changes nothing records nothing and mails nothing, so a series inserted with attendees silently cannot be announced later without a real change.
- Graph: creating an event with attendees always mails them (documented, "can't be configured"). Create without attendees, patch an instance, then patch the master adding the attendee sent one invitation for the series and one for the exception, the exception carrying the attendee after; once announced, both removing the attendees and deleting mail a cancellation.

## Left

- A failure of the attendee patch itself after Google or Graph applied it still mails (invitation, then cancellation).
- A propagated series whose failure stays rejected is still pushed again every run; with this change it mails nobody until its occurrences land.
- An override naming attendees other than the series' is written with them, silently on Google and as Graph's instance patch does on Graph; not checked live.

## Spec

sync.md, "A series is pushed whole or not at all".
