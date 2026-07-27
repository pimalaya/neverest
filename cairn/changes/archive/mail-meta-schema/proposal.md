---
cairn: change
id: mail-meta-schema
status: landed
created: 2026-08-01
---

# Versioned mail meta schema (`v: 1`)

## Why

The `meta` blob a sync writes for a `message/rfc822` item is the summary a reader
(a Himalaya pimdir backend, action plan M4) renders an envelope list from without
fetching a body. Today Neverest emits an ad-hoc, unversioned `{subject, from,
date}` — too thin for a list (no recipient, no size, no message-id for `alt:`
items) and with no version tag to evolve. For the store to be a usable cache, the
writer and reader must share a **stable, versioned** shape.

`meta` is opaque at the store level (`ReplicaMeta` is a `String`, `items.meta` is
`TEXT`, already `serde_json`-encoded), so the contract is just a documented JSON
shape — no shared crate, no codec. io-pimdir stays kind-agnostic.

## What

- Define the `message/rfc822` meta as `v: 1` JSON: `v`, `message_id?`, `subject`,
  `from?`, `to?`, `date?` (RFC 3339), `size?`. Absent optional fields mean
  "unknown" (omitted, not `null`). Flags are not in meta (they are `items.flags`).
- Widen Neverest's `MetaSummary` and emit it from **both** writer paths — the
  enumerate/`Meta` path (`envelope_meta`, from the `Envelope`) and the
  streamed/`Full` path (`parse_headers`, from the header prefix + the stream's
  known octet length for `size`).
- Document the schema in `pimdir/SPEC.md` §13 ("Application meta conventions"),
  the cross-tool reference both Neverest and Himalaya point at.

## Scope / non-goals

- Mail only. `text/vcard` / `text/calendar` mirror the pattern when first written.
- No reader yet (that is M4); no shared crate — Himalaya defines its own matching
  `Deserialize` struct against the documented shape.
