---
cairn: delta
change: calendar-invitation-intents
---

# Delta

## ADDED Requirements

### Requirement: A calendar intent is performed through the provider's verbs

Neverest performs the `calendar-reply` and `calendar-cancel` intents (pimdir STORAGE Annex B.2) at the start of each sync, one-way included, through the source the payload names, on the item that source binds: Graph through the event's `accept`, `tentativelyAccept`, `decline` and `cancel` actions with the comment, Google by answering the account's own attendee or deleting the meeting it organises, both with `sendUpdates=all`. A performed intent is acknowledged; a refusal (the account attends nothing, or organises nothing), a 4xx, a gone item or an undecodable payload parks it; a 408, 412, 429, 5xx, transport error or an item not pushed yet leaves it pending. Each attempt is reported under `intents`. Both are declared on each calendar the source syncs and `none` source-wide; CalDAV declares them `none`.

#### Scenario: An invitation answered on Google

- **GIVEN** an event of a Google calendar the calendar attends
- **WHEN** a `calendar-reply` `TENTATIVE` is queued on it and the account syncs
- **THEN** the intent is acknowledged and the next sync reads `PARTSTAT=TENTATIVE`

## MODIFIED Requirements

### Requirement: Google Calendar notifies as the resource asks

A new event carrying a UID is inserted with it as `iCalUID` and notifies its attendees when the resource is scheduled (pimdir Annex B.1) and the account organises it (no organizer, the calendar itself or the account's address); otherwise it is imported, notifying nobody. An update notifies the attendees only when the resource is scheduled; a delete notifies them when the event has attendees.
