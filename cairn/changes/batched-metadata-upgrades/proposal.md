---
cairn: change
id: batched-metadata-upgrades
status: landed
created: 2026-10-06
---

# Batched metadata upgrades

## Why

A first mail sync upgrades every live Probed handle in a collection through one metadata fetch and one engine write. A large IMAP folder can therefore send tens of thousands of UIDs in one request. A failed request leaves that entire metadata upgrade pending.

## What

Upgrade the Meta probe tier sequentially in batches of at most 64 handles, reusing the existing fetch cap. Complete each PimdirUpgrade before requesting the next batch. Stop on the first error. Earlier committed metadata remains in the store, and a later run selects only placements still Probed.

Sort the complete eligible Meta handle list with PimdirHandle's lexical order before partitioning. This preserves which handle claims a duplicated identity first, matching the engine's existing order within a single upgrade.

Keep the Full probe tier and its dry-run guard unchanged. Keep tombstones excluded. Missing or unparseable summaries remain Probed. Use the existing engine and atomic store writes, without a second checkpoint or recovery format.

## Scope

This change bounds metadata requests, fetched results and write transactions. It does not bound the initial collection load or add date filters, UID windows, a configurable batch size, automatic retries or parallel metadata requests. Production NAS binaries and configuration are outside this PR.

## Verification

Exercise the production batching path with the real PimdirStore and engine. Record only the remote fetch boundary. Check empty and partial batches, eligibility, lexical ordering and duplicate identities across batch boundaries, a later failure followed by reopening and resuming, unchanged checkpoint/generation, missing summaries and dry-run tiers.

Run the targeted regressions, the upstream all-feature unit suite, clippy and formatting. Reuse existing feature configurations unless imports or gates change. Live provider tests require upstream secrets and will remain explicitly unrun. Disclose OpenAI Codex assistance in the pull request.
