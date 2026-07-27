---
cairn: tasks
change: mail-meta-schema
---

# Tasks

- [x] `remote.rs`: widen `MetaSummary` to the `v: 1` schema (`v`, `message_id`,
      `subject`, `from`, `to`, `date`, `size`); omit absent optionals.
- [x] Emit it from `envelope_meta` (enumerate) and `parse_headers` (streamed,
      threading the stream's octet length in as `size`).
- [x] Document the schema in `pimdir/SPEC.md` §13.
- [x] Tests: both writer paths produce parseable `v: 1` JSON; absent optionals
      omitted (not `null`).
- [x] `nix develop --command cargo test --bins`; `cargo fmt`.
- [x] Fold `delta.md` into `cairn/spec/sync.md`; add `cairn/log` entry; mark
      change `landed` and archive.
