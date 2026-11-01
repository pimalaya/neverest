---
cairn: change
id: a-truncated-dav-listing-is-incomplete
status: landed
created: 2026-10-30
---

# A truncated DAV listing is incomplete

## Why

Release blocker B2 of the pimdir stable-release review (2026-10-30). The `sync-collection` loop (src/dav/client.rs:487-522) decides `complete = token.is_none()` before it drains the truncated rounds (RFC 6578 §3.6, a `507` row). Two paths then hand a partial listing over as complete:

- after `MAX_SYNC_ROUNDS` (32) truncated rounds it only warns, and a first listing still returns `complete: true`;
- a truncated answer carrying no sync token resets the token to none and lists again from the start, so a server that always truncates without a token is read 32 times from its start and the cut listing returned as complete.

A complete listing reads as "absence means removed" (pimdir SYNC §4): on a first sync every member past the cut is inferred deleted, and in a two-endpoint account deleted on the other endpoint. This repository's spec already requires a truncated listing to be reported as a delta (cairn/spec/sync.md, "A DAV server without `sync-collection` is listed instead"), which the `PROPFIND` fallback does and the report loop does not.

## What

- The loop tracks whether its last answer was still truncated. A listing whose last answer was truncated, rounds exhausted or token missing, is returned incomplete (`complete: false`), read by the engine as a delta: what it lists lands, nothing is inferred removed.
- A truncated answer without a sync token stops the loop rather than listing again from the start; the checkpoint then stays the one the listing started from, so the next run asks again from there.
- A truncated answer with a token keeps draining as today, its token the checkpoint.

## Not done

- io-webdav skipping a member row with a non-2xx status other than 404 and 507 inside a complete listing (a 403 or a 5xx member read as deleted): an io-webdav fix, listed in connector-listings-fail-rather-than-shrink.
