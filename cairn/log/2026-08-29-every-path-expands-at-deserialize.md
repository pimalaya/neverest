---
cairn: log
change: every-path-expands-at-deserialize
landed: 2026-08-29
---

# Every path expands at deserialize

`store.root` was shell-expanded by its deserializer; `tls.cert` was declared bare beside it and handed to the TLS layer exactly as written. `imap.tls.cert = "~/ca.pem"` therefore looked for a literal `./~/ca.pem`, and failed naming a file the user never wrote. The sample invites that spelling: it writes `store.root` with a `~` three sections further down.

## What landed

`TlsConfig::cert` deserializes through the existing `shell_expanded_path_opt`, keeping `#[serde(default)]` so an absent key never reaches the deserializer and stays `None` rather than expanding an empty path. The sample says the path is shell-expanded. A round-trip test covers both path keys, the absent case, and that a re-serialized document reloads to the same value.

## Why not at the call site

There is one reader today, and adding the expansion there would have worked. It is the wrong place: the next reader has to remember, and this key is proof that a reader does not. `shell_expanded_path_opt` stays hand-rolled for now, carrying a TODO naming the shared `pimalaya-config` helper it should be, which ortie hand-rolls identically.

## Capabilities moved

- **sync**: a path-valued configuration key now expands at deserialize by requirement, not by habit.
