---
cairn: tasks
change: one-seam-for-the-wire-name
---

- [x] `wire_name` is one free function, delegated to by the method and by `display_name`
- [x] `phase2_hydrate` fetches under the wire name and caches under the hub id
- [x] `push` strips the move destination
- [x] Test: a hub id becomes the name its server knows, prefixes and other namespaces intact
