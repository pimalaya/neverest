---
cairn: log
change: a-sync-stops-cleanly-and-codes-its-failures
date: 2026-11-15
---

# A sync stops cleanly and codes its failures

Owner rulings of 2026-11-15, items 10, 13, 16 and 24 of the merged list, on pimalaya-cli 51ce8b1 and pimalaya-config d5a33cb.

## What landed

- Item 10, signals: `sync::install_shutdown` installs pimalaya-cli's `signal::Shutdown` once the stores are locked; `sync::stopping()` stops the pair loop between collections, `Work::next` (no new spine or batch) and `hydrate_pool` (no new batch). An account not reached is reported failed, "Interrupted before the account ran"; the one stopped in gets `SyncOutput::interrupted`, which `incomplete()` reads, so the run exits 4. The spinner shares the handler, so a second signal restores the cursor and exits 130.
- Item 10, events: `collection-finished` carries `code`, `error` and `sources` (from `Failure`), where `error` was the whole chain as one string.
- Item 13: `main` reports through `error::coded`, `with_code` over `refusal_code`, so a server refusal under a failed command reaches the failure object, as it already reached rows.
- Item 16: a source whose backend the build leaves out fails, when opened, coded `backend-not-built` naming its feature (`SourceBackendConfig::feature`: `dav` for CardDAV and CalDAV, `msgraph` for the three Graph tables). A JMAP source says it cannot be opened yet. `Account` keeps a failed endpoint as a `Failure`, rebuilt by `Failure::to_error`, where it kept a string and lost the code.
- Item 24: `Config::select` codes an unknown name `account-missing`.
- Tests: `an_interrupted_run_is_incomplete`, `a_collection_stopped_short_carries_its_code_and_causes`, and `a_backend_left_out_of_the_build_names_its_feature` (run in a build without `gcal`).
- Not tested end to end: a real SIGINT during a sync.

## Capabilities moved

- [sync](../spec/sync.md): *A signal stops a sync cleanly* added; the events, the backend feature, the account filter and the refusal codes requirements amended.
