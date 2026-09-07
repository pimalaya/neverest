---
cairn: delta
change: the-store-has-a-home-on-every-platform
---

# Delta

## ADDED Requirements

### Requirement: The default store root resolves on every supported platform
An account whose configuration names no `store.root` SHALL resolve its store under a per-platform base: the XDG state directory on Linux and the BSDs, `~/Library/Application Support` on macOS and iOS, `%LOCALAPPDATA%` on Windows. A platform SHALL NOT be left resolving through a mechanism it does not implement, since every account-touching verb reaches this and the wizard does not, so the failure lands after a configuration was successfully written.

Only the base SHALL vary. The layout under it SHALL be `neverest/<account>/` on every platform, the store being a directory rather than a file, so one relative path addresses a store whoever wrote it. `store.root` SHALL override the whole resolution.

A roaming or cache location SHALL NOT be chosen where the platform distinguishes one: the store holds message bodies and a SQLite index, is the only local copy when the account retains bodies, and costs a full resync when dropped.

#### Scenario: A store resolves without a hand-written root
- GIVEN an account naming no `store.root`
- WHEN it is initialized on Linux, on macOS or on Windows
- THEN the store resolves under that platform's own base, at `neverest/<account>/`
