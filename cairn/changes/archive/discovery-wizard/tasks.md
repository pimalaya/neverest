---
cairn: tasks
change: discovery-wizard
---

# Tasks

- [x] `wizard/search`: parallel compose discovery (`compose_all_within`, 8s
      deadline, `NEVEREST_DNS_RESOLVER`), `Discovered` / `DiscoveredKind` /
      `AuthCaps`, provider short-circuit, ranking.
- [x] `wizard/secret`: password and OAuth-token pickers over the shared
      `pimalaya-cli` keyring module.
- [x] `wizard/imap_smtp`: CAPABILITY-probed SASL mechanism list, credential
      prompts, IMAP connection test, discovered SMTP (reuse-or-reprompt, LOGIN
      only) and its connection test.
- [x] `wizard/msgraph`: user id + bearer token prompts, connection test.
- [x] `wizard/discover`: welcome banner, email-only prompt, derived account
      name, configuration list, one-side (`left` + implicit store) account,
      write.
- [x] `wizard/edit`: `neverest configure` re-runs the same flow, preserving
      `default`, `store`, `mailbox`, `message`, `connections` and a hand-written
      `right`.
- [x] Delete `wizard/pacc.rs`, `wizard/autoconfig.rs`, `wizard/srv.rs`,
      `wizard/account.rs`.
- [x] Deps: bump io-pim-discovery to 0.4 (`compose_all_within`, released); `pimalaya-cli`
      features drop `jmap`, add `smtp`.
- [x] Docs: CHANGELOG entry; sample config / README wizard wording if it
      describes the old prompts.
- [x] fmt, clippy, all tests green.
- [x] Cairn: fold the spec, write the log entry, mark landed, archive.
