---
cairn: log
change: the-drive-loop-counts-its-verb
date: 2026-11-19
---

# The drive loop counts its verb

## What landed

- src/sync/mod.rs: `run_verb_paged` (which `run_verb` calls) holds io-pimdir's in-flight guard, `PimdirSourceStore::running`, from the first resume to the verb's completion, pause or failure, as io-pimdir's own `run` does, so a collection of the process refuses as `InFlight` while a neverest verb runs (pimdir audit-2026-11-12 line N2, pimdir STORAGE §5). Harmless before, the sweep collecting only after the verbs, but the refusal no longer depends on that order.
- Cargo.toml: io-pimdir patched to the local ../io-pimdir under `[patch.crates-io]` until io-pimdir releases `PimdirSourceStore::running` (io-pimdir cairn/log/2026-11-19-a-runner-of-its-own-counts-its-verb.md); the patch goes with that release.
- CHANGELOG under Fixed.

## Spec

- [sync](../spec/sync.md): new *A verb counts as in flight while neverest drives it*.

## Tests

src/sync/driver/tests.rs `a_running_verb_keeps_the_collector_out`: a paged mail round whose remote asks a second handle of the process to collect at every listing; each collection refuses as `InFlight`, and the collector runs after the verb. Against the loop without the guard it fails, every collection running (`refused` all false).
