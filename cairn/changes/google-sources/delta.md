---
cairn: delta
change: google-sources
---

## ADDED Requirements

### Requirement: Google contacts and calendars are native sources
See cairn/spec/sync.md, folded on landing.

## MODIFIED Requirements

### Requirement: Sources are remote backends only
Google People joins the `text/vcard` sources and Google Calendar the `text/calendar` ones.

### Requirement: Every remote backend is a cargo feature
`gpeople` and `gcal` gate the two Google backends.

### Requirement: A backend under the account is a source named after its protocol
`gpeople` and `gcal` join the sugar keys.

## REMOVED Requirements
