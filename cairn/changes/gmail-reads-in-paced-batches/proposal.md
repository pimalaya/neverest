---
cairn: change
id: gmail-reads-in-paced-batches
status: landed
created: 2026-10-13
---

# Gmail reads in paced batches, once a run

Self-contained. Ports the Android client's Gmail findings (android `cairn/changes/google-sync`, `gmail-first-sync-stall`, `rust/src/client/gmail.rs`, `throttle.rs`) to neverest, whose job stays to sync everything: these are ordering and pacing, never caps.

## Why

- **One request a read.** A round page read the metadata of its unbound ids one `messages.get` after the other, a body one raw get each: about 5 reads a second on one connection, latency-bound. A 100,000-message account costs 200,000 round trips.
- **The per-minute quota is not recognised.** Gmail's refusal reads `Quota exceeded for quota metric 'Total Query Cost' and limit 'Units per minute per user'`, its reason `rateLimitExceeded` in `errors[]`. The throttle read the message for `Rate Limit Exceeded` and the status, so this 403 failed the run. Its concurrency refusal (`Too many concurrent requests for user`, 429) carries the same reason; only the wording tells them apart. A daily quota was read as no throttle at all, failing every request until Google's midnight.
- **No minute budget.** The pacing held 200 units a second, about 40 reads, where the binding quota is the minute's: 6,000 units a user for a Google project today (read in the console of MOA's project, 2026-10-09).
- **Labels pay twice.** A message under `INBOX` and a user label was read under each, and each label read its own history (`history.list?labelId=`), every changed message read again per label.
- **A write on a gone message fails.** Removing a label from, or starring, a message deleted since answers 404, and the push failed rather than converge.

Measured by the Android client on the test account (2026-10-10): batches of 10 sent one after another drew no refusal in 2,250 reads, 25 drew 1.2 %, 50 drew 19 %; two batches of 50 side by side drew 17 to 33 refusals each (429 concurrency). Gmail billed a `messages.get` 5 units; Google documents 20 and 15,000 a minute.

## What

- **Refusals by reason** (`src/throttle.rs`, Google part): `google_refusal` reads io-gmail and io-gpeople's `reasons`, `detail_reasons` and `google_status`, and the message (io-gcal keeps the message only). A burst (429, 503, a rate-limit 403) waits 1 to 2 s, or the `Retry after` Google states; a per-minute quota (the wording names `per minute` or `PerMinute`) waits 2, 3, 4, 5 s, then the next minute plus up to a second, in the run; a daily quota (`dailyLimitExceeded`, `per day`) gives the source up until the next midnight in America/Los_Angeles, the report naming it under `throttled`. `Throttle::call` is untouched: a per-request `GoogleWaits` turns the class into the wait it takes.
- **A minute's budget** (`Throttle::paced_per_minute`): a sliding sixty seconds of quota units beside the per-second bucket, `gmail.units-per-minute` (6,000 by default) in the units Gmail bills (5 a read); a batch takes the sum of its reads. Choosing the billed units over the documented ones: at 20 a read the default would be 300 reads a minute, a 100,000-message account taking eleven hours; refusals that slip through are waited out by reason.
- **Batched reads** (`src/gmail/batch.rs`, a `Batcher` trait the tests fake): metadata, sizes and raw bodies through `POST www.googleapis.com/batch/gmail/v1` on a second session of the client, `gmail.batch-size` reads a batch (10), one batch in flight across the source (`gmail.batches-in-flight`, 1), sequential. Per inner answer: read; 404 gone, never an error; a burst or a missing part sent again at the head of the next batch after 1 to 2 s; a minute refusal likewise after 2 to 5 s, one wait a batch, growing while batches stay refused, then the next minute; a daily one gives the source up; six tries a message, then the source gives up for the last wait. A raw batch holds about 8 MiB by size estimate, a message over 5 MiB is read alone, and each body is written to its sink as its batch lands and dropped, its base64 dropped once decoded.
- **One read a run** (`src/gmail/run.rs`, shared by the source's connections through `GmailAccount`): the metadata of a message carrying two or more synced labels is kept (5,000 at most, oldest out), so the next label listing it names it without a read, its flags taken from that label's own flag listings, which postdate its checkpoint; sizes read with the metadata are kept (100,000 at most) for the body reads, an unknown size asked by a minimal batch first.
- **History once a run**: one `history.list` with no `labelId` from the earliest checkpoint asked, kept for the run and projected on each label: a record names the label, or the collection holds the message, or an arrival names no label. A delta reuses a message's labels only when read after the history was. An expired history (404) still restarts that label's round.
- **Writes converge**: a single modify, a delete from `TRASH`, answered 404 is done; a `batchModify` answered 404 (undocumented for unknown ids) is done again one message at a time, each 404 skipped.

## Not in scope, described

- **One body for a message filed under two labels.** Each collection's placement owns its object, and hydration asks each collection for its bodies, so a message under two labels downloads twice. Serving the second from the blob already stored needs the driver (`hydrate_batches`, `HydrateFeed`) to group targets by handle across collections of a source whose handles are global (Gmail), and is left to the driver ordering work (n4-order) to avoid crossing it.
- **Sizes from the store.** The driver knows each target's size (`HydrateTarget.size`) but `fetch_bodies` takes ids only; passing the sizes would spare the minimal batch of a resumed hydration. Same reason.
- **A gone body.** A message deleted between its listing and its body read is left out of `fetch_bodies`; the driver's per-item fallback then reads it alone and fails on its 404, as before. Telling the driver "gone" belongs with the two above.
- Measuring on a real account: no Gmail server here; the live suite (`tests/google.rs`) needs the test account's secret.
