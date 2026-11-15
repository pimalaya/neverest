---
cairn: delta
change: gtd-hardening
---

# Delta

## ADDED Requirements

### Requirement: A create superseding a member is delivered by moving it
A CalDAV source SHALL deliver an `Add` naming the member it supersedes (pimdir SYNC §3 "Superseded", §4) by moving that member into the target calendar with WebDAV `MOVE` (RFC 4918 §9.9, `Overwrite: F`, an absolute `Destination`), gated on the member's last-synced ETag, then writing the stored body over the moved member, gated on the ETag the move answered and unconditional when it answered none. The add SHALL be accepted under the moved member's handle and the revision the write left, so the server never holds the `UID` twice: a server keeping one `UID` per account (Fastmail) refuses an upload while the source holds it.

A `MOVE` answered as one the server cannot do (405, 501, or 404 for the member, Google's answer) SHALL have the add delivered by the plain upload, as without a superseded member. A `MOVE` refused for the identity SHALL reject the add as a refused upload does: a duplicate `UID` named as one (`A refused duplicate names itself`), a moved member (412) as a rejected `move`. A body missing from the store SHALL reject the add before anything moves. A write failing after the move SHALL reject the add, the target's next listing naming the moved member. A CardDAV source SHALL upload, io-webdav moving no card, and every other backend SHALL ignore the superseded member.

#### Scenario: A task filed into another calendar moves
- **GIVEN** a task of `caldav/tasks` filed into another calendar of the same CalDAV server, keeping its `UID`
- **WHEN** the target's create is pushed
- **THEN** the member is moved with `MOVE` gated on its ETag, its body is written over it, and the create is accepted under the moved handle with the write's ETag

#### Scenario: A server without MOVE takes an upload
- **GIVEN** the same filing on a server answering `MOVE` with 404
- **WHEN** the create is pushed
- **THEN** the body is uploaded with `If-None-Match: *` and the create accepted

### Requirement: A delete of a member already gone is accepted
A DAV delete answered 404 or 410 SHALL be accepted (pimdir SYNC §4): pushes are at least once, and a create delivered by a move took the member it superseded along before that member's delete is pushed.

### Requirement: A DAV relocation moves the member
A `Remove` relocating a CalDAV member into another calendar SHALL move it with WebDAV `MOVE`, keeping its `UID` in one place. Where the server cannot move it (405, 501, 404), and on CardDAV, the member SHALL be re-created at the target and deleted from the source on its ETag, the delete running only once the create was accepted, so a failure leaves the member where it was.

## MODIFIED Requirements

### Requirement: A collection is keyed by its backend id and named by its display name
Every hub collection id, every wire call and the account's collection filter SHALL be built from the backend's own id: the mailbox name on IMAP and on Graph, the label name on Gmail, on DAV the path segment of a collection directly under the home set and the href's path, starting with a slash, of one outside it (io-webdav's id; Google lists every calendar as `/caldav/v2/<calendar>/events/`, hub id `caldav//caldav/v2/<calendar>/events`). A display name SHALL NOT address a collection, being optional, mutable and free to collide. The default calendar a scheduling inbox names SHALL be keyed as the listing keys it.

The store SHALL still carry what the collection is called: `set_collection_name` runs beside every `ensure_collection` with the name without the namespace the hub id carries. A DAV source SHALL take it from `DAV:displayname`, falling back to the id when it is blank or absent; every other source's id is already its name. Where two sources meet, the first in declared order with a non-blank name wins. Naming SHALL NOT fail a run: nothing keys on the column, so a refused write is logged and the sync continues.

### Requirement: A CalDAV source syncs calendars
A source MAY declare a `caldav` backend, whose items are `text/calendar` calendar object resources and whose collections are the calendars under the principal's calendar home set (RFC 4791 §6.2.1), keyed by their path segment, or by their path for one outside the home set (`A collection is keyed by its backend id and named by its display name`). It SHALL accept the same `server`, `tls`, `alpn` and `auth` fields as a CardDAV source, and SHALL carry no send channel, submission being a mail capability.

The item SHALL be the calendar object **resource**, never the component: RFC 4791 §4.1 keeps every component sharing a `UID` in one resource, so a recurring series and its modified instances are one item under one link id, and an override is a body edit rather than an item of its own. A new resource SHALL be named `<UID>.ics`, the `UID` sanitised to one path segment, so the href stays derivable from the body, unless its key was minted, which the name then carries too.

A calendar SHALL be synced whole. Restricting it to a component type is an item-level filter, which no kind has.

#### Scenario: A calendar syncs, follows a server edit and retains a delete
- GIVEN a CalDAV server holding two events in one calendar
- WHEN the account is synced, one event is edited on the server and another deleted, and it is synced again
- THEN the store holds both events keyed by their `UID`, follows the edited body, and keeps the deleted one as retained
