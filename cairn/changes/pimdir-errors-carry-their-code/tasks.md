---
cairn: tasks
change: pimdir-errors-carry-their-code
---

# Tasks

- [x] Wait for io-pimdir's `PimdirError::code()`, Display rewording, `PimdirSubmit` and the staged bodies; pin it (local path until the io-pimdir changes of 2026-10-30 are pushed).
- [x] Bodies through the write side io-pimdir hands a lock holder: the connectors stream into `store.blob_stage()` (`HydrateSink` keeps no hasher, `commit()` answering the object), a settled or merged conflict body goes through `producer.stage_body`, the tests stage through a producer or `blob_stage()`.
- [x] JSON error codes from `PimdirError::code()`: src/error.rs `ErrorCode`, `pimdir_error` at every store open and conflict write, `main` evaluating through `ErrorReport::<ErrorCode>::eval_coded`, `neverest-error` describing the codes. Test, failing first: tests/drain.rs `a_store_a_newer_neverest_wrote_fails_with_its_code` (`left: Null`, no `code` on the old code).
- [x] No "delete the store" advice left in any printed error; the sidecar refusals (src/offline/state.rs) name `sync --reset --force` and what it does, not "let it resync".
- [x] `submit` read through `PimdirSubmit::from_payload`, `SubmitMeta` gone; a payload naming another body than the row pins parks (`a_payload_naming_another_body_parks`, sent the pinned body on the old code); the payload himalaya wrote and the one it writes now, its keys sorted, read alike (`a_payload_reads_alike_whatever_its_key_order`).
- [x] Wildcard arms for io-pimdir's `#[non_exhaustive]` enums: none needed, every match on them already had one.
- [x] Spec folded, log written, CHANGELOG.
