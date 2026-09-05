---
cairn: delta
change: the-engine-lives-in-the-store
---

## ADDED Requirements

### Requirement: The summary is the format's typed row
The summary written for an item SHALL be io-pimdir's `PimdirSummary`, the row of the kind's summary table with the addresses it names (pimdir STORAGE Annex A): `mail_summary` for `message/rfc822`, `contact_summary` for `text/vcard`, `event_summary`, `task_summary` or `journal_summary` for `text/calendar`, plus `item_address`. A reader renders a list from those rows and never parses a blob. Flags are not in the summary. Both the envelope (`Meta`) and the streamed (`Full`) paths of mail SHALL emit the same row, the streamed path carrying the message's known octet length as `size` rather than the header prefix it read.

### Requirement: The derivations are the format's
A link id, a summary and a sort key SHALL be what pimdir STORAGE Annex A gives, derived by io-pimdir: `summary::derive` at the `Full` tier from the streamed bytes, and `PimdirMailSummary` built from the IMAP or Graph envelope at the `Meta` tier, so the schema cannot drift from the format's by a field or a spelling. This crate SHALL define no summary struct and no scanner of its own.

The streamed tier reads a header prefix rather than the whole body, so it SHALL restate the message's octet length as the summary's `size` and SHALL leave `attachment` unknown, no MIME part having been walked. The envelope tier SHALL carry every `From`, `To`, `Cc` and `Bcc` address the backend surfaces, so the address rows the two tiers write agree.

#### Scenario: A non-ASCII subject reaches a reader readable
- GIVEN a message whose `Subject:` is RFC 2047 encoded
- WHEN either tier summarises it
- THEN the summary's subject holds the decoded text, not the encoded word

### Requirement: Link id and summary are per-kind, resolved at one seam
The cross-collection link id and the typed summary SHALL be produced by io-pimdir's derivation for the media type, selected from the source's declared kind at a single dispatch point. `message/rfc822` keeps the bare `Message-ID` identity with its `(subject, date, sender)` (`alt:`) fallback. `text/vcard` and `text/calendar` use the bare vCard / iCalendar `UID`, falling back to the content hash (`hash:`) for a body carrying no `UID`; an iCalendar `RECURRENCE-ID` SHALL NOT enter the link id, so a recurrence override stays the same item.

The `text/calendar` sort key SHALL be the item's start resolved to RFC 3339 in UTC (`DUE` then `DTSTART` for a `VTODO`, `DTSTART` otherwise), read through the `VTIMEZONE` the resource itself carries, so an agenda reads chronologically without the store holding a time zone database.

## MODIFIED Requirements

### Requirement: Every item carries a per-kind sort key
The sync SHALL write a `sort_key` beside the summary of every item it summarises (pimdir STORAGE §9.3), derived by the same io-pimdir derivation and never parsed back out of the summary by the store. `message/rfc822` SHALL carry the `Date:` header normalised to RFC 3339 in UTC at seconds precision, so byte order is chronological order whatever offset the sender wrote; `text/vcard` SHALL carry the display name (`FN`) casefolded and trimmed. A kind resolving at two tiers SHALL derive the byte-identical key at both, on the same terms as its link id: a key that moved when the body arrived would re-sort the item on hydration. Content carrying nothing to derive from SHALL keep the empty key, which the store reads as unknown.

### Requirement: An IMAP handle-space change rebuilds the collection and bumps its generation
For an IMAP source, the driver SHALL compare the stored checkpoint's UIDVALIDITY before and after the pull; on a change it SHALL run io-pimdir's rekey, carrying cached bodies, summaries and pending state over by link id. The rebuild batch drops every old handle as `Rekeyed`, which the store reads as the rebuild and answers by bumping `collections.generation` in the transaction applying it (pimdir SYNC §8), so a frontend derives its epoch (an IMAP UIDVALIDITY) from the store alone; the driver reads the generation back rather than routing the batch anywhere special. Ordinary syncs and full resyncs never bump. Graph sources never rebuild: Graph message ids survive a delta reset (an expired delta link restarts a full round without changing identity).

