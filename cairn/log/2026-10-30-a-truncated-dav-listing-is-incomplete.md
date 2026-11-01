---
cairn: log
change: a-truncated-dav-listing-is-incomplete
date: 2026-10-30
---

# A truncated DAV listing is incomplete

Release blocker B2 of the pimdir stable-release review: a first `sync-collection` listing the server kept truncating was handed over as complete.

## What landed

- src/dav/client.rs: `DavClient::sync` tracks whether its last answer was truncated; a listing still truncated (rounds spent, or an answer without a token) is incomplete, and a truncated answer without a token ends the listing, the checkpoint staying the one it started from.
- Tests over a replayed io-webdav stream: `a_listing_truncated_to_the_last_round_is_incomplete`, `a_truncated_answer_without_a_token_ends_the_listing`, `a_truncation_followed_to_its_end_is_complete`.

## Evidence

On the old code the first two failed: a 32-round truncated first listing came back complete, and an answer without a token was listed again from its start 32 times (`c0.vcf` to `c31.vcf`).

## Capabilities moved

- sync: a truncated `sync-collection` listing is incomplete (added).
