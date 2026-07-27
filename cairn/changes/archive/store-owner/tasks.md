---
cairn: tasks
change: store-owner
---

# Tasks

- [x] Deps: io-pimdir v2 fallout (io-replica `client` feature gone); add
      io-msgraph / io-oauth / io-smtp with path patches.
- [x] `config`: `MsgraphAuthConfig` flows (bearer, device-code,
      client-credentials, client-credentials-cert), per-account `smtp` table,
      `store.hydration`.
- [x] `client`: opaque enumeration checkpoint + string handles in the seam;
      `OpenContext`; `Client::Msgraph` arms.
- [x] `msgraph`: `GraphAuth` (three flows + bearer, tokens.json 0600),
      `GraphClient` (folders two levels, delta rounds, 410 recovery, raw
      bodies, flags/meta/link mapping, send_mime) + unit tests.
- [x] `driver`: pre-sync queue drain into the report; Outbox local-only
      filter; outbox flush through the send channel; IMAP handle-space
      rebuild via `ReplicaRekey` + `write_rekeyed`; full-hydration mode.
- [x] `offline/outbox`: send channel (SMTP submission / Graph sendMail),
      outbox meta envelope, flush + scripted SMTP sink test.
- [x] `cli/sync`: flock at the real store dir with a 60 s bounded wait.
- [x] `report`: drained / parked / outbox sections (text + `--json`).
- [x] Tests: drain integration, flock wait, rekey + generation bump over a
      fake remote, Graph pure functions, outbox flush against the SMTP sink.
- [x] fmt + clippy clean, whole suite green; sample config + CHANGELOG.
- [x] Fold `delta.md` into `cairn/spec/sync.md`; log; land.
