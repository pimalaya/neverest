---
cairn: log
change: the-live-night-findings
landed: 2026-11-24
---

# A Google Meet on an event with no attendee

Task N6 of the-live-night-findings. The live night of 2026-11-23 found two failures that looked like one. tests/google.rs `a_google_meet_is_created_with_its_event` failed with "Google attached a conference", and calendula's `--online-meeting` add got no Meet.

1. The suite never asked for a Meet. Its `Replica::add` queues through io-pimdir's `enqueue_with_body`, which strips every `X-PIMDIR-` property from a calendar body (pimdir STORAGE §18), so the event reached Google with no conference request (traced on 2026-11-24). tests/common now has `Replica::add_asking`, queuing through `enqueue_user_body` as a frontend does for a meeting the user ticked. The Graph suite's online meeting test had the same flaw and uses it too.
2. An event asking a Meet with no attendee to tell was imported (`events.import`, `conferenceDataVersion=1`), and Google leaves an imported event's create request `pending` for ever. Such an event the account organises is now inserted with its `iCalUID` and `sendUpdates=none` (src/gcal/client.rs `add_item_stream`).

Capability moved: sync.md, the Google Calendar source (create by import or insert).

## Checked

- tests/google.rs `a_google_meet_is_created_with_its_event`, now a scheduled meeting and a lone event: without the fix it fails ("timed out waiting until the Meet links reach the store"), with it it passes.
- src/gcal/client.rs `an_event_asking_a_meet_is_inserted_quietly`: without the fix the create is `POST .../events/import`, with it an insert with `sendUpdates=none`.
