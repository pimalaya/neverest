---
cairn: tasks
change: a-gmail-keyword-is-refused-up-front
---

# Tasks

- [x] src/offline/capability.rs: a Gmail source's `mail.flags.keywords` `none`, with its detail
- [x] Tests: `a_gmail_source_refuses_any_keyword_but_important_at_the_gate` (src/offline/driver.rs: a declared Gmail store refuses `$Custom` naming `mail.flags.keywords`, queues `$Important` with no partial); `a_gmail_source_states_its_important_and_junk_rows` reads `none` and the detail. Both fail with the `partial` row restored (checked by reverting the row)
- [x] CHANGELOG (**BREAKING**, upgrade list), MIGRATION.md
- [x] Fold into cairn/spec/sync.md, log, archive
