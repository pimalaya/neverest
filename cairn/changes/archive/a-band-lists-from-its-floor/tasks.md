---
cairn: tasks
change: a-band-lists-from-its-floor
---

# Tasks

- [x] Regression tests failing on the code before: `a_band_round_keeps_a_message_received_after_its_date` (src/offline/driver.rs, a fake Gmail inbox searching by arrival through the scripted wire's new responder) and `a_band_round_keeps_a_message_sent_after_its_date` (src/msgraph/client/mail.rs, a fake Graph row whose `Date` header lies before its `sentDateTime`)
- [x] `client::floor_of`; Gmail `scope_query` reads the floor alone; Graph's `enumerate` lists by the floor
- [x] The Graph tests counting band rows follow: a widening lists the coverage again
- [x] Fold into cairn/spec/sync.md, log, CHANGELOG
