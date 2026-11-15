---
cairn: tasks
change: live-submission
---

- [x] `ehlo_domain` greets with the loopback address literal
- [x] `tests/stalwart2.sh` publishes port 25 for both instances (2525, 2526)
- [x] `tests/submit.rs`: stage a body, enqueue a `submit` intent, sync
- [x] Test: the run reports the submission and the queue row is gone
- [x] Test: the delivered message comes back into the store on the next sync
- [x] The marker is per-run, so a message an earlier run delivered cannot pass this one
