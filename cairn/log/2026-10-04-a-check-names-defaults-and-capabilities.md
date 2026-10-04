---
cairn: log
change: a-check-names-defaults-and-capabilities
date: 2026-10-04
---

# A check names defaults and capabilities

A caller adding an account picks one calendar and one address book, and needs to know before any sync what each source can do (create a folder, reply to an invitation). `neverest check` named the mail roles only, and capabilities reached the store at the first sync.

## What landed

- src/client.rs: `Client::default_collection`, a check-only probe answering the id of the collection a new item goes to.
  - src/gcal/client.rs: the calendar the calendar list marks `primary`.
  - src/gpeople/client.rs: the one address book (`contacts`, the user's own contacts).
  - src/msgraph/client.rs, client/calendar.rs: the calendar with `isDefaultCalendar`; the default Contacts folder (`contacts`).
  - src/dav/client.rs: on CalDAV, the principal's `schedule-inbox-URL`, then the inbox's `schedule-default-calendar-URL` (RFC 6638 §9.2), keyed by its last segment as a listing keys a calendar; a failed lookup is no default. CardDAV and mail answer none.
- src/cli/check.rs: each collection carries `default: true` when it is that one (left out when false); each source lists `capabilities: [{ name, support, detail? }]`, the source-wide rows `offline::capability::declaration` gives a sync, empty for a target.
- Tests: tests/check.rs against Stalwart (`a_caldav_check_names_the_default_calendar_the_scheduling_inbox_states`, `a_carddav_check_names_no_default_address_book`); a unit test on the key of a default calendar URL. Google and Graph by reading only: no test credentials here.

## Capabilities moved

- sync: a check names the default calendar and address book.
- sync: a check reports what each source declares it can do.
