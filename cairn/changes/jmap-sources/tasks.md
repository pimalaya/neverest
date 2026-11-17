---
cairn: tasks
change: jmap-sources
---

# Tasks

Not started: scheduled after the next major release (owner, 2026-11-15). The status moves from `draft` to `active` when section 0 is answered.

## 0. Decide

- [ ] J1: a mailbox is keyed by its name path (check a name holding a `/` on Fastmail and Stalwart first).
- [ ] J2: a delete from an email's last mailbox destroys it.
- [ ] J3: one `jmap` table with domains (`jmap.mail`, `jmap.contact`, `jmap.calendar`), the shape Graph took in neverest abb2a95 (2026-11-15); the keys now directly in `jmap` move under `jmap.mail`, the old spellings refused by name.
- [ ] J4: a contact write carries `ifInState`, one delta and one retry on `stateMismatch`.
- [ ] J5: ship on the whole `headers` list, move to the one-header property when io-jmap has it.
- [ ] Re-read every fact of the proposal against the tree of the day: io-jmap's version, the seam of src/client.rs, the Android connector.

## 1. io-jmap (first, each its own release)

- [ ] A property variant asking for one raw header (`header:<name>:asRaw:all`), with its answer typed.
- [ ] A streamed blob download beside `blob_download`.
- [ ] `Calendar/set` (for the calendar source only).

## 2. Mail wiring

- [ ] Cargo: `jmap = ["dep:io-jmap", "io-jmap/client"]` in `default`, with the TLS and `vendored` forwards.
- [ ] `JmapAccount` in src/account.rs (server, auth, TLS, identity, drafts mailbox), built by `SourceAccountBackend::resolve`, the "cannot be opened yet" refusal removed; `SourceBackendConfig::feature` answers `jmap`.
- [ ] `Client::Jmap`, every `match` arm of src/client.rs, `open` and `init`. `media_type` is `message/rfc822`, `global_handles` true.
- [ ] `sends_natively()` true for a `jmap` mail source; an `smtp` table beside it refused at load.
- [ ] `supports` (src/offline/capability.rs) and `check` (src/cli/check.rs) for a JMAP source, from `myRights` and the configured rights.
- [ ] The src/client.rs header: "JMAP configs parse but do not open yet" removed.

## 3. Mail adapter (src/jmap/client.rs, src/jmap/run.rs)

- [ ] Session: `session_get`, no redirect, limits read from the core capability, the pool bounded by `maxConcurrentRequests`.
- [ ] `list_collections`, roles and the default collection from `Mailbox/get`; create, rename and delete through `Mailbox/set`.
- [ ] `enumerate`: the full round with its state taken first, the delta over `Email/changes` read once a run (the run cache, as `GmailRun`), the projection on a mailbox with what left it stated vanished, `cannotCalculateChanges` as `Listed::CursorRejected`.
- [ ] `heads`: the delta head of an open round.
- [ ] Scope: `after` with a margin, `narrowed`, the unnarrowed id listing on the last page.
- [ ] `fetch_summaries` from the raw `headers` through io-pimdir's `derive_meta`; tick the JMAP line of a-message-is-keyed-by-the-formats-reading.
- [ ] `fetch_bodies` and `get_item_stream` through the blob download.
- [ ] `update_flags` as keyword patches.
- [ ] `add_item_stream` (blob upload, `Email/import`), `move_item`, `move_items`, `copy_item`, `delete_item` (J2), `find_message`; `update_item_stream` refusing; a write on an email gone counting as done.
- [ ] Send: the `submit` intent through `EmailSubmission/set`, at most once (src/offline/submit.rs).
- [ ] Rate refusals through src/throttle.rs.

## 4. Mail tests

- [ ] Unit tests on a fake JMAP server: the projection of a change on two mailboxes, the vanish of a left mailbox, the restart on `cannotCalculateChanges`, the scope margin, an import refused, a send answered twice.
- [ ] Integration on Stalwart (tests/stalwart.sh): two replicas of one account, an add crossing, a flag crossing back, a move, a delete from the last mailbox, a send.
- [ ] A JMAP endpoint and an IMAP endpoint of one Stalwart account in one store: the folders pair, a message is one item.
- [ ] A live suite on Fastmail, read-only (the account is pull-only): session, listing, one full round, one delta.

## 5. Contacts (`jmap.contact`, source `jmap-contacts`)

- [ ] The `jmap.contact` domain and its sugar (source `jmap-contacts`), the session shared with `jmap.mail`, kind `text/vcard`.
- [ ] Collections from `AddressBook/get`, items from `ContactCard/query` and `ContactCard/changes`, the vCard through io-jmap's `vcard` feature, the revision a hash of the card's JSON.
- [ ] Writes through `ContactCard/set` with `ifInState` (J4); the three-way merge of a refused edit on the next run.
- [ ] Tests: fake server, Stalwart, Fastmail read-only.

## 6. Land (each source)

- [ ] Fold the delta into cairn/spec/sync.md, log entry, status `landed` when the last planned source lands.
- [ ] README backend list, config.sample.toml (the "no JMAP backend is compiled in yet" note removed, the keys documented), MIGRATION.md, CHANGELOG `Added`.
- [ ] fmt, clippy `-D warnings` with `jmap` alone and with all features.

## 7. Calendars (`jmap.calendar`, source `jmap-calendar`), not scheduled

- [ ] Opens when the calendars draft is published or a provider serves it: its own requirement in the delta, `Calendar/set` in io-jmap, the Android app's calendar/jmap.rs as the starting point.
