---
cairn: delta
change: calendar-over-caldav
---

# Delta

## ADDED Requirements

### Requirement: A CalDAV source syncs calendars
A source MAY declare a `caldav` backend, whose items are `text/calendar`
calendar object resources and whose collections are the calendars under the
principal's calendar home set (RFC 4791 §6.2.1), keyed by their path segment.
It SHALL accept the same `server`, `tls`, `alpn` and `auth` fields as a CardDAV
source, and SHALL carry no send channel, submission being a mail capability.

The item SHALL be the calendar object **resource**, never the component: RFC
4791 §4.1 keeps every component sharing a `UID` in one resource, so a recurring
series and its modified instances are one item under one link id, and an
override is a body edit rather than an item of its own. A new resource SHALL be
named `<UID>.ics`, the `UID` sanitised to one path segment, so the href stays
derivable from the body.

A calendar SHALL be synced whole. Restricting it to a component type is an
item-level filter, which no kind has.

#### Scenario: A calendar syncs, follows a server edit and retains a delete
- GIVEN a CalDAV server holding two events in one calendar
- WHEN the account is synced, one event is edited on the server and another deleted, and it is synced again
- THEN the store holds both events keyed by their `UID`, follows the edited body, and keeps the deleted one as retained

### Requirement: One adapter serves both DAV protocols
CardDAV and CalDAV SHALL be implemented by one client adapter, parameterised by
which of the two a session speaks. The difference between them SHALL be confined
to the home set it discovers, the collection listing it runs and the resource
extension it names a new item with; enumeration, multiget, conditional writes,
flag handling and the reconnect repair are RFC 4918 and RFC 6578 and SHALL NOT
be written twice.

The adapter SHALL report its own media type to the client seam, so one backend
variant still declares two kinds and the store records the right one per
collection.

## MODIFIED Requirements

### Requirement: Every remote backend is a cargo feature
Each remote SHALL be gated by a cargo feature: `imap` for the IMAP backend,
`msgraph` for the Microsoft Graph backend, `dav` for the CardDAV and CalDAV
backends, `smtp` for the SMTP submission channel. All of them SHALL ship in the
default feature set.

CardDAV and CalDAV SHALL share one feature rather than take one each: they are
one dependency, one adapter and one discovery mechanism, so separate features
would gate nothing that is separately compiled. A feature that merely aliases
another is not introduced for the older spelling.

A missing backend SHALL surface at runtime, never at build time: every feature
combination compiles, the configuration surface stays whole (every source config
still parses), and an unavailable backend fails when the source is *opened*, as
the JMAP and Gmail sources already do. A build with neither `smtp` nor `msgraph`
has no send channel and SHALL warn rather than perform a submit intent. Each
optional backend crate SHALL take its TLS provider from neverest's own
`native-tls` / `rustls-aws` / `rustls-ring` / `vendored` features rather than
pinning one.

### Requirement: Link id and meta are per-kind, resolved at one seam
The cross-collection link id and the `v:1` meta summary SHALL be produced by one
implementation per media type, selected from the source's declared kind at a
single dispatch point. `message/rfc822` keeps the bare `Message-ID` identity with
its `(subject, date, sender)` (`alt:`) fallback. `text/vcard` and `text/calendar`
SHALL use the bare vCard / iCalendar `UID`, falling back to the content hash
(`hash:`) for a body carrying no `UID`; an iCalendar `RECURRENCE-ID` SHALL NOT
enter the link id, so a recurrence override stays the same item. Each kind's meta
schema SHALL follow the pimdir SPEC Annex A convention registered for it.

The `text/calendar` sort key SHALL be the item's start resolved to RFC 3339 in
UTC (`DUE` then `DTSTART` for a `VTODO`, `DTSTART` otherwise), read through the
`VTIMEZONE` the resource itself carries, so an agenda reads chronologically
without the store holding a time zone database.

### Requirement: The conventions are the format's, the readers are not
A link id, a summary and a sort key SHALL be what pimdir SPEC Annex A and the
format's `vectors/meta.json` give, and the summary SHALL be
`io_pimdir::conventions`'s own type (`PimdirMailMeta`, `PimdirCardMeta`,
`PimdirCalendarMeta`), so the schema cannot drift from the format's by a field or
a spelling. This crate SHALL NOT define a summary struct of its own.

A **scanner** stays here only while io-pimdir's loses data this one does not, and
each gap SHALL be held by a test naming it:

- `conventions::mail` reads headers raw, so an RFC 2047 encoded-word subject
  reaches a reader as `=?utf-8?q?…?=`;
- `conventions::card` splits a property on the first colon, cutting the value of
  a legal quoted parameter that holds one (RFC 6350 §3.3), and leaves RFC 6350
  §3.4 escaping in place.

The format's vectors are ASCII-only and cover neither, so nothing upstream
reports the difference. `conventions::calendar` has no such gap and SHALL be
delegated to outright: it reads the summary fields verbatim, which is how Annex
A.3 spells them, and it resolves the sort key through the resource's own
`VTIMEZONE`, which is the answer two writers of one store must not give
differently. When io-pimdir closes a gap, its `derive` SHALL likewise replace the
scanner rather than be mirrored beside it.

#### Scenario: A calendar resource longer than the streamed prefix is sized whole
- GIVEN a calendar resource whose body exceeds the header prefix the stream captures
- WHEN it is summarised
- THEN `meta.size` holds the octet count the stream reported, not the prefix's
