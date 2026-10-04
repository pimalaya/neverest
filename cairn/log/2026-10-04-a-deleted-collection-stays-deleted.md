---
cairn: log
change: a-deleted-collection-stays-deleted
date: 2026-10-04
---

# A deleted collection stays deleted

In two-endpoint mode the collection diff was create-only: a collection one endpoint listed and the other did not was created on the other, so one deleted on an endpoint came back from its pair on the next run. Found while planning MOA's folder creation (MOA's `docs/plan/carillon-scope.md`, step A.5); MOA itself syncs one source.

## What landed

- src/offline/driver.rs: `diff_collections` reads the store as the base. `held_by_both` asks the store which listed-on-one-side collections both endpoints held (`collection_sources`: a binding or a checkpoint of each); for those the other endpoint deleted it and the delete crosses (`collection.delete` permitting), for the rest it is new and is created (`collection.create` permitting). A delete the remaining endpoint forbids leaves it there, logged, never recreated. After a crossed delete the store drops the collection (`delete_collection`), so one made again later reads as new.
- src/sync/hunk.rs: `CollectionHunk::Delete` is produced now; its `dead_code` allowance goes.
- Tests: three unit tests on the diff; tests/endpoints.rs `a_book_deleted_on_one_endpoint_is_deleted_on_the_other_not_recreated` against Radicale (create, cross the delete, create again). `a_one_way_account_overwrites_the_target_instead_of_parking_the_divergence` and `two_endpoints_holding_one_card_under_two_bodies_park_it_and_never_overwrite` fail before and after this change, unrelated.

## Capabilities moved

- sync: a collection deleted on one endpoint is deleted on the other.
