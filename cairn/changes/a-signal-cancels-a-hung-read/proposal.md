---
cairn: change
id: a-signal-cancels-a-hung-read
status: active
created: 2026-11-26
---

# A signal cancels a hung read

Carried from the-contract-audit-rulings (archived 2026-11-26), finding NEV-X2 of the CLI contract audit of 2026-11-20.

## Why

"A signal stops a sync cleanly" (cairn/spec/sync.md) lets what is in flight finish on a first SIGINT or SIGTERM. A read blocked on a silent server is in flight for as long as pimalaya-stream's `Retry::Until` budget runs (src/retry.rs of pimalaya-stream), so a first signal waits it out, and the account it hangs on is never reported unreached. neverest cannot reach the socket: the protocol clients own their streams, and pimalaya-stream 0.4.1, which neverest locks, exposes no cancellation.

## What

- pimalaya-stream: a cancellation hook, a shared shutdown flag its `Retry::Until` loop polls between short socket timeouts, failing the read as interrupted once set (a change and a release there first).
- neverest: hand pimalaya-cli's `signal::Shutdown` flag to every stream it opens; a read cut by the flag reports the account `unreached` (or its report `interrupted`), the run exits 4.
- Spec: "A signal stops a sync cleanly" says a hung read is cut by the first signal.

## Blocked on

pimalaya-stream's hook and its release.
