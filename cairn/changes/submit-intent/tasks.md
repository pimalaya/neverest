---
cairn: tasks
change: submit-intent
---

- [x] `src/offline/outbox.rs` → `src/offline/submit.rs`: `SUBMIT` kind,
      `SubmitIntent`, `SubmitMeta` (the former `OutboxMeta`), `SubmitFailure`.
- [x] Failure classification: SMTP 5xx (and an undecodable payload, a missing
      body) permanent, SMTP 4xx and transport transient; Graph 4xx permanent
      except 408/429. `GraphClient::send_mime` returns its client error so the
      status is readable.
- [x] `drain_submits` in the driver: acknowledge, park or leave pending;
      `flush_outbox` gone, `open_send_channel` kept.
- [x] No channel compiled in: skip, warn, never park.
- [x] Removed `OUTBOX`, `is_outbox`, both `ensure_collection(OUTBOX, …)` calls
      and the collection-listing filter.
- [x] Report: `outbox` → `submitted` (queue row id, collection, subject, error,
      parked).
- [x] Docs: module header carries the payload contract and the at-least-once
      property; `config.sample.toml`, README, CHANGELOG (BREAKING).
- [x] Tests: an intent sends its pinned body through its envelope; 5xx parks and
      4xx retries; an undecodable or bodyless intent parks; no collection name
      is reserved anymore.
- [x] Fold delta into `cairn/spec/sync.md`; write log entry.
- [x] Verified against io-pimdir's working tree: its drain skips
      `PimdirAction::Unknown`, `pending_actions` hands the row back whole, and
      `drop_action` / `fail_action` acknowledge, retry or park it.
- [ ] Drop the `[patch.crates-io]` block once io-pimdir publishes.
- [ ] himalaya's producer half (enqueue a `submit` action).
