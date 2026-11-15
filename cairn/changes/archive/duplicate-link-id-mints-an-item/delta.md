---
cairn: change
change: duplicate-link-id-mints-an-item
---

# Delta

## ADDED Requirements

### Requirement: A new resource name never collides with a stored one
A resource name derived for an item being appended SHALL be unique within its collection. Where the item's link id was minted because its identity was already taken (pimdir SPEC §9), the name SHALL carry the same distinguishing part, so two items sharing a `UID` are pushed to two hrefs.

The fallback that derives a name from the body is the trap: a duplicate's body carries the same `UID` as its twin, so a name derived from it collides by construction, and a colliding `PUT` is not refused by the server but applied to the resource already there. The copy that was already synced is overwritten by the copy being appended, which loses an event and reports success.

#### Scenario: Two copies are pushed to two names
- GIVEN two items of one collection sharing a `UID`, one keyed bare and one minted
- WHEN both are appended to a source that holds neither
- THEN they are created under two distinct resource names

### Requirement: A create is refused when the server hands back a bound handle
A create whose assigned handle is already bound by that source in that collection SHALL be recorded as a rejected push, never as a binding. The engine binds one handle per item per source, and two items pointing at one handle make the next enumeration read one of them as vanished, which propagates a delete of a resource nobody removed.

A server answering a create by updating the resource that already holds the `UID`, rather than refusing it, is what produces the collision. That behaviour is out of spec (RFC 6352 §6.3.2) and cannot be prevented from here, so it is detected on the way back instead.

#### Scenario: A merging server is caught
- GIVEN an append of an item whose `UID` the target already holds
- WHEN the server answers with the href of the existing resource
- THEN the push is reported as rejected and no second binding is written

### Requirement: A refused duplicate names itself
A push refused with the CalDAV or CardDAV no-uid-conflict precondition SHALL be reported as a duplicate `UID` refusal, naming the source, the collection and the `UID`, in the text and `--json` reports alike. It SHALL keep appearing on every run until the source stops holding the identity twice.

The repetition is the point: the run wrote nothing, the state is unresolved, and the line carries the one action that resolves it. That is what separates it from the phantom fetch this change removes, which named work no run could ever complete.

#### Scenario: The refusal is actionable
- GIVEN a target that refuses a duplicate `UID`
- WHEN a run pushes the second copy
- THEN the report names the refusal, the `UID` and the collection, and the run reports having written nothing

### Requirement: A duplicated identity is mirrored, not reported
A collection holding two resources under one identity SHALL be mirrored as two items, and SHALL produce no report entry of its own. The store holds what the source holds, and a report entry is for work a run could not do.

## MODIFIED Requirements

## REMOVED Requirements

### Requirement: An ambiguous identity is reported, never judged
**Reason**: nothing is ambiguous any more. The engine mints a key for the second copy (pimdir SPEC §9), so both resources are stored, listed and pushed, and there is no frozen state left to surface. The half of the requirement worth keeping, that this crate derives no duplicate rule of its own and never calls the collection invalid, holds unchanged and is now visible in what it does not print.
