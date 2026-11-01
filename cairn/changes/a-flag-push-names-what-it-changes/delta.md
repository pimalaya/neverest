---
cairn: change
change: a-flag-push-names-what-it-changes
---

# Delta

## ADDED Requirements

### Requirement: A flag push names what it adds and removes
A flag change SHALL reach a server as the flags it adds and the flags it removes (pimdir SYNC §4), never as a whole set replacing the server's: an IMAP `UID STORE +FLAGS` and `-FLAGS`, a Graph patch naming only the properties a flag of either set maps to (`isRead` for `\Seen`, the follow-up status for `\Flagged`), a Gmail modify naming only the labels either set maps to. A flag named in neither set SHALL be left as the server holds it, so a flag another client changed between the listing and the push survives; a change naming no flag the server maps SHALL send nothing.

#### Scenario: Another client answers a message while the run removes its flag
- **GIVEN** a message listed seen and flagged, then answered on the server by another client
- **WHEN** the run pushes the removal of `\Flagged`
- **THEN** the message holds `\Seen` and `\Answered` on the server

## MODIFIED Requirements

None.

## REMOVED Requirements

None.
