---
cairn: log
change: the-store-has-a-home-on-every-platform
landed: 2026-09-08
---

# The default store location was Linux-only

`replica_dir` resolved the default store root through `dirs::state_dir()`, which answers `Some` on Linux and the BSDs alone. On macOS and Windows every account-touching verb failed with `Cannot resolve XDG state directory`, naming a specification those platforms do not implement, unless the configuration set `store.root` by hand. `configure` was the one verb that did not reach it, so the wizard wrote a working configuration and the next command died.

The base is now the only part that varies: XDG on Linux and the BSDs, `~/Library/Application Support` on macOS and iOS, `%LOCALAPPDATA%` on Windows. The layout under it stays `neverest/<account>/` everywhere, so one relative path addresses a store whoever wrote it, and `store.root` still overrides all of it.

The neither-cache-nor-roaming choices are deliberate: the store holds bodies and a SQLite index, is the only local copy under `retain = true`, and costs a full resync when dropped.

The two non-Linux arms are compiled by no Linux build, so they were type-checked out of tree against dirs 7 before landing.

Reported by shurizzle in pimalaya/neverest#24, against `StateSnapshot::path`, which the store refactor had since replaced. That patch also carried a `wasm32` arm and a per-platform file layout; neither was taken, and the proposal says why.

Capability moved: **sync**, one added requirement on the default store root.
