---
cairn: change
id: a-collection-is-deleted-only-when-asked
status: landed
created: 2026-10-30
---

# A collection is deleted only when asked

## Why

Release blocker B3 of the pimdir stable-release review (2026-10-30). The two-endpoint collection diff (src/offline/driver.rs, `diff_collections`, `held_by_both`) deletes a collection on the remaining endpoint when the other endpoint's listing no longer shows it, and `collection.delete` grants that by default (src/config.rs, `CollectionSourceConfig` and `CollectionPermissions`). Absence is the only collection-deletion signal IMAP and DAV give, so the inference stays, but several listings drop a collection nobody deleted:

- Google Calendar's `calendarList` leaves hidden calendars out unless asked (`showHidden`, src/gcal/client.rs:226-229): hiding a calendar in the Google UI deletes its mirror on the other endpoint.
- Graph contact folders are read as one page of 100, two levels deep (src/msgraph/client/contacts.rs:54-90): a 101st folder, or a grandchild, reads as deleted.
- An IMAP mailbox turned `\Noselect` is filtered out of the listing.
- A rename reads as a delete plus a create before the members are reconciled, so the other endpoint's members not yet synced go with the delete.

pimdir SYNC §5 forbids a sync deleting on a server on its own account from an absence; the plan (pimdir spec edit 3) keeps one exception, a collection delete the user enabled.

## What

- `collection.delete` defaults to false, in the configuration and in the permissions a source without a `collection` table gets. A collection deleted on one endpoint then stays on the other, as a forbidden delete does today, until the user enables the delete for that endpoint. Breaking for an account relying on the default.
- Google Calendar lists hidden calendars too (`showHidden`).
- Graph contact folders are read page by page and at every depth.
- The sample configuration and the README say the delete is off unless enabled.

## Not done

- A delete enabled by the user is still inferred from an absence: a `\Noselect` mailbox and a rename still read as a delete there. pimdir's collection rule (spec edit 3: only when empty or fully reconciled in the run) lands with the spec's storage-draft-05 and io-pimdir; neverest follows it then.
