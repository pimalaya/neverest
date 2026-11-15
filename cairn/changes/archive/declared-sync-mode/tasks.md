---
cairn: tasks
change: declared-sync-mode
---

# Tasks

## Configuration

- [x] Add `targets` (a named map, like `sources`), `one-way` and `retain` to
      `AccountConfig`; keep one struct with `deny_unknown_fields` so a typo is an
      unknown-field error rather than a mode flip.
- [x] Validate the arity matrix, refusing every other combination by naming the
      cell reached and the nearest legal one.
- [x] Refuse `retain = false` with no targets: the local store is the destination,
      so it would sync to nowhere.
- [x] Default `retain` to false when targets are declared, true when they are not.
- [x] Remove `collection.namespace` from the schema; default the hub namespace to
      the source name internally.
- [x] Refuse `collection.namespace` by name, as `left` and `right` are, pointing
      at `targets` and `one-way`.

## Engine

- [x] Replace `StoredBodies` with the declared pair: `retain` decides bodies,
      `one-way` decides authority. Drop the three-state derivation and
      `StoredBodies::derive`.
- [x] Make streaming a crossing an internal choice of the `one-way` path, taken
      when the pairing allows it, never a user-visible state.
- [x] Implement one-way resolution: the `sources` side wins, the target is
      enumerated but never authoritative, and no conflict is recorded.
- [x] Keep N sources isolated from each other in the no-targets cases.

## Safety

- [x] Stamp the mode triple (arity, `one-way`, `retain`) into `NeverestState` at
      `init` and at `sync --reset`.
- [x] Refuse a run whose stamped `one-way` moved from false to true, naming the
      one-time acknowledgement that records the new mode.
- [x] Report, without blocking, a `retain` that moved from true to false (stored
      bodies stay until `pimdir gc`) and a bare arity change.
- [x] Have `init` state the account's behaviour in words, counting what is on a
      target and not on the source when `one-way` is set.
- [x] Document that `--reset` destroys data rather than a cache once `retain` is
      on, and guard it accordingly.

## Reporting

- [x] Remove the per-run store report and the `bodies` map in `NeverestState`,
      keeping the persisted mode in its place.
- [x] Reword `check` to state the account's mode in plain language.
- [x] Narrow `sync --source` by source name rather than by namespace.

## Docs

- [x] config.sample.toml: the matrix, `retain`, and the removal of
      `collection.namespace`.
- [x] CHANGELOG.md and MIGRATION.md.
- [x] Fold the delta into cairn/spec/sync.md and log it.
