---
cairn: change
id: the-releases-of-2026-11-19
status: landed
created: 2026-11-19
---

# The releases of 2026-11-19

## Why

io-imap 0.12.0, io-smtp 0.10.0, io-sasl 0.3.0, io-pimdir 0.18.0, pimalaya-cli 0.7.0 and ical-rs 0.7.2 were released on 2026-11-19. io-imap, io-smtp and io-sasl break: the std runtime moved from the `client` cargo feature to `std`, and io-sasl's types cross between the IMAP and SMTP clients, so the three move together.

pimdir audit-2026-11-12 P5: the retention sweep (src/sync/driver/upkeep.rs `purge_account`) committed one synced transaction per expired item, a thousand expired messages being a thousand fsyncs, since `purge_retained_before` is store-wide and accounts share a store. io-pimdir 0.18 adds `PimdirStore::purge_items`, chosen items purged in one transaction.

## What

- The six crates from crates.io, no patch; `imap` and `smtp` enable `io-imap/std` and `io-smtp/std`; one io-sasl (0.3.0) in `cargo tree`.
- `purge_account` lists the account's expired retained items, then purges them with one `purge_items` call.
- Not taken: N2 (a public in-flight guard on `PimdirSourceStore`) was not released in io-pimdir 0.18.
