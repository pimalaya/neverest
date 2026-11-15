---
cairn: tasks
change: credentials-resolved-once
---

# Tasks

- [x] pimalaya-config: add `command::CommandConfig`, the configured shape of a
      command, and carry it in `Secret::Command` in place of a built
      `std::process::Command`, which is neither comparable, hashable nor
      clonable and has forgotten the shape it came from.
- [x] pimalaya-config: add `SecretResolver`, memoizing spawns on that shape,
      compared as written and never across the two forms.
- [x] pimalaya-config: log each spawn at `debug` with its elapsed time, and
      neither the value nor the arguments.
- [x] Add `crate::account`: `Account`, `SourceAccount`, `SourceAccountBackend`,
      and the per-protocol connect material.
- [x] Take the resolver in `SaslConfig::try_into_sasl` and
      `DavAuthConfig::try_into_dav_auth`.
- [x] `client::open`, `client::init` and `Pool` take a `SourceAccount`.
- [x] Driver: resolve once in `run`, thread the account through `run_local`,
      `run_targets`, `run_pair`, `open_source_contexts` and the submit drain.
- [x] `connect_smtp` takes an `SmtpAccount`.
- [x] `check`, `init` and the wizard's connection tests resolve before opening.
- [x] Cover the invariant: one command named by four endpoints spawns once.
- [x] CHANGELOG.md in both repositories.
- [x] Fold the delta into cairn/spec/sync.md and log it.
