---
cairn: change
id: gpeople-batch-get
status: active
created: 2026-10-18
---

# Google People bodies read in batches

## Why

- **One read a person.** `GpeopleClient::fetch_bodies` (src/gpeople/client.rs:291-310) calls `person` (line 284) once per id, each a `people.get` with the vCard fields (`GPEOPLE_PERSON_VCARD_FIELDS`). People bills each against the per-user per-minute "Critical read requests (Contact and Profile Reads)" quota, so scanning an address book burns it: the A2 audit run of 2026-10-11 failed `google_contacts_sync_both_ways` on `People API returned HTTP 429: gpeople gave up after being throttled`, twice in the same minute, right after the io-gpeople live suite, and passed three minutes later in 39 s. `people:batchGet` reads up to 200 persons a call.
- **The cleanup draws the same quota.** The live test's cleanup (tests/google.rs:157-183) finds the run's cards with `contacts_search` (line 170), which fails on the same 429, so the cleanup cannot run when it is needed most and `neverest-live-*` cards stay on the test account.

## What

- `fetch_bodies` reads its ids through `people:batchGet` (`resourceNames`, at most 200 a call, the same `personFields`), each person written to its sink and committed with its etag as it is read; a resource the batch answers not found is a person gone, never an error. Requires io-gpeople to expose the batch get (`people.getBatchGet`): add it there first if 0.5 has none.
- The throttle and its Google refusal classes stay as they are: a batch is one request.
- tests/google.rs cleanup: delete the cards the run created by the resource names it recorded, rather than by search; a search kept only as a fallback.

## Not in scope, described

- Pacing People against its per-minute quota the way Gmail is paced (gmail-reads-in-paced-batches): fewer calls should be enough; measure first.
