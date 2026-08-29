---
cairn: tasks
change: a-source-drains-its-own-namespace
---

# Tasks

- [x] Take the namespace in `drain_queues` and skip the collections outside it.
- [x] Report skipped actions in the drain's info line.
- [x] Cover it: a caldav drain leaves a mail collection alone, the imap drain
      applies it.
- [x] Namespace the collection ids the drain tests use, as production does.
- [x] CHANGELOG.md.
- [x] Fold the delta into cairn/spec/sync.md and log it.
