# neverest architecture

Read the [Pimalaya ARCHITECTURE](https://github.com/pimalaya/.github/blob/master/ARCHITECTURE.md) first: it describes the conventions every Pimalaya repository shares (layering, the sans-I/O coroutine approach, command and config conventions, code style, licensing). This document only covers what is specific to neverest, and assumes you know that shared context.

If a statement here conflicts with the code, the code wins; please flag it.

## Where neverest fits

neverest is an **application**, the top layer of the Pimalaya stack: a CLI that synchronizes emails between two backends. It has no library target (only `main.rs`) and writes no protocol or storage logic of its own. It is a thin shell that drives the sans-I/O libraries below it:

- [io-email](https://github.com/pimalaya/io-email): the cross-protocol email domain API, exposed as the blocking `EmailClientStd` that every side is opened onto;
- [io-imap](https://github.com/pimalaya/io-imap), [io-jmap](https://github.com/pimalaya/io-jmap), [io-gmail](https://github.com/pimalaya/io-gmail), [io-msgraph](https://github.com/pimalaya/io-msgraph), [io-m2dir](https://github.com/pimalaya/io-m2dir): the protocol/storage backends;
- [pimconf](https://github.com/pimalaya/pimconf): account discovery (PACC, Thunderbird autoconfig, RFC 6186 SRV);
- [pimalaya-cli](https://github.com/pimalaya/cli), [pimalaya-config](https://github.com/pimalaya/config), [pimalaya-stream](https://github.com/pimalaya/stream): shared CLI plumbing (clap args, printer, logger, wizard prompts), TOML config loading, and the blocking I/O runtime (TLS, SASL).

All real I/O lives in those libraries; neverest consumes their blocking `*Std` clients and only orchestrates them and renders results.

## The sync model

An account pairs two **sides**, `left` and `right` (a `Side` tag, `side.rs`). There is no implicit direction: the labels are symmetric and the diff is bidirectional. Each side selects exactly one backend (`SideConfig`: `imap`, `jmap`, `gmail`, `msgraph` or `m2dir`); `client.rs` opens that backend onto a fresh io-email `EmailClientStd`, so the rest of the engine talks to both sides through one uniform shared-API client regardless of protocol.

Sync is a 3-way reconcile against a persisted snapshot:

1. **Snapshot.** Per account, a cache at `$XDG_CACHE_HOME/neverest/<account>/state.json` (`sync/state.rs`, `StateSnapshot`) records the last-known mailbox set, per-message entries, and the per-side backend state tokens (IMAP/JMAP sync markers). Its mere presence is the single source of truth for "this account is initialized": `sync` refuses to run without it, `init` refuses to run with it.
2. **Diff.** `sync/diff.rs` walks every mailbox surviving the filter, compares each side's live listing against the snapshot, and emits `MailboxHunk` / `EmailHunk` work units (`sync/hunk.rs`). The diff is pure: it takes snapshots plus listings and returns hunks, pre-gated by the per-side permissions so a forbidden mutation never becomes a hunk.
3. **Apply.** `sync/pool.rs` fans the hunks out across per-side connection pools and applies them, then `sync/report.rs` aggregates created / updated / deleted mailboxes, flags and messages into the printed `SyncReport`.

`sync.rs` is the entry point that wires those three stages together and, on IMAP sides, issues a single `SELECT` per mailbox batch (the per-side clients run with `auto_select = false`, so the engine pre-selects once instead of paying a `SELECT` per `STORE`/`FETCH`/`COPY`).

## Connection pools

Each side opens **N independent clients** (`sync/pool.rs`, `Pool`), one per worker, so hunks apply concurrently. Workers run in `(left, right)` pairs so a `Copy` hunk always holds both a read end and a write end. The per-side pool size comes from `<side>.<backend>.pool-size`, defaulting per backend: IMAP 8, the HTTP backends (JMAP, Gmail, Microsoft Graph) 4, m2dir 8; IMAP warns above a soft cap. The worker count is `min(left.len, right.len)`.

## Per-side permissions

Every backend table carries `mailbox.{create,delete}`, `flag.update` and `message.{create,delete}` (all default `true`). They gate what the engine may mutate on that side: a hunk that would violate the policy is dropped from the patch and surfaced in the report, so a side can be made read-only for any subset of operations.

## Commands

The command tree (`cli/`, one module per subcommand) is small and sync-focused:

- `init`: opens both sides (surfacing credential/network errors up front) and writes the empty cache snapshot that marks the account initialized.
- `sync`: runs the diff + apply described above; `-d`/`--dry-run` prints the patch without applying it, `--reset` drops cached state (whole snapshot, or just `--include-mailbox` ones) before running.
- `check`: opens both sides and lists mailboxes, a cheap probe whose value is surfacing config/credential errors.
- `configure` (alias `cfg`): re-runs the wizard over an existing account, pre-filling current values.
- `manuals`, `completions`: man pages and shell completions.

Output follows the Pimalaya stdout/stderr rule: all data and errors go to stdout through `pimalaya_cli::printer` (with `--json` switching to JSON), stderr carries logs only. Each subcommand is a clap-derived struct carrying its own arguments with an `execute(self, printer, config_paths)` method; `cli/main.rs` is the single dispatch point.

## Configuration and the wizard

Config is loaded by pimalaya-config from the first existing canonical path (or the `-c` / `NEVEREST_CONFIG` override), with later paths deep-merged on top. The schema (`config.rs`) is multi-account: named `[accounts.<name>]` blocks, each with a `left` and a `right` side plus account-level mailbox filters. Each side is one backend sub-table whose cross-cutting options (permissions, pool size) live inside it.

The IMAP and JMAP blocks carry discoverable server settings; the Gmail and Microsoft Graph blocks (`gmail`, `msgraph`) instead carry `user-id` (default `me`), TLS settings, `alpn` (default `["http/1.1"]`) and an `auth.token` holding the OAuth 2.0 bearer access token, the only authorization those REST APIs accept (supplied raw or via a `token.command`). They need no server address (the API host is fixed) and no refresh logic (the token is supplied externally).

When no config file exists, `Config::load_or_wizard` runs the interactive wizard (`wizard/`) to bootstrap one. The wizard discovers a remote IMAP or JMAP server (PACC, then Thunderbird autoconfig, then RFC 6186 SRV) for one side and prompts for a local m2dir root for the other. Gmail and Microsoft Graph sides are configured by hand: they take a bearer token rather than discoverable settings, so the wizard leaves them to the config file (and `configure` keeps an existing Gmail/Graph side untouched).

## Module layout

```
src/
  main.rs            entry point: parse Cli, build printer, dispatch
  cli/               clap parser + one module per subcommand (init, sync, check, configure)
  config.rs          TOML schema: Config, AccountConfig, SideConfig (imap/jmap/gmail/msgraph/m2dir)
  client.rs          open()/init(): build an io-email EmailClientStd for one side
  side.rs            Side tag (left/right) + client pair selection
  sync/              the sync engine
    sync.rs          run entry point wiring diff + apply
    diff.rs          pure 3-way diff, emits hunks pre-gated by permissions
    hunk.rs          MailboxHunk / EmailHunk work units + their apply
    pool.rs          per-side connection pools, paired (left, right) workers
    state.rs         StateSnapshot cache (mailboxes, messages, per-side tokens)
    report.rs        SyncReport aggregation
  wizard/            first-run interactive config bootstrap (discover, pacc, autoconfig, srv, edit, account)
```
