---
cairn: log
change: a-cut-listing-is-reported
date: 2026-11-17
---

# A cut listing is reported

Finding NEV-5 of the CLI family analysis of 2026-11-15: a DAV listing the server truncated (a `507` row with no token, or in the `PROPFIND` fallback) ended the run in sync, exit 0, one card of three landed and the next run printed "already in sync"; only `coverage[].at == null` and a warning off by default said so.

## What landed

- `Enumeration::truncated`, set by the DAV listing and sync paths; `PimRemote::take_truncated`; `Noted::truncated`, itemized as a `failures` entry with the new step `listing` (exit 4).
- Test tests/pair.rs `a_listing_the_server_cuts_short_is_reported_on_every_run`, both listing paths over the fake CardDAV server, failing with exit 0 without the fix.
- Trivial fix on the row every-failure-reaches-the-result added: no proposal.

## Capabilities moved

- [sync](../spec/sync.md): *A truncated `sync-collection` listing is incomplete* and *A store step that fails is reported* amended.
