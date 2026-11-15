# Migration guide

v0.1.0 and v1.0.0-beta were early releases sitting on top of `email-lib`. v0.2.0 is a full rewrite on top of the I/O-free `io-*` ecosystem, so the CLI, the configuration schema and the sync engine all changed shape.

This page lists the changes most likely to bite when upgrading. The full configuration schema lives in [config.sample.toml](./config.sample.toml), and the exhaustive list of changes in [CHANGELOG.md](./CHANGELOG.md).

## From v0.1 to v0.4

### Highlights

- The binary is synchronous: Tokio is gone, the `io-*` clients using `std::net`.
- **`left` and `right` are gone.** An account names its endpoints and the direction between them: **sources** over one pimdir store, optionally **targets** to copy them to. A configuration still carrying either is refused, naming its replacement.
- **A backend written directly under the account** (`imap.server = "…"`) is sugar for a source named after its protocol, which is the whole configuration for a single-provider account. `sources.<name>.<protocol>.*` is the explicit form, and the only one able to express two sources of one protocol.
- **An account may hold several kinds.** Mail, contacts and calendar sources sit under one account and one store, their collections keyed apart so they never meet.
- **What an account does is its arity plus two flags**, and `collection.namespace` is gone. With no `targets` every source syncs into the local store, isolated from the others; with them the source is copied to each target.
- **`one-way = true` makes the sources authoritative**, so the other side is overwritten rather than merged and nothing is reported as a conflict. The old `left` / `right` pair was a two-way mirror, which is one source and one target with `one-way` left off.
- **`store.retention` and `store.hydration` are gone.** Whether the store keeps bodies is the account's `retain`: true with no target, since the store is the destination, false with targets, a configuration naming both having asked to copy between them. A configuration still carrying either key is refused.
- **`retain = true` alongside a target makes the store a backup** rather than a cache, so `sync --reset` refuses to drop it while it holds anything no server has, and `sync --reset --force` moves it aside.
- A source is a remote. **Local file backends are no longer sync sources**: the local pimdir store is the local replica, so a Maildir or m2dir tree beside it would be a second local copy. Resync from the authoritative server instead.
- **CardDAV** and **CalDAV** are new backends, behind the `dav` cargo feature and in the default set. **Microsoft Graph** is behind `msgraph`, which is not a default while it syncs mail alone. JMAP and Gmail sources parse but cannot be opened yet.
- **Notmuch** is removed.
- **Keyring** and **OAuth** are out of the binary: source secrets from a command instead, such as `pass` or `secret-tool`, and [ortie](https://github.com/pimalaya/ortie) for OAuth access tokens.
- The sync vocabulary is kind-neutral: **collections** and **items** rather than mailboxes and messages. The `mailbox`, `message` and `filters` spellings kept working as config aliases through v0.3.0 and are refused by name after it (see [From v0.3 to v0.4](#from-v03-to-v04) below); the v0.1 `folder` ones never did, and no alias survives in the `--json` report.
- **A sync can exit 3 or 4.** A run that reconciled every collection and still left something waiting for a person (a parked conflict, a refused duplicate `UID`, a rejected write) exits 3 rather than 0 or 1, and one that could not do all its work exits 4. A script treating any non-zero code as failure needs to tell 3 and 4 from 1. Exit 2 is a command line neverest refused.

### Suggested steps

1. Copy [config.sample.toml](./config.sample.toml) next to the old config.toml and port your accounts.
2. Run `neverest account check` to validate every endpoint and read back what each account does, in words.
3. Run `neverest init` to create the store every account shares.
4. Run `neverest sync --dry-run` to inspect the first patch.
5. Drop `--dry-run`, replace the old config, done.

## From v0.3 to v0.4

Two things change at once, and neither is migrated:

- The store follows pimdir storage-draft-11, sync-draft-11 and search-draft-11 (io-pimdir 0.12). A store any earlier neverest wrote, v0.3.0 or a git build, one of draft-10 included, is refused by every profile as `store-version`.
- **Every account shares one store**: `neverest/` under the state directory itself (`$XDG_STATE_HOME/neverest` on Linux, `~/Library/Application Support/neverest` on macOS, `%LOCALAPPDATA%\neverest` on Windows), no longer one directory per account under it. A neverest directory still holding the per-account stores an earlier neverest kept is refused as `store-layout`, naming each, and so is a `store.root` store whose neverest.json an earlier neverest wrote, or kept beside its pimdir.db.

What only an old store holds is the work it queued (a send, an edit or a create staged by a frontend) and its retained items, so reinstall in this order:

1. With the neverest that wrote the stores, run `neverest sync -a <account>` for each account until nothing is queued: the run exits 0, its report lists no `parked` action, no `submitted` entry left parked and no conflict, and `neverest drain -a <account>` applies nothing more. Settle or cancel what stays parked.
2. Upgrade neverest.
3. Move every old store out of the neverest directory, deleting nothing: `mv $XDG_STATE_HOME/neverest $XDG_STATE_HOME/neverest-v0.3` on Linux moves them all at once. Move a `store.root` directory away too, or point `store.root` at a new path.
4. Run `neverest init`, which adds every account to the new shared store, then `neverest sync`, which lists every collection again and downloads its bodies.

Keep the moved stores until the new one holds what you expect: their retained items are only there.

A store another user can read is refused too, as `store-exposed`: one `neverest init` creates is its user's alone, and an older one moved back in place needs `chmod 700` on its directory and `chmod 600` on its pimdir.db.

What else changes:

- **`-a/--account` is a filter**, and repeatable. Without it, `sync`, `check`, `drain` and `init` run over every account, one after the other; `index` builds every store the accounts use. `conflict show`, `conflict resolve` and `sync --reset` act on one account, so `-a` names it when there are several. `default = true` is read and ignored, with a warning; drop it at your convenience.
- **One run, one lock.** The store is locked for the whole run, so two accounts of one store never sync at once, and a second run waits up to a minute then reports the store busy (exit 4). An account that needs its own schedule (a cron of its own, a frontend-triggered run that must not wait) names its own `store.root`.
- **An account's failure is its own.** A run goes on with the next account and exits 4 when one failed. Under `--json`, `sync`, `drain`, `init` and `check` print one result listing every account (`{"accounts": [...]}`, a sync's report under `accounts[].report`), and `index` one listing every store (`{"stores": [...]}`); `sync` announces each account with an `account-started` event.
- **Store ids carry the account.** A source id is `<account>/<source>` and a collection id `<account>/<source>/<name>` (`work/imap/INBOX`, where it was `imap/INBOX`), so two accounts with an `imap` source never share a collection. A report still names an endpoint as the configuration does (`imap`). An account name holding `/` is refused.
- **`sync --reset` starts one account over**: it drops that account's collections and syncs them again, every other account untouched. `--force` drops the conflicts, unpushed edits and retained items they hold rather than moving a store aside; a queued or parked action still refuses the reset until a drain applies it or its frontend cancels it.
- **Frontends** (himalaya, calendula, cardamum): point `pimdir.root` at the neverest directory and name the account with `pimdir.account`. A collection named by its id gains the account (`himalaya -f work/imap/INBOX`); a role (`inbox`, `sent`) keeps working.
- **neverest keeps its own files under the store's local/**: neverest.json, sync.lock and the mark of a send being handed over, where they stood beside pimdir.db. A script reading or locking one names it there (`local/sync.lock`).
- **A send that may have been delivered is `unconfirmed`**: its report entry carries `unconfirmed: true` and an `error` beginning `unconfirmed: `, the state every pimdir frontend reads, where a git build wrote `uncertain`.
- **A queued move of an item waiting for a decision is parked**, nothing moved, its error naming the diverging body, where it was applied: settle the conflict with `neverest conflict resolve`, then move the item again.
- `neverest check` names, under `orphans`, an account the store holds that no configured account names: renaming an account's TOML key leaves its collections under the old name until the key is renamed back.
- **A failing row splits its error.** Under `--json`, a `rejected`, `submitted`, `intents`, `parked` or `unreached` row and a failed patch entry carry `code`, `error` (the outermost message) and `sources` (its causes), where `error` held the whole chain; `rejected[].reason` is now `rejected[].error`. Read `code` (`permission-denied`, `quota-exceeded`, `rate-limited`, `recipient-refused`, `null`) rather than matching the text, or join `error` and `sources` with `: ` for the old string.
- **Exit statuses follow the Pimalaya exit contract.** A sync that waits for a person exits 3, where v0.3.0 exited 2, and one that could not do all its work 4, where it exited 3; `check` with an endpoint down exits 3. Exit 2 is now a command line neverest refused, printed under `--json` as a failure object coded `usage-invalid`.
- **A configuration key neverest does not read is refused**, at every level, naming it: a typo in a `sources.<name>` or `targets.<name>` table, or at the root, used to be ignored, and a source naming two backend tables kept the first. A key an earlier neverest read is refused as the file loads, in any account, naming its replacement and this page: `left`, `right`, `folder`, `envelope`, the account-level `collection`, `collection.namespace`, `store.retention`, `store.hydration`, and the flat `smtp.login` and `smtp.password`.
- **`sync` stops cleanly on Ctrl-C or SIGTERM**: it prints the result of what it did, the account it stopped in marked `interrupted`, and exits 4; a second signal ends it at once. Under `--json`, `collection-finished` carries `code`, `error` and `sources`, where `error` held the whole chain.
- **`conflict show --json` prints a body that is not UTF-8 as `{data, size, mime}`**, `data` in base64, where it printed a string with U+FFFD replacements. A script reading `base`, `local` or `remote` takes a string or that object.
- **The configuration wizard is gone**: `neverest configure`, the offer a bare `neverest` made without a configuration, and the `wizard` cargo feature. Write the configuration by hand from [config.sample.toml](./config.sample.toml); a command finding none fails naming the path it looked for.
- **`-c` takes one path, and `NEVEREST_CONFIG` is gone**, as in every Pimalaya CLI. Before: `neverest -c public.toml:private.toml sync`, or `NEVEREST_CONFIG=public.toml:private.toml neverest sync`. After: `neverest -c public.toml -c private.toml sync`, the files merged in that order. A path holding a `:` is now read whole. A script exporting `NEVEREST_CONFIG` passes `-c "$NEVEREST_CONFIG"` instead, one flag per file.
- **A bare `neverest` without a configuration fails**, exit 1, naming the missing file and the sample, where it printed the help and exited 0; at a terminal it prints the welcome giving the commands that create the file. With a configuration it still prints the help. **`completion`, `manual` and `json-schema` with no argument fail on stdout, exit 1**, where they printed their help on stderr and exited 2: name a shell, a command, or `--dir`.
- **`check` capabilities read `level`.** Before: `.accounts[].sources[].capabilities[].support`. After: `.accounts[].sources[].capabilities[].level`, the same `full`, `partial` or `none`.
- **A queue row is `row`.** Before: `.accounts[].applied[].id` and `.accounts[].parked[].id` of `drain`, `.accounts[].report.parked[].id`, `.accounts[].report.submitted[].id`, `.accounts[].report.intents[].id` and `.accounts[].report.overruled[].action` of `sync`. After: `.row` in each. A sync's parked row names its kind `kind` (`.parked[].action` before), spelled as the store does (`set-flags`), and a rejected flag push's `action` reads `set-flags` (`set flags` before).
- **A failed account splits its error** under `sync` and `drain`, as the rows already did, and so does a failed index refresh: `.accounts[].error` is the outermost message and `.accounts[].sources` its causes, where `error` held the whole chain. Join `error` and `sources` with `: ` for the old string. `init` keeps the whole chain, its `sources` naming the endpoints it opened.
- The missing configuration reads "No configuration found at <path>, start from the sample: <url>" and an unknown account "Account `x` not found, the configuration holds: a, b", still coded `account-missing`: match the code, not the text.
- **A one-way account no longer renames a folder on the server** when a frontend asks, unless its source sets `collection.rename = true`: unset, `collection.rename` is false on a `one-way` account and true otherwise. Before: a frontend's `collection-rename` on a one-way source was performed. After: refused up front, the source declaring `collection.rename: none`.
- **`check` is `account check`**, as in every Pimalaya CLI. Before: `neverest check`. After: `neverest account check`, the same report; `check` still runs it, hidden from the help, and its JSON schema stays under `neverest-check` beside `neverest-account-check`. `neverest account list` is new: the accounts and the backend tables each writes, under `--json` pimalaya-cli's `{name, default, backends, notBuilt}` rows.
- **The `mailbox`, `message` and `filters` spellings are refused by name**, config aliases from v0.2.0 to v0.3.0. Before: `imap.mailbox.create = false`, `imap.message.delete = false`, `imap.collection.filters.include = ["INBOX"]`. After: `imap.collection.create = false`, `imap.item.delete = false`, `imap.collection.filter.include = ["INBOX"]`, in every backend table, under `sources.<name>` and `targets.<name>` too. The account-level `message` table is `item` (`item.filter.since`), and the account-level `mailbox` table, refused as `collection` until now, is refused under its own name: write `<protocol>.collection.filter`.
- **Microsoft Graph is one `msgraph` table**, as in cardamum, calendula and carillon. The session keys (`auth`, `user-id`, `tls`, `alpn`, `proxy`) stay in `msgraph`, written once; each domain's `collection`, `item`, `flag` and `pool-size` move under `msgraph.mail`, `msgraph.contacts` or `msgraph.calendar`, a domain with none of them written `= true`. **Nothing in the store moves**: the sources keep their names (`msgraph`, `msgraph-contacts`, `msgraph-calendar`, or the name you gave one), so the first run finds every collection and resumes where the last stopped. Each spelling that is now refused:
  - A mail-only `msgraph` table, naming no domain. Before: `msgraph.auth.token.command = [...]`. After: the same, plus `msgraph.mail = true`.
  - `msgraph-contacts`. Before: `msgraph-contacts.auth.token.command = [...]`, `msgraph-contacts.item.delete = false`. After: `msgraph.auth.token.command = [...]` (written once for every domain), `msgraph.contacts.item.delete = false`, or `msgraph.contacts = true` with no key of its own.
  - `msgraph-calendar`. Before: `msgraph-calendar.auth.token.command = [...]`. After: `msgraph.auth.token.command = [...]`, `msgraph.calendar = true`.
  - `collection`, `item`, `flag` and `pool-size` directly in `msgraph`. Before: `msgraph.collection.filter.include = ["Inbox"]`, `msgraph.item.delete = false`, `msgraph.flag.update = false`, `msgraph.pool-size = 2`. After: `msgraph.mail.collection.filter.include = ["Inbox"]`, `msgraph.mail.item.delete = false`, `msgraph.mail.flag.update = false`, `msgraph.mail.pool-size = 2`.
  - A named source or target. Before: `sources.people.msgraph-contacts.auth.token.command = [...]`. After: `sources.people.msgraph.auth.token.command = [...]`, `sources.people.msgraph.contacts = true`, one domain a named source, which keeps its name (`people`). `targets.<name>` reads the same.
  - Tables naming different tokens, users or proxies cannot share one session: keep them apart as named sources under the names they had, which keeps their ids. Before: `msgraph-calendar.auth.token.command = ["ortie", "token", "show", "-a", "calendars"]` beside a mail `msgraph` table of another token. After: `sources.msgraph-calendar.msgraph.auth.token.command = ["ortie", "token", "show", "-a", "calendars"]`, `sources.msgraph-calendar.msgraph.calendar = true`.
  - Under `--json`, `account list` reads `msgraph` in `backends`, and an `account check` row `backend: "msgraph"`, for every Graph source, where they read `msgraph-contacts` or `msgraph-calendar`; the row's `source` still names the domain's source (`msgraph-contacts`).
- **A Gmail source refuses every keyword but `$Important`**, before a frontend queues it, where it let any through with a warning and the push dropped it. Before: `himalaya flag add -m <account>/gmail/INBOX 42 -f seen -f custom` was queued, `\Seen` reached Gmail and `custom` was lost. After: it is refused naming `mail.flags.keywords`; queue `-f seen` alone. An add, copy or move into a Gmail label of a message carrying a keyword (a `$Forwarded` from an IMAP folder) is refused too, as one carrying `\Answered` already was: remove the keyword first. Such an action queued before the upgrade parks at the next `neverest drain` or `sync`, listed under `parked`: cancel it in its frontend and queue it again without the keyword.
- **A removed configuration key reads the family sentence**: "The `left` table is removed, <why>: <what to write instead>, see MIGRATION.md", where it read "`left` and `right` are gone: … See <URL>". The same keys are refused: match the key, not the text.

### From a git master build

No v1.0.0 was ever released: what was prepared under that number ships as v0.2.0.

If you ran a build from master, its store is refused as `store-version`: reinstall it as [From v0.3 to v0.4](#from-v03-to-v04) says above.

A build from before v0.2.0 may have written the send channel as a flat `smtp.login` and `smtp.password`: write one `smtp.sasl` mechanism instead, spelled as under `imap.sasl` (`smtp.sasl.plain.username` and `smtp.sasl.plain.password`, or `smtp.sasl.login.*` for the LOGIN exchange those keys ran).

## From v1.0.0-beta to v0.2

Everything in [From v0.1 to v0.2](#from-v01-to-v02) below applies too: v1.0.0-beta only added a few cosmetic changes on top of v0.1.0.

### CLI

| v1.0.0-beta | v0.2.0 |
|---|---|
| `doctor <account>` (aliases `check`, `check-up`, `checkup`) | `check -a <account>` |
| `--debug` (alias for `RUST_LOG=debug`) | `--log-level debug` (alias `--log`) |
| `--trace` (alias for `RUST_LOG=trace` plus a backtrace) | `--log-level trace` |

### Configuration

| v1.0.0-beta | v0.2.0 |
|---|---|
| `folder.filters = "..."` | `collection.filter = "..."` |
| `envelope.filters.{before,after}` | removed |
| `left\|right.folder.aliases.<name>` | removed |

`color-eyre`'s spantrace and backtrace output is gone: errors flow through `anyhow` and pimalaya-cli's error report. `tracing` is replaced by `log`.

## From v0.1 to v0.2

### CLI

| v0.1.0 | v0.2.0 |
|---|---|
| `synchronize <account>` | `sync -a <account>` |
| `check-up <account>` | `check -a <account>` |
| `configure <account>` | `configure`, which generates a new account rather than editing one |
| (none) | `init -a <account>`, mandatory once, before the first sync |
| `-f` / `--include-folder` | `-m` / `--include-collection` |
| `-x` / `--exclude-folder` | `-x` / `--exclude-collection` |
| `-A` / `--all-folders` | `-A` / `--all-collections` |
| `-o {plain,json}` | `--json` |
| `-C` / `--color` | removed, color follows the terminal |
| `RUST_LOG=...` only | `--log-level` (alias `--log`), `--log-file <PATH>` |

The positional `<account>` argument becomes an optional `-a` / `--account <NAME>` flag, falling back to the entry marked `default = true`.

### Configuration

| v0.1.0 | v0.2.0 |
|---|---|
| `folder.filter = "..."` | `collection.filter = "..."` |
| `folder.filter.{include,exclude}` | `collection.filter.{include,exclude}` |
| `folder.aliases` | removed |
| `left.backend.type = "imap"` plus `host` / `port` / `encryption` / `auth` | `imap.server = "..."` plus `imap.tls.*` and `imap.sasl.*` |
| `left.backend.type = "maildir"` plus `root-dir` | removed |
| `left.backend.type = "notmuch"` | removed |
| `left.folder.permissions.{create,delete}` | `<protocol>.collection.{create,delete}` |
| `left.flag.permissions.update` | `<protocol>.flag.update` |
| `left.message.permissions.{create,delete}` | `<protocol>.item.{create,delete}` |
| `left` / `right` | `sources.<name>.<protocol>.*` and `targets.<name>.<protocol>.*`, plus `one-way = true` to copy rather than merge |
| account-level `collection.filter` | `<protocol>.collection.filter`, on the source it filters |
| `store.retention`, `store.hydration` | removed; the account's `retain` |
| `smtp.*` at the account root | `smtp.*` again (`smtp.server` plus `smtp.tls.*` and `smtp.sasl.*`, spelled exactly as the IMAP ones), or `sources.<name>.smtp.*`; at most one source per account may declare it |
| keyring entries | `{ command = ["pass", "show", "..."] }`, or any other secret manager |
| `auth.type = "oauth2"` | SASL `oauthbearer` or `xoauth2`, the token coming from [ortie](https://github.com/pimalaya/ortie) |
| `envelope.filter.{before,after}` | removed |

The sync cache is now the pimdir store at `neverest/<account>/` under the platform's state location: `$XDG_STATE_HOME` on Linux and the BSDs, `~/Library/Application Support` on macOS, `%LOCALAPPDATA%` on Windows. Override it with `store.root`. The presence of its database is the single source of truth for "this account is initialized".

New account-level settings are `one-way` and `retain` (what the account does), `store.purge-after` (the retention sweep) and `connections`. Per source, `<protocol>.item.update` gates in-place body edits and `<protocol>.pool-size` overrides the connection pool.

Nothing of the v0.1 cache is read: `init` builds the store from scratch and the first sync fills it.

Neverest stamps the account's mode beside the store and compares it every run. Turning `one-way` on over an account that synced both ways is refused once, the run that follows being the one that discards what the previous mode was merging.

`neverest sync --accept-mode` says you meant it and is remembered. A `retain` that drops from true to false, and a change in the number of endpoints, are reported and do not block.
