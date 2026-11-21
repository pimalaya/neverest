---
cairn: log
change: connector-listings-fail-rather-than-shrink
date: 2026-11-21
---

# A member listed with an error is not removed

A CardDAV or CalDAV listing naming a member with a 403 or 5xx row no longer reads that member as deleted. io-webdav dropped such rows (`from_multistatus`), so a full listing (a first `sync-collection`, every `PROPFIND` one) lacked the member and the run pushed its delete to the other endpoint; io-webdav's an-enumeration-returns-what-it-could-not-read returns them as `WebdavSyncDelta::unreadable`.

## What landed

- src/client.rs: `Enumeration::errored`, the members a page listed with an error, each its handle and status. The other connectors list none.
- src/dav/client.rs: `sync` and `list` gather `errored` from every round (handles named by `href_id`, as members are), warn of each, and return the listing incomplete when any is there. `sync` keeps the checkpoint it started from in that case: the returned token is past the member's change, so the next run asks again and reports it again, as a truncation without a token does.
- src/sync/remote.rs, driver/reconcile.rs, driver/itemize.rs: the errored members travel by collection beside `truncated` and are reported as a `failures` error of the existing `listing` step, its error naming each member with its status (`b.vcf (403)`). No new row kind: `listing` now means "cut short, or a member listed with an error".
- Cargo.toml: `[patch.crates-io] io-webdav = { path = "../io-webdav" }`, Cargo.lock following, until io-webdav releases the change; the owner replaces it with the release pin.

## Spec

sync: the `listing` step's definition widened; new requirement "A DAV member row answered with an error makes its listing incomplete" (folded from the change's delta).

## Verified

`a_member_listed_with_an_error_makes_the_listing_incomplete` (src/dav/client.rs) failed on `complete` with the field added and left empty. tests/pair.rs `a_member_the_server_answers_with_an_error_is_not_deleted`, over the fake DAV server's new `refused` fault, failed on the old code: with `sync-collection` the run exited 0 with no error, and by `PROPFIND` the target lost `b.vcf`. Both pass. `cargo fmt --check`; `cargo clippy --locked --all-targets -- -D warnings` over the CI feature sets (default, all, `rustls-ring` alone, `native-tls,imap,smtp`, `rustls-ring` with each backend) clean; `cargo test --all-features` 722 passed, 71 ignored; `cargo test --no-default-features --features rustls-ring,imap,smtp` 410 passed, 50 ignored.
