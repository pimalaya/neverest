---
cairn: log
change: side-owned-send-channel
landed: 2026-08-07
---

# The send channel moved into the side

`smtp` sat at the account root, beside `left` and `right`, reading as if it
belonged to both sides or to neither. It belongs to one: a submission server
completes the provider the side already names, and whether a side needs one at
all depends on its backend (Graph sends through `sendMail`, JMAP will send by
itself once its submission lands). The channel now lives in the side.

```toml
[accounts.example]
left.imap.server = "imaps://imap.example.org:993"
left.smtp.server = "smtps://smtp.example.org:465"
left.smtp.login = "user@example.org"
left.smtp.password.command = "pass show mail"
```

**Schema** (`config.rs`): `SideConfig` is now a table pairing a flattened
`SideBackendConfig` (the former `SideConfig` enum: imap / jmap / gmail /
msgraph) with an optional `smtp` block; `AccountConfig.smtp` is gone, and the
account root refuses it (`unknown field `smtp``, `deny_unknown_fields`). New
helpers: `SideConfig::new` (a side with no channel of its own) and
`sends_natively` (Graph today, JMAP when it lands). Flatten costs the side
table its `deny_unknown_fields`, but a mistyped backend key is still refused
(no variant matches the flattened data); two backends on one side now resolve
to the first written instead of erroring, which the tests pin.

**Resolution** (`offline/driver.rs`): `open_send_channel` walks
`account_config.sides()` in order and takes the first side offering a channel,
its own `smtp` table before its native send, then opens it. `left` therefore
wins on an account where both sides could send, a statable rule replacing the
implicit "the root table beats every side". The pick is decided before any
borrow of the live sessions, so the Graph arm can hand out the session of the
side it chose.

**Wizard** (`wizard/discover.rs`, `wizard/edit.rs`): `configure` returns a
`SideConfig` carrying the discovered submission endpoint (the `Configured`
side+smtp pair is gone), so the IMAP + SMTP pair discovery returns as one
service lands as one side. `neverest configure` keeps the channel the side
already had when a re-run discovers none.

Verified: 45 tests green (new coverage for the channel under the side, the
refused root table, a natively-sending Graph side, a mistyped backend key), fmt
clean, clippy clean except the pre-existing `incompatible_msrv` warning in
`cli/sync.rs`.

Breaking: a configuration keeping `smtp` at the account root fails to parse.
The fix is a one-line move under `left` (or `right`). This lands before 1.0.0.

Spec updated: `sync` (MODIFIED: "The Outbox is local-only and flushes through
the send channel" is now side-owned with a first-side-wins rule; ADDED: "A side
pairs one backend with its send channel").
