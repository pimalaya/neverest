---
cairn: log
change: larger-metadata-batches
landed: 2026-10-06
---

# Use a separate metadata batch size

Following the PR review, Meta probes now fetch and commit at most 1000 handles per sequential batch through a driver-owned META_BATCH_SIZE. Metadata is light and sequential, so it needs fewer round trips than the body-sized limit allowed. Body fetching keeps BATCH_SIZE at 64 and its original documentation. Lexical duplicate claims, completed-batch persistence and Full-tier behavior are unchanged. This supersedes the 64-handle metadata limit recorded in batched-metadata-upgrades; that historical log is preserved.

## Validation

The six store-backed metadata regressions passed with the new limit, covering 999, 1000, 1001 and 2001 probes, failure/reopen/resume, cross-batch duplicate claims, missing replies and dry-run behavior. The existing body-order batching regression also passed, retaining 64-body batches. Formatting and all-target/all-feature clippy passed after formatting one test array; clippy reports the same eight existing warnings in untouched code.

The wider build, documentation and dependency-audit evidence from 66346ee is reused: this follow-up changes only the metadata limit, its affected tests and documentation, without changing dependencies, features or other call paths. No live provider test or production change was made. Upstream CI remains for the maintainer to run because the fork lacks CACHIX_AUTH_TOKEN.

## Capabilities moved

sync: metadata batches use a separate 1000-handle limit; body batches remain at 64.
