---
cairn: change
id: pimdir-errors-carry-their-code
status: landed
created: 2026-10-30
---

# pimdir errors carry their code

## Why

Pimdir stable-release plan (2026-10-30), Phase A common. A store error reaches a caller as prose: neverest's JSON error carries no code a program can switch on, io-pimdir's Display still advises "delete the store and let it resync" (io-pimdir src/client.rs:1327), which neverest prints after its own remedy, and neverest hand-rolls the `submit` intent payload (src/offline/submit.rs) that io-pimdir is to type as `PimdirSubmit`.

io-pimdir now gives `PimdirError::code()`, a Display without the delete advice, `Version` saying upgrade, a typed `PimdirSubmit`, and a write side for bodies held under a lock (`PimdirBlobStage`), its reader blobs read-only (io-pimdir cairn/log/2026-10-30-errors-carry-a-stable-code.md, 2026-10-30-the-producer-stages-its-own-body.md).

## What

- neverest's JSON errors carry the code `PimdirError::code()` gives, through its `ErrorCode` (pimalaya-cli machine contract).
- No message of neverest's, or of io-pimdir's that neverest prints, advises deleting the store; the remedies are an upgrade, `sync --reset` on an idle store, or `sync --reset --force`.
- The `submit` intent is read through io-pimdir's `PimdirSubmit`.
- Bodies reach the blob tree through `blob_stage()` (connectors) and `stage_body` (conflict settlements and merges), never written or hashed by neverest.
