---
cairn: log
change: the-account-groups-by-kind
date: 2026-11-17
---

# The account groups its settings by kind

`conflict.merger` became `contact.merger` and `calendar.merger`: `conflict resolve --interactive` runs the merger of the conflict's kind, refuses naming the key when the kind has none, and refuses mail, whose bodies are immutable. One command per account had been handed cards and events alike, so an account holding both could not name tcard and tcal. `item.filter.since` became `mail.filter.since` and Graph's `msgraph.contacts` domain `msgraph.contact`, the source id staying `msgraph-contacts`.

- src/config/mail.rs, contact.rs, calendar.rs replace `ItemSyncConfig` (src/config/filter.rs keeps `resolve_since`) and src/config/conflict.rs.
- src/cli/conflict.rs: the merger is picked by `Kind`; test `a_card_is_not_handed_to_the_calendar_merger`, checked failing with cards routed to `calendar.merger`.
- Capability moved: sync (Deciding is a command, never a run; the Graph and scope requirements renamed their keys).
