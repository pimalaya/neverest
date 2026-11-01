---
cairn: log
change: a-collection-is-deleted-only-when-asked
date: 2026-10-30
---

# A collection is deleted only when asked

Release blocker B3 of the pimdir stable-release review: a collection one endpoint stopped listing was deleted on the other by default, and several listings dropped collections nobody deleted.

## What landed

- src/config.rs: `collection.delete` defaults to false (`CollectionSourceConfig`, `CollectionPermissions`); `create` still defaults to true.
- src/gcal/client.rs: `calendarList` asked with `showHidden`, so a calendar hidden in the Google UI is still listed.
- src/msgraph/client/contacts.rs: contact folders read page by page (`$skip` while a next link comes) and breadth first at every depth, where one page of 100 and two levels were read.
- config.sample.toml, README.md, the driver's module header; tests/endpoints.rs (Radicale, ignored by default) enables the delete on both endpoints for its lifecycle test.
- Tests: `a_collection_delete_is_granted_only_when_asked_for`, `the_calendar_list_names_hidden_calendars`, `every_page_and_depth_of_contact_folders_is_listed`, each failing on the old code (delete granted, no `showHidden=true` in the request, folders `{contacts, f1, f2}` where five exist).

## Not done

- With the delete enabled, an IMAP mailbox turned `\Noselect` and a rename still read as a delete: pimdir's collection rule (only when empty or fully reconciled in the run) waits for storage-draft-05 and io-pimdir.

## Capabilities moved

- sync: a collection deleted on one endpoint is deleted on the other (modified: only where `collection.delete` is set); a collection listing names every collection the source holds (added).
