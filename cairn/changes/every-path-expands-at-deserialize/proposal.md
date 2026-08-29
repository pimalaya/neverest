---
cairn: change
id: every-path-expands-at-deserialize
status: landed
created: 2026-08-29
---

# `tls.cert` was the one path key that never expanded

## Why

`store.root` is shell-expanded by its deserializer, so `store.root = "~/store"` reaches every call site already resolved and none of them has to remember. `tls.cert` was declared as a bare `Option<PathBuf>` beside it, and the one call site that reads it hands it straight to the TLS layer. A user writing `imap.tls.cert = "~/ca.pem"`, the spelling the sample invites by using `~` for `store.root` three sections down, got a lookup for a literal `./~/ca.pem`: no such file, and an error naming the wrong thing.

The same field, declared the same way, is a live bug in himalaya and himalaya-tui, and the pattern that avoids it is not "expand it at the call site" but "expand it where a call site cannot be reached without it".

## What

- `TlsConfig::cert` deserializes through `shell_expanded_path_opt`, keeping `#[serde(default)]` so an absent key never reaches the deserializer and stays `None`.
- The sample says the path is shell-expanded.
- A round-trip test covers both path keys, the absent case, and that a re-serialized document reloads to the same value.
