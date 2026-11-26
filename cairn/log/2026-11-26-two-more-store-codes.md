---
cairn: log
change: two-more-store-codes
landed: 2026-11-26
---

# Two more store codes relayed

io-pimdir 1.0.0 added `PimdirError::SourceEmpty` (`source-empty`) and `PimdirMutateError::Unsynced` (`target-unsynced`). 2026-11-26-io-pimdir-1-0 listed `source-empty` among the codes neverest cannot meet; neverest now relays both, spelled as io-pimdir spells them, so a caller reads io-pimdir's code should either surface, though neither is expected (every source id neverest binds is `<account>/<source>` or one the store holds; its only local edit is the one-way overwrite's `Edit`, and a frontend's move or copy into a collection no source syncs is parked by the drain).

## What landed

- src/error.rs: `ErrorCode::SourceEmpty` and `ErrorCode::TargetUnsynced` (kebab-case, so the `neverest-error` schema lists them), both in `PIMDIR_CODES`, covered by `a_relayed_code_keeps_io_pimdirs_spelling`; `ErrorCode::of_mutate` and `mutate_error` read `PimdirMutateError::code` through the same table as `of_pimdir`.
- src/sync/driver/reconcile.rs: the one-way overwrite's refused `Edit` carries its code (`mutate_error`) rather than a bare message.
- Test: src/error.rs `an_unsynced_target_and_an_empty_source_carry_their_codes`.

## Spec

[sync](../spec/sync.md): *A store error carries its code* relays `source-empty` and `target-unsynced`, says why neither is expected, and drops `source-empty` from the codes neverest cannot meet (in that requirement and in the rules kept from inline comments).
