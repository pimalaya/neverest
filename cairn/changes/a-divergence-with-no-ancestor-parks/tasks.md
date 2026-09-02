---
cairn: tasks
change: a-divergence-with-no-ancestor-parks
---

- [x] Read the hub's own cross-source conflict flag and the diverging body it carries
- [x] Snapshot the flag before each round, so a divergence parks once and a settled one is not re-parked
- [x] Park the target's placement with the source's body as its own and the target's recorded against it, at the revision its pull observed
- [x] Write no base body, so the automatic merge leaves an ancestor-less divergence alone rather than settling it
- [x] Prove that marking the target stops the projection pushing the source's body over it
- [x] Report it once, count it among the outstanding, and exit 2
- [x] Skip it under `one-way`, where the source decides, and on a dry run
- [x] Test: two endpoints holding one identity under two bodies park it, keep both bodies, and settle either way round
