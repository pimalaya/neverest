---
cairn: tasks
change: a-gateway-error-is-waited-out
---

# Tasks

- [x] `Request::retries`, `google_refusal` and `GoogleWaits::throttled` take 502 and 504 as 503; the docs that name 503 name them too
- [x] Tests: a 502 and a 504 retried on a read and not on a create, Google reading a 502 as a burst (failing on the code before)
- [x] fmt, clippy, test; CHANGELOG; fold into spec/sync.md, log
