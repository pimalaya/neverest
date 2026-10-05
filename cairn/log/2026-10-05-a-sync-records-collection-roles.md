---
cairn: log
change: a-sync-records-collection-roles
date: 2026-10-05
---

# A sync records the role each server states

`check` was the only place neverest read collection roles and defaults, and a program (MOA) read them from its JSON to set up an account, which made a doctor command a data source. pimdir draft-04 gives the store a `collections.role` (io-pimdir `ef8eae0`), so the sync now records them and a frontend reads them from the store.

## What landed

- Cargo.toml, Cargo.lock: io-pimdir by git rev `ef8eae0`.
- `Collection::role`, filled by each listing where it is free: IMAP `LIST` attributes (now `\Flagged` and `\Important` too), Gmail system labels, Graph `isDefaultCalendar` and the default Contacts folder, Google's `primary`, People's one book.
- `Client::lookup_roles`: Graph's well-known mail folders and the CalDAV default calendar, which cost requests; `stated_roles` asks only when the source's collection set moved, keeping the answer in `neverest.json` (`looked_up_roles`).
- `run_local` records the role of every collection it declares, next to its name, clearing one the server no longer states. A paired sync records none yet.
- `sync --declare-only`: every collection, filter aside, with kind, name and role; only queued collection creations run; refused with targets.
- `check` documented as a doctor.

## Verification

- Unit: `roles_a_listing_lacks_are_looked_up_once_per_collection_set`, `a_list_row_states_its_role_by_its_attributes_never_its_name` (flagged, important).
- tests/roles.rs on Stalwart (:143, :8080): a declaration records every IMAP folder with its role and no item, a filtered sync keeps them; the CalDAV default is recorded and reused from the sidecar. tests/check.rs, create.rs, multikind.rs, hydration.rs, moves.rs green; caldav.rs, carddav.rs, calendars.rs, duplicates.rs, submit.rs and relay.rs fail the same way on the parent commit (fixtures this machine lacks), not touched here.

## Capabilities moved

- sync: a sync records the role each server states (new), a declaration syncs no item (new), a check lists collections with their roles (a doctor; roles now in the shared shape).
