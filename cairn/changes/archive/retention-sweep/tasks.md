---
cairn: tasks
change: retention-sweep
---

- [x] `HumanDuration` (parse + render + serde) and `StoreConfig::purge_after`.
- [x] `StoreConfig::purge_cutoff(now)`: `None` when unset, RFC 3339 millis
      otherwise, in the shape the store stamps `retained_at` with.
- [x] `sweep_retained` in the driver: warns rather than fails, runs on both run
      paths after the sync, never in a dry run.
- [x] `sync --no-purge`.
- [x] Report: `purged` section (items + bytes), text and `--json`.
- [x] Docs: `config.sample.toml` (knob + backup recipe), README (Retention and
      purging), CHANGELOG.
- [x] Tests: cutoff boundaries and the delay's document round-trip; the sweep
      runs only when a delay is configured.
- [x] Fold delta into `cairn/spec/sync.md`; write log entry.
- [x] Verified against io-pimdir's working tree (`purge_retained_before` /
      `PimdirPurgeReport` landed the same day).
- [ ] Drop the `[patch.crates-io]` block once io-pimdir publishes.
