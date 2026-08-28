---
cairn: log
change: credentials-resolved-once
landed: 2026-08-28
---

# Credentials stopped being resolved at the socket

`client::open` took a `SourceConfig` and spawned its `password.command` inside,
and `Pool` kept that configuration so every lazily-opened connection could spawn
it again. A posteo account with an IMAP source at `-j 4`, a CardDAV one, a CalDAV
one and a send channel therefore ran `gpg` six times per sync, four of them
concurrently, for what is one or two `pass` entries. None of it was logged, so a
locked agent read as several silent seconds before the first `Opened 1
connection(s)`.

**New seam** (`account.rs`): `Account` holds one account's endpoints with every
secret resolved, built once by `Account::resolve` and read by name through
`Account::get`. `SourceAccount` carries a `SourceAccountBackend`
(`ImapAccount` / `DavAccount` / `MsgraphAccount`) and its optional
`SmtpAccount`, each holding exactly the argument list of the backend's
`connect`. None of them derive `Debug`. A backend this build cannot open is
refused here rather than at connect. `Account::resolve` carries the "Resolving
credentials" spinner itself, rather than each of the three commands that call
it, and reports how many endpoints failed so a resolution that lost one does not
read as a clean one.

**Connection layer** (`client.rs`): `open` and `init` take `&SourceAccount`,
and `Pool` stores the account instead of the config, so `workers()` opens a
connection with no command left to spawn. `connect_smtp` (`offline/submit.rs`)
takes an `SmtpAccount` the same way.

**Driver** (`offline/driver.rs`): `run` resolves once, right after the mode
check and before any endpoint opens, then threads the account through
`run_local`, `run_targets`, `run_pair`,
`open_source_contexts` and the submit drain. `open_source_contexts` clones the
resolved account per thread where it used to clone the config. A per-endpoint
resolution failure surfaces through `Account::get`, which lands in the existing
per-source error path, so a stale `pass` entry for calendars leaves mail alone.

**Memo** (pimalaya-config): `SecretResolver` spawns each distinct command once,
keyed on the command as the configuration wrote it. That key exists because
`Secret::Command` now carries a `command::CommandConfig` (a shell line, or a
program and its arguments) instead of a built `std::process::Command`: the
configured shape derives `Clone`, `Eq` and `Hash`, where the built command has
none of the three and has forgotten which shape it came from, so comparing two
meant rebuilding a shape out of them and cloning one meant copying it field by
field. Keeping what was written deletes both, and with them the environment and
working directory a built command could carry, which the clone dropped and the
serializer discarded anyway. The two shapes are never compared across: a shell
line and its argv spelling are two commands, whatever they end up running.
`Secret::get` logs each spawn at `debug` with its elapsed time, and neither the
value nor the arguments. The `secret` feature gained `log`. Consumers reach it
through `[patch.crates-io] pimalaya-config.path = "../config"` until it is
published, and the payload change makes that release a breaking one.

**Callers**: `check` and `init` resolve the whole account once and open each
endpoint from it; the wizard's connection tests resolve the single configuration
they hold through `SourceAccount::resolve` / `SmtpAccount::resolve`, and
`wizard/secret.rs` builds a `CommandConfig` variant where it built a `Command`.

Verified: 124 unit tests green, including a new one pinning the invariant (four
endpoints naming one command spawn it once) and six in pimalaya-config (memoized
spawn, distinct commands, the two shapes resolving on their own, the shape
round-tripping as written, and the argv spelling of a shell line comparing
unequal while running the same thing). fmt and clippy clean, and the feature
matrix builds with no backend, with each backend alone, and with every pair that
carries the send channel.

No configuration change: this moves where a configured command runs, not what
may be configured. Six `gpg` invocations per run become one for the account
whose four tables name one entry.

Spec updated: `sync` (ADDED: "Credentials are resolved once per run"; MODIFIED:
"Microsoft Graph is a first-class source", its token now resolved once per run
with the rest rather than once per opened client).
