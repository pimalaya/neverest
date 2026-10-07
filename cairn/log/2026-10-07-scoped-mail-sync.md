---
cairn: log
change: scoped-mail-sync
date: 2026-10-07
---

# Scoped mail sync

Block 4 of the joint plan (pimdir `cairn/changes/scoped-mail-sync`), the neverest connectors on io-pimdir `35a1c3f` (pimdir draft-04) and io-msgraph `00ef069`.

## What landed

- Cargo.toml: io-pimdir `35a1c3f`, io-msgraph `00ef069`.
- src/client.rs: `Enumeration` is a page (`cursor`, optional `checkpoint`, `bytes`), `EnumEntry::meta`, `Listed` (`Page`, `CursorRejected`), `Held` (what the store binds), `Client::enumerate(collection, request, held)`, `Client::scope_bound`; `Client::Imap` boxed.
- src/offline/remote.rs: `PimRemote` implements the new seam: meta from the listing, else from the store's binding (`Bound`, `with_bound`), else the bodies of the page fetched across the pool (`name_by_body`, skipped on a dry run, `with_bodies`); `keep_in_scope` drops what the `Date` puts out of scope; body and meta octets counted on the source (`Pool::downloaded`).
- src/imap: rounds of 500 UIDs highest first (`search_round`, `RoundUids` kept for the run), `fetch_metas` (`UID FLAGS RFC822.SIZE BODY.PEEK[HEADER.FIELDS (…)]`, meta by `mail::derive_meta`), `fetch_spines` for the bound, cursor `(UIDVALIDITY, lowest UID)`, QRESYNC deltas naming the unbound; undated messages searched apart from `SENTSINCE`.
- src/msgraph: `enumerate_mailbox` page per response under the summary `$select` and `received_filter`, `maxpagesize` 1,000 with every link, `delta_page`, 410 as `CursorRejected`, `hasAttachments` and `ccRecipients` in the summary; contacts delta at 1,000; `create` for requests that create.
- src/gmail: `round_page` (100 ids, `scope_query`, `historyId` first, page token cursor, 400 as `CursorRejected`), `flag_sets` once a run, `metadata` and `named_entry`, history deltas naming the unbound; `Content-Type` among the headers.
- src/offline/driver.rs: probes and `upgrade_probed` gone; `SourceCtx` carries the scope and the dry run; `sync_verb` passes the scope and `scope_bound`; `sync_side` hands `PimRemote` the binding; `itemize_fetches` names what a pull added with its body; `report_coverage` per collection synced, and on a failed scan.
- src/config.rs: `item.filter.since` (`ItemFilter`), `resolve_since`, `AccountConfig::scope` (refusal by key beside contacts and calendars); src/cli/sync.rs: `--since`.
- src/sync/report.rs: `coverage` (`CollectionCoverage`, `OpenRound`), `downloaded` (`SourceDownload`), an open round incomplete.
- src/throttle.rs: `Request::{Idempotent, Create}`, `google_throttled_for`; Graph, Gmail, Google Calendar and People, DAV route creates through it. Graph body batches answered 500, 502 or 504 retried without giving the source up.
- Tests: unit tests for the scope filter, IMAP meta and cursor, Graph pages, filter and attachment mark, Gmail meta and query, the configuration, the report, the throttle, an interrupted and a refused round; tests/scope.rs (Stalwart): an old `Date` received today, a future `Date`, none, a widening, a narrowing that deletes nothing, a removal out of scope, a round killed after its first page and resumed.

## Capabilities moved

- sync: a mail sync lists within a scope on the `Date`; a provider narrows a scope with a margin; a mail round lands page by page; every listed member arrives named; the report states coverage and downloads; a request that creates is sent again only when unprocessed (new requirements). The pull plan, targeted meta fetches, IMAP enumeration, DAV enumeration, the bodies a run reports, the incomplete exit code and the throttling back-off (modified). A probed item raised to its tier (removed).
