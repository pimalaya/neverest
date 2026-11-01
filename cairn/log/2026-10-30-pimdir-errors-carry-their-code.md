---
cairn: log
change: pimdir-errors-carry-their-code
date: 2026-10-30
---

# pimdir errors carry their code

Phase A of the pimdir stable-release plan: neverest moved to the io-pimdir API of 2026-10-30 (errors with a stable code, the producer staging its own body, the typed `submit` intent), pinned to the local checkout until it is pushed.

## What landed

- Cargo.toml: `io-pimdir = { path = "../io-pimdir" }` under a NOTE saying it returns to a git rev once the io-pimdir changes of 2026-10-30 are pushed; Cargo.lock changes the io-pimdir entry alone.
- src/offline/remote.rs: `PimRemote` and the body fetches stream into `PimdirBlobStage` (`store.blob_stage()`); `HydrateSink` keeps no hasher of its own, `commit()` answering the object. src/offline/driver.rs passes the owner's stage down from each store it opens and reads bodies through its read side.
- src/conflict.rs, src/offline/driver.rs `merge_conflicts`: a settled or merged body is staged by the producer that enqueues it (`stage_body`).
- src/error.rs: `ErrorCode`, the store codes neverest relays by io-pimdir's spelling, and `pimdir_error`, used at every store open (`open_store`, `init`, the reset, the conflict commands) and conflict write; src/main.rs evaluates through `ErrorReport::<ErrorCode>::eval_coded`, and `neverest-error` describes the codes.
- src/offline/state.rs: the sidecar refusals say what `sync --reset --force` does (moves the store aside, deleting nothing) rather than "let it resync".
- src/offline/submit.rs: `SubmitMeta` is gone, the payload read by `PimdirSubmit::from_payload`; one naming another body than the row pins parks.
- io-pimdir's `#[non_exhaustive]` enums needed no new arm: every match on them already had a wildcard.
- Tests: tests/drain.rs `a_store_a_newer_neverest_wrote_fails_with_its_code`; src/offline/submit.rs `a_payload_reads_alike_whatever_its_key_order`, `a_payload_naming_another_body_parks`; src/error.rs two tests on the code mapping; the test writers stage through a producer (`stage_body`) or `blob_stage()`, tests/intents.rs and tests/submit.rs queueing a send through `PimdirSubmit::to_action` as himalaya does.

## Evidence

On the old code `a_store_a_newer_neverest_wrote_fails_with_its_code` printed `{"backtrace":null,"error":"The store was written by a newer neverest; upgrade neverest to sync this account",...}`, no `code`; `a_payload_naming_another_body_parks` sent the pinned body (the check disabled, `prepare` answered `Ok`).

## Capabilities moved

- sync: a store error carries its code (added); a body reaches the blob tree under a lock (added); a queued submission is a `submit` queue intent (modified: read by `PimdirSubmit`, `object` required and the pinned one).
