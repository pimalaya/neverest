---
cairn: log
change: the-v1-review
date: 2026-11-16
---

# The v1 review, phase 1: deletions

- The by-name refusals of removed keys (`removed_key!`) are gone: every key neverest does not read, an earlier neverest's included, fails as an unknown field through `deny_unknown_fields`. Their tests became one table of old keys refused as unknown.
- `default` is no longer read: a configuration naming it fails as unknown.
- MIGRATION.md is deleted (no migration from 0.x to v1); the store refusals keep their order without citing it.
- The `NEVEREST_PROFILE` profiler (src/offline/prof.rs) is deleted.
- The direct `crossterm` dependency is removed; `account` reads "List and check the configured accounts".
- Cargo.toml and deny.toml carry the owner's edits (dev-dependencies first, RUSTSEC-2023-0071 ignored, the `domain` git source and `0BSD` dropped).
- Capability moved: sync (the configuration refuses every key it does not read; a store the format outgrew; `default` removed).