### Requirement: IMAP enumeration is incremental (QRESYNC)
Enumeration SHALL carry a per-mailbox cursor `(UIDVALIDITY, HIGHESTMODSEQ)` in the `PimdirCheckpoint`. On a QRESYNC-capable server (ENABLEd on connect) with a cursor whose UIDVALIDITY still matches, `enumerate` SHALL issue a QRESYNC `SELECT (QRESYNC (uidvalidity highestmodseq))` and return a **delta** (`complete = false`): only the messages changed since the modseq plus the vanished UIDs, issuing **no FETCH when nothing changed**. Without a usable cursor (first sync, UIDVALIDITY change, malformed checkpoint) or on a non-QRESYNC server it SHALL return a **full** `FETCH 1:* (UID FLAGS)` snapshot (`complete = true`). Enumeration SHALL fetch UID and FLAGS only, never ENVELOPE, since the link id is resolved at the `Meta` tier.

### Requirement: Mutable-content backends carry a revision and push updates
A backend whose item bodies change in place SHALL report a content revision (an ETag) on every enumerate and fetch, and SHALL return the revision the server assigned from every accepted write. `PimdirChangeKind::Update` SHALL be pushed as a conditional write against the base revision. A write the server refuses because the revision moved SHALL be reported as rejected, so the engine re-merges and records the divergence as a conflict rather than overwriting the remote. Item bodies on an immutable-content backend (mail) keep no revision, and an update there is still rejected as impossible.

### Requirement: A refused delete is held, never reverted
Every source SHALL sync under `PimdirDeletePolicy::Keep`. Both refusals (`push` off, or `item.delete = false`) run through that one disposition, and each source here is bound to the store's hub, which fixes the answer: reverting a tombstone states that the source still holds the member, and a hub reads that as the item being alive (add-beats-delete across sources), so it clears the deletion for every source and mirrors the item back to the one it was deleted on.

A source configured to take no deletes would then resurrect on both what the user removed on one, which is the opposite of what that setting is for.

#### Scenario: A read-only source keeps the removal
- GIVEN a staged delete on a source whose `item.delete` is false
- WHEN the source is synced
- THEN nothing is pushed and the tombstone stays, rather than being undone into a clean row

### Requirement: One identity is one item across an account's endpoints
An identity two endpoints of one account already hold SHALL bind to a single shared item, whichever endpoint the store reads first, and SHALL NOT be minted a second key. The minting rule answers one collection holding one identity twice; a sibling endpoint holding it once is not that.

Identity is settled by the fetch that reads it, against the placements the store answers with. A source's whole-collection projection carries the copies its sibling holds and it does not, so that the merge can derive the append, and reading those as claims on the identity turns the second endpoint's own card into a duplicate of the first endpoint's. The store's load by key therefore answers with the rows that source binds and nothing else, which is io-pimdir's rule (its store capability, a load by key answers with the rows the source binds), and neverest reads the store through the plain seam with no narrowing of its own.

A mirror and a migration both start from two servers already holding the same items, so binding only when an item propagates from one side is binding in exactly the case that does not matter.

#### Scenario: Two servers already holding one card
- GIVEN the same card on both endpoints before the store has read either
- WHEN the account is synced
- THEN the store holds one item under the card's own key, neither endpoint is asked to take a copy it already holds, and a second run is quiescent

## REMOVED Requirements

### Requirement: The mail summary is a versioned schema
Superseded by "The summary is the format's typed row": the JSON `meta` blob and `PimdirMailMeta` are gone from io-pimdir.

### Requirement: The conventions are the format's, the readers are not
Superseded by "The derivations are the format's": io-pimdir decodes RFC 2047 words and splits and unescapes vCard properties, so the scanners this held here have nothing left to hold.

### Requirement: Link id and meta are per-kind, resolved at one seam
Superseded by "Link id and summary are per-kind, resolved at one seam".
