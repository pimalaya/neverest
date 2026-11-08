---
cairn: tasks
change: a-quota-refusal-keeps-the-send-queued
---

# Tasks

- [x] Settle the replies that mean a rate or a quota, from the providers' own pages, and write the list into delta.md: Gmail (`550 5.4.5`, support.google.com/a/answer/3726730), Exchange Online SMTP AUTH submission limits, Fastmail; and whether a reply without an enhanced code is read by its wording
- [x] Regression test first, failing on 4edcb06: in tests/intents.rs, a new `Smtp` variant of the scripted server (`RefuseData`) answers the end of `DATA` with `550 5.4.5 Daily user sending limit exceeded`; the row stays queued and `submitted[0].parked` is `false`; a second queued send is not attempted. No server, no Docker
- [x] Unit tests on the classifier: `5.4.5` transient, `550 5.1.1` (unknown recipient) still permanent, a 5xx with no enhanced code permanent unless its wording is in the set
- [x] `reply_of` (src/offline/submit.rs) returns the reply text with the code; `smtp_disposition` reads both; a quota refusal stops the run's further sends as a failure with no reply does
- [x] Graph: check Microsoft's throttling and sending-limit pages for a quota answered as 403; none documented, `sendMail` answers 202 before transport
- [x] CHANGELOG (Fixed)
- [x] Fold delta.md into cairn/spec/sync.md, log, land
