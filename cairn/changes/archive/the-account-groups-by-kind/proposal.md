---
cairn: change
id: the-account-groups-by-kind
status: landed
created: 2026-11-17
---

# The account groups its settings by kind

## Why

`conflict.merger` named one command per account, and `conflict resolve --interactive` handed it every kind, only the file extension telling cards from events. An account holding contacts and calendars (one `msgraph` table with both domains, `gpeople` beside `gcal`, `carddav` beside `caldav`) could name tcard or tcal, not both: whichever it named failed on the other kind, and the only way out was a wrapper script dispatching on the extension.

The account's other per-kind key sat under a generic name too: `item.filter.since` bounds mail and nothing else, and `item` held nothing more. Graph's domain table spelled the kind `contacts` beside `mail` and `calendar`.

## What

The account root carries one table per kind, named in the singular as Graph's domains are:

- `mail.filter.since`, where `item.filter.since` was.
- `contact.merger` and `calendar.merger`, where `conflict.merger` was. `conflict resolve --interactive` runs the merger of the conflict's kind, refuses naming the key when that kind has none, and refuses mail outright.
- `msgraph.contact`, where `msgraph.contacts` was. The source the sugar names stays `msgraph-contacts`, so no store moves.

v1 already refuses every 0.x configuration, so the old spellings are unknown fields like any other, with no refusal by name.

## Not in scope

The scope refusal beside contacts and calendars is unchanged: `mail.filter.since` still refuses an account whose run syncs a contact or calendar endpoint, the scope being one per run.

the-merger-is-configured-once keeps its intent, now over the `contact` and `calendar` tables.
