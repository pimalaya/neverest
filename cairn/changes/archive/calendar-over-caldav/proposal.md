---
cairn: change
id: calendar-over-caldav
status: landed
created: 2026-08-28
---

# Calendars sync over CalDAV, through the adapter contacts already use

## Why

Neverest syncs mail and contacts. Calendar is the third domain the project is
built around, and every piece it needs already exists: io-webdav implements RFC
4791 as a mirror of the RFC 6352 surface the CardDAV backend is written against,
io-pim-discovery finds a CalDAV endpoint through the same RFC 6764 mechanism,
and io-pimdir's `conventions::calendar` derives the link id, the Annex A.3
summary and the resolved sort key. Nothing in the engine, the store or the
client seam is mail- or contact-shaped: the seam speaks collections and items,
and the only kind-aware code is one dispatch point.

So the work is not a backend. It is wiring a second flavour into the one that
exists.

## What

**One DAV adapter, two flavours.** `src/carddav/` becomes `src/dav/`, and the
client carries a `DavKind` naming which of the two it speaks. The protocols
differ in exactly three things — the home set they discover, the collection they
list, and the extension a new resource is named with — and in nothing the sync
sees: enumeration, multiget, conditional writes and the reconnect repair are RFC
4918 and RFC 6578, which both sit on. Duplicating the adapter would have meant
maintaining the same 580 lines twice, and the CardDAV half is the one that was
debugged against real servers.

**One cargo feature.** `carddav` becomes `dav`, covering both. They are one
dependency (io-webdav compiles both RFCs unconditionally), one adapter and one
discovery mechanism, so splitting them would gate nothing. The feature is
unreleased, so the rename costs nothing downstream.

**The kind delegates outright.** `text/calendar` is
`io_pimdir::conventions::calendar::derive`, not a scanner of our own. The two
reasons the mail and card scanners stayed here do not apply: io-pimdir reads a
calendar's summary fields the way Annex A.3 spells them, verbatim, which is the
reading a frontend wants, and it resolves the sort key through the `VTIMEZONE`
the resource carries — the one genuinely hard part, and the one two writers of a
store must not answer differently. A second implementation could only drift.

**Configuration and wizard mirror CardDAV.** A `caldav` table with the same
`server`, `tls`, `alpn` and `auth` fields, the same direct-backend sugar, and a
discovered CalDAV entry the wizard offers beside the CardDAV one.

## Not in scope

**No component filter.** A calendar is synced whole: every `VEVENT`, `VTODO` and
`VJOURNAL` it holds. Filtering by component is an item-level filter, which the
account does not have for any kind yet.

**No scheduling.** RFC 6638 free/busy and iTIP delivery are calendaring
features, not sync ones. A resource crosses neverest as opaque bytes.
