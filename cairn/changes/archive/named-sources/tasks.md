---
cairn: tasks
change: named-sources
---

# Tasks

## Decided

- **No compatibility, in the config or on disk.** v1 is unreleased and nothing runs on the current shape, so a removed key is refused with its replacement rather than aliased or ignored, and a store written before this change is not read.
- **`collection.namespace` sits under the source's `collection` table**, beside its permissions and filter.

## Config

- [x] `AccountConfig` gains `sources: HashMap<String, SourceConfig>`. `left` and `right` are refused at load, naming `sources.left` / `sources.right` and the shared `collection.namespace` that reproduces a mirror.
- [x] Direct-backend sugar: `imap`, `carddav`, `caldav`, `jmap`, `gmail`, `msgraph` under the account fold into `sources.<protocol>`. Declaring one both ways is an error, not a merge.
- [x] `SideConfig` becomes `SourceConfig`; `SideBackendConfig` becomes `SourceBackendConfig`. The enum shape is unchanged, it is already right.
- [x] `collection.namespace` on the source, defaulting to the source name.
- [x] Move `collection.filter` from the account onto the source.
- [x] Delete `store.retention` and `store.hydration` from the config, and refuse a file still carrying them, naming the derived value that replaces it.
- [x] Reject two sources declaring `smtp` at load, replacing the "left wins" tiebreak.
- [x] Validate at load: at least one source; every source name a usable pimdir source id.

## Engine seam

- [x] `src/offline/mod.rs`: `source_id(side)` becomes the source's name; delete the `Side`-to-id mapping.
- [x] Collection ids carry kind and namespace everywhere they are built.
- [x] `src/offline/driver.rs`: iterate the source map rather than a left/right pair, grouping by `(kind, namespace)`, and derive what the store keeps from that group's size and pairing (one source: every body; exactly two on a streamable pairing: none; otherwise: what crossed).
- [x] Relay: source and target are named, not left and right (in `driver.rs`; `pipe.rs` itself was already name-free). Relay only at exactly two sources, and three or more are refused outright rather than kept-what-crossed.
- [x] The derived value gates fetching only. Never delete a stored body because the derivation moved; leave it unreferenced for `pimdir gc` or `sync --reset`.
- [x] `src/offline/submit.rs`: the send channel resolves from the one source that declares it.

## Side removal

- [x] Delete `src/side.rs`. Report hunks (`src/sync/hunk.rs`, `src/sync/report.rs`) key on the source name, text and `--json`.
- [x] `sync --source <name>` narrows which sources run; the account still names the database.
- [x] Anything reading "left"/"right" in CLI output or error messages.

## Report

- [x] `src/sync/report.rs`: a per-kind-and-namespace section stating source count and what the store keeps, rendered in text and `--json`, present even when the run wrote nothing. Objects and bytes held where bodies are kept.
- [x] Persist the previous run's derived value per kind so a run can name a transition. Not in store meta: io-pimdir exposes no consumer key-value surface, so it lives in `neverest.json` beside the store (`src/offline/state.rs`), which also carries the collection-id layout guard: old value, new value, the configuration change behind it, what became unreferenced, and the command that reclaims it.
- [x] `src/cli/check.rs`: report the same derivation, computed from the config alone, so it answers before a first sync and while a remote is down.

## Wizard

Scope narrowed on the owner's call after the proposal was written: the wizard keeps its old shape (bare invocation, no config found, one account, one backend, offline usage), and only its spelling changes.

- [x] Write one account with one source, through the direct-backend sugar.
- [x] Never write a namespace; a mirror stays hand-configured.
- [~] ~~Offer every reachable service whose backend is compiled in, instead of one kind per run.~~ Dropped: everything past one source is manual config.

## Tests

- [x] A config carrying `left` / `right`, or `store.retention` / `store.hydration`, is refused with a message naming its replacement.
- [x] The sugar and its expansion produce the same source id and the same collection keys.
- [x] Two isolated same-kind sources: two collections, no cross push.
- [ ] Two sources sharing a namespace: a create propagates, a delete propagates, `item.delete = false` on one holds its tombstone. The tombstone half is covered by `a_side_that_may_not_delete_keeps_the_tombstone`; the propagation halves need a two-source harness and are only exercised by the ignored live suites.
- [x] A CardDAV book and a mailbox both named `Default` do not collide: covered by the hub-id round trip and by a namespace claimed by two kinds being refused, which is what would have made them collide.
- [x] Two sources declaring `smtp` is refused at load.
- [x] The derivation, table-driven: one source keeps every body, two streamable keep none, two DAV keep what crossed, three keep what crossed.
- [ ] Adding a second source to a hydrated namespace leaves every stored body on disk and reports the transition. The naming half is covered in `state.rs`; that no body is dropped is enforced by construction (the derivation gates fetching only, nothing deletes) but is not asserted by a test.
- [x] A config carrying `store.retention` is refused, naming the derived value. (Refused rather than warned-and-ignored: see the decision above.)
- [ ] The report states what the store keeps on a run that wrote nothing, and `check` states it with no server reachable. Both are implemented and the derivation itself is unit-tested; the rendering paths are not asserted end to end.

## Docs

- [x] `config.sample.toml`: rewrite around sources, with the direct-backend sugar as the headline shape and the explicit `sources` table shown for a mirror and a fan-in.
- [x] `MIGRATION.md`: the v0 to v1 path already ports accounts by hand, so it needs sources instead of sides, and nothing about compatibility.
- [x] `CHANGELOG.md` under `[Unreleased]`, as a net diff.

## Fold

- [x] Fold the delta into `cairn/spec/sync.md` and write `cairn/log/<date>-named-sources.md`.
