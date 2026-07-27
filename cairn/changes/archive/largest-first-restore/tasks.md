---
cairn: tasks
change: largest-first-restore
---

- [x] `EmailRemote` carries a `handle → size` map; `with_progress` takes it.
- [x] `fetch_full` orders largest-first when sizes are present, UID order when not.
- [x] Driver `hydrate_all` builds the size map from each placement's store meta
      (`meta_size`) — no round trip; two-source `hydrate_copies` passes empty.
- [x] Build, fmt, clippy clean; 15 tests pass.
- [x] Live Stalwart: a big message (UID 3 among 1–5) is fetched first in the
      batched command (largest-first confirmed).
- [ ] Fold delta into `cairn/spec/sync.md`; write log entry.
