---
cairn: tasks
change: msgraph-bearer-only
---

# Tasks

- [x] `config`: collapse `MsgraphAuthConfig` to `{ token: Secret }`; delete the
      device-code / client-credentials / client-credentials-cert config structs
      and their defaults; rewrite the auth parsing test bearer-only.
- [x] `msgraph`: delete `auth.rs` (flows, tokens.json 0600, io-oauth usage);
      update `mod.rs` docs.
- [x] `client`: resolve the token command inline in `open`; remove
      `OpenContext` and its plumbing (`init`, `Pool`, cli/init, cli/check,
      driver).
- [x] Deps: drop io-oauth and base64 from Cargo.toml.
- [x] Docs: rewrite the sample msgraph auth block (ortie pattern, OAuth lives
      in ortie).
- [x] CHANGELOG: net-style rewrite of the Graph auth entries.
- [x] fmt, clippy (only the known autoconfig warning), all tests green.
- [x] Cairn: fold the spec, write the log entry, mark landed, archive.
