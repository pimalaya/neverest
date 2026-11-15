---
cairn: change
id: a-settled-body-is-a-body-of-its-item
status: landed
created: 2026-08-29
---

# A settled body is a body of its item

## Why

`conflict resolve --interactive` took whatever bytes the merger left. `Merger::run` decides that a decision was made by comparing the output file against the empty bytes it seeded, and nothing after that reads them: `Conflict::apply` wrote them into the blob tree and staged an `update`, calling `Kind::parse_body` only for the summary it derives on the way past and ignoring what that parse could not find.

Confirmed end to end against a real CardDAV account. A merger writing `this is not a card at all` and exiting zero settled the conflict and stored the 26 bytes:

    seq=30 link=nvt-delta level=Full
    meta=Some("{\"fn\":\"\",\"size\":26,\"v\":1}")

The `meta` is the tell: an empty `fn`, no `uid`, and a row the store still addresses as `nvt-delta`. What stopped it going further was the server, which answered the push with `403 Resource is not a vCard object`; a server that stores what it is given would have taken it, and the store's own copy, which is what a frontend reads, was destroyed before any server was asked.

Three ordinary things produce it: a merger that crashes after a partial write, a template a person saves half-finished, a tool that writes its error message to the output path. The automatic merge already refuses exactly this and says so, with `Merged::Unmergeable`; the interactive path performed no such check.

## What

`Conflict::apply` validates the chosen body before the blob write, so a body no parser reads never reaches the tree at all. Valid means two things:

- **It reads as the collection's kind.** The body opens with `BEGIN:` and closes with `END:` of the kind's component, `VCARD` for contacts and `VCALENDAR` for calendars (RFC 6350 §6.1.1, RFC 5545 §3.4). Mail is refused outright: its bodies are immutable, so no body settles a message.
- **It keeps the item's identity.** The `UID` the body states is the one the item is bound by. A resolution stating another `UID`, or none, is a resolution of some other item, and taking it leaves the store addressing a row by an identity its content no longer carries.

The reading is the kind's own scanner rather than vcard-rs and ical-rs. A build without the `merge` cargo feature has neither, and is also the build where an interactive resolution is the *only* way a divergence is ever settled, so it is the one that can least afford to be guarded differently.

Both refusals name what is wrong and change nothing: the divergence stays parked, exactly as an aborted merger leaves it.

## Alternatives weighed

**Parsing with the merge's CST** (vcard-rs, ical-rs) is stricter and would catch a malformed line inside a well-delimited card. It is behind a cargo feature this check must work without, and two implementations of "is this a card" would drift. The scanner catches the shape the campaign produced and every shape a truncated write produces.

**Only checking the identity** was tempting, since it alone catches the reported body. It has a hole: a card carrying no `UID` is keyed by a digest of itself, so both sides state no identity and any bytes at all would pass.
