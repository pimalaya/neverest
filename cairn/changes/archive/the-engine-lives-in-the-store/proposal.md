---
cairn: change
id: the-engine-lives-in-the-store
status: landed
created: 2026-09-03
---

# The engine lives in the store

## Why

io-replica is retired. Its sync engine now lives inside io-pimdir, under the `Pimdir` prefix, and the storage trait io-pimdir was the only implementor of is gone with it: the store services its own storage yields, and a driver of its own hands them to `PimdirSourceStore::service`. The store caught up with the pimdir standard of the same day at the same time: the opaque `meta` blob is five typed summary tables plus `item_address`, derived under STORAGE Annex A by the crate rather than by each writer, a pulled member is a probe row, a rebuild's `Rekeyed` drops bump the generation, and a store from an earlier draft is refused as stale.

neverest carried three things that duplicated what the engine now does: a narrowed storage seam (`HeldStore`) keeping a sibling's offered copy out of the identity check, a card and calendar scanner beside io-pimdir's conventions, and a mail summary of its own beside `PimdirMailMeta`. Each was written where the crate below fell short, and each is now the crate below's.

## What

- Cargo.toml drops io-replica and takes io-pimdir 0.4 through a path patch, the engine being unreleased.
- src/offline/ ports onto `io_pimdir::{change, collection, coroutine, hub, load, mutate, object, placement, rekey, remote, sync, upgrade}` and `io_pimdir::client::{PimdirStore, PimdirSourceStore, PimdirError, blobs, producer, reader}`. The driver keeps its own loop for profiling and the cached `Full` apply, servicing storage yields with `PimdirSourceStore::service`; `HeldStore` goes, the store's own load by key now answering with the rows the source binds; the rekey pump goes, the generation being read back after the store bumped it on the batch's `Rekeyed` drops.
- src/kind/ keeps the dispatch, the merge and the read side of a minted key (`split_link_id`, which a write needs to name a resource), and delegates every derivation to io-pimdir: `summary::derive` at the `Full` tier, `PimdirMailSummary` built from the envelope at the `Meta` tier. kind/vcard.rs and kind/ical.rs are deleted; kind/mail.rs restates the streamed size and leaves the attachment unknown. The envelope carries `Cc` and `Bcc` so the two tiers write the same address rows.
- The queue's `update` carries no summary, the owner deriving it from the body: `conflict::Conflict::apply` and the run's own merge stage the hash alone.
- The one io-pimdir edit: a load by key (`PimdirLoadScope::Links`) answers with the rows the source binds and no longer with the copy the hub offers for an item the source lacks, which is what `HeldStore` filtered here and which the pimdir spec (SYNC §6) states as the mint's condition.
- cairn/spec/sync.md loses every `Replica*` name and the scanner requirement, and states the derivations as io-pimdir's.
