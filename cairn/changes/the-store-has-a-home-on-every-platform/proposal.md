---
cairn: change
id: the-store-has-a-home-on-every-platform
status: landed
created: 2026-09-08
---

# The default store location was Linux-only

## Why

`replica_dir` resolved the default store root through `dirs::state_dir()`, and that function answers `Some` on Linux and the BSDs alone. The `dirs` documentation says so in its own table: macOS and Windows are both a dash.

So on macOS and on Windows every command that touches an account failed, unless the configuration named `store.root` by hand. `store_dir` falls back to `replica_dir` whenever that key is unset, and `init`, `sync` and all three `conflict` verbs go through it. `configure` does not, which is the worst ordering available: the wizard runs, writes a working configuration, and the next command dies.

It did not fail silently. It failed with `Cannot resolve XDG state directory`, which names a specification the platform does not implement, on a platform that was never going to have one.

The release matrix builds `aarch64-darwin`, `x86_64-darwin` and `x86_64-windows`, so this ships three binaries that cannot run out of the box.

Reported by shurizzle in pimalaya/neverest#24, against `StateSnapshot::path` before the store refactor moved that resolution into `replica_dir`.

## What

Only the **base** varies by platform. The layout under it stays `neverest/<account>/` everywhere, the store being a directory rather than a file, so a store is addressable by the same relative path whoever wrote it.

- Linux and the BSDs keep the XDG state directory.
- macOS and iOS take `~/Library/Application Support`, not `~/Library/Caches`: dropping the store costs a full resync, and it holds the only local copy when the account retains bodies.
- Windows takes `%LOCALAPPDATA%`, not the roaming `%APPDATA%`: the store holds message bodies and a SQLite index, neither of which belongs on a profile that syncs between machines.

`store.root` overrides all of it, as before.

## Not in scope

**No WebAssembly arm.** The reporting patch gated one, and neverest links rusqlite, opens TCP sockets and takes `fs4` file locks, so it has never built for `wasm32` and cannot. Gating a target that fails to compile anyway adds an arm nothing reaches.

**No per-platform layout.** The patch also moved macOS and Windows to `<base>/neverest/state/<account>.json`, which fitted the single-file state snapshot it was written against. The store is a directory now, so only the base moves.
