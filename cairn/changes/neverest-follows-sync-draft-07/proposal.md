---
cairn: change
id: neverest-follows-sync-draft-07
status: landed
created: 2026-11-01
---

# neverest follows storage-draft-08 and sync-draft-07

## Why

io-pimdir bff5737 (pushed, master, still 0.8.0) follows pimdir d671bbd: storage-draft-08, sync-draft-07, search-draft-08 (io-pimdir cairn/changes/the-crate-follows-storage-draft-08, whose "Not in this change" names this follower). neverest is the owner every other consumer reads behind, so it goes first among the end-to-end checks, though its compile port is independent of theirs.

Built on 2026-11-01 against ../io-pimdir at bff5737 (`cargo check --all-targets`, over the uncommitted work of 2026-10-30):

- Resolution fails before anything compiles: io-pimdir takes rusqlite 0.41, neverest pins `rusqlite = "0.40"` directly (Cargo.toml:71), two `links = "sqlite3"`.
- With rusqlite at 0.41, 26 errors at 33 sites, all named in io-pimdir's CHANGELOG Unreleased:
  - `load::PimdirLoadScope` is `PimdirLoadSelection` (src/offline/storage.rs:20, src/offline/driver.rs, src/msgraph/client/mail.rs:1123).
  - `capability::X_VTODO` is `CALENDAR_COMPONENT_VTODO` (`calendar.component.vtodo`), read only where a row states it (src/offline/capability.rs:18, 106, 150; seven sites in src/offline/driver.rs).
  - `intent::PimdirSubmit` carries no `object` (the message is the row's body, `to_action(message)`) and `source` is a `String`, as are `PimdirCollectionCreate` and `PimdirInvitation` sources (src/offline/submit.rs, src/offline/create.rs:51, tests/submit.rs, tests/intents.rs).
  - `PimdirRemoteSnapshot` gains `narrowed` and `enumerated` (src/offline/remote.rs:212).
  - `PimdirError::Stale` is gone (`Version { found }`, `SyncVersion { found }`); `hash::PimdirHashAlgo`, `hash_algo()` and `open_with_hash` are gone, `PimdirBlobs::open` takes the directory alone (src/offline/driver.rs:50, tests/common/mod.rs:305).
  - `PimdirBlobs::path` answers `io::Result<PathBuf>` (src/offline/driver.rs).
- A second pass will show what type errors hide: the `#[non_exhaustive]` enums (`PimdirError`, `PimdirSyncEvent`, `PimdirEnumerated`), `PimdirListing::Head`, `PimdirSourceStore::upgrade(options, ..)`, `PimdirParked::guard(named, ..)`, `PimdirDrainReport`, `PimdirGcReport` and `PimdirChangeCursor` no longer `Copy`.

## What the compiler does not see

1. **Narrowed rounds** (pimdir SYNC §4, §5). A scoped mail round (`item.filter.since`, `sync --since`) narrowed by a provider filter states `narrowed` on every page of the round and, on its last page, `enumerated`: every handle the collection holds read without a date filter. IMAP narrowing by `SENTSINCE` hands `UID SEARCH ALL`; Gmail narrowing by `after:` hands the label's ids; Graph narrowing by `sentDateTime` hands the folder's message ids. An unscoped round states neither. This is what stops an undated draft below the window, or a message whose `Date` runs ahead of its arrival, from being dropped; it also makes the margin and the no-`Date` carve-out of cairn/spec/sync.md "A provider narrows a scope with a margin" and "A mail sync lists within a scope on the `Date`" the engine's, so both requirements are rewritten rather than patched.
2. **Upgrades settle by the source's policy** (SYNC §6): every `PimdirUpgrade::new` (five sites in src/offline/driver.rs) takes `PimdirUpgradeOptions::from(&sync_options)`; the default settles nothing (`Manual`).
3. **New outcomes reach the report** (cli-output-contract): `Overruled` from `PreferLocal`'s keep and the fill's settle, `parked` from the empty-listing park (a collection the server lists empty no longer drops its bindings without `--accept-absences`), `unanswered`, `rejected_creates`, `unreadable`, `PimdirDrainReport::overruled`; src/sync/report.rs, src/json_schema.rs, the exit code.
4. **Error codes**: io-pimdir no longer raises `store-stale`, `store-newer` or `store-older`; it adds `store-version`, `sync-version`, `store-exposed`, `index-version`, `index-zone-unknown`, `flag-invalid`, `action-malformed`, `conflict-unread` (src/error.rs, src/json_schema.rs).
5. **Every store is reinstalled** (pimdir-drafts-exact): a store any earlier neverest wrote is refused as `store-version` by every profile, never migrated. MIGRATION.md says, in order: run the previous neverest's `sync` until no row is pending (queued writes live only in that store), upgrade, move the store aside, `neverest init` and sync again. The refusal's message says the same and never says to delete.
6. **Modes** (STORAGE §18): readers and producers refuse a store another user can read as `store-exposed`. src/cli/init.rs:70 creates the replica with `fs::create_dir_all` under the umask; confirm io-pimdir restricts it at creation, else create it through `io_pimdir::client::modes::create_dir_all`.
7. **The index** (neverest-builds-the-search-index): an index of another draft is refused as `index-version` and an earlier one rebuilt; `floating_zone` stays io-pimdir's default (`UTC`).

## Decisions for the user

- **Enumerate on every scoped round** (recommended): IMAP costs one `UID SEARCH ALL`; Gmail and Graph page ids only, measured on the test accounts before landing. The alternative, `narrowed` without `enumerated` on Gmail and Graph, still finds absent only what was dated before the round opened, so nothing is wrongly dropped, but a deletion below the window is missed until an unscoped round.
- **Commit the 2026-10-30 work first** (recommended), so this port is a diff of its own.

## Pin

Built against `path = "../io-pimdir"` while porting; at the end `[patch.crates-io]` takes `io-pimdir = { git = "https://github.com/pimalaya/io-pimdir", rev = "bff5737..." }`, as calendula's 84aa00c did, since CI has no sibling checkout. The requirement stays `0.8` until the user releases io-pimdir.
