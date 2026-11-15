---
cairn: change
change: a-download-order-is-chosen
---

# Delta

## ADDED Requirements

### Requirement: The caller chooses the download order
`sync --download-order <largest|newest>` SHALL choose the order phase 2 of the one-source sync downloads bodies in; `largest` is the default. Under `largest`, bodies are ordered as before: by size, largest first, within each collection and, batch by batch, across the account. Under `newest`, each collection's bodies SHALL be ordered by their date (the mail summary's instant, from the store meta), newest first, then chunked into batches that stay within one collection, the batches queued across the account by their newest body. A body without a date SHALL come after every dated one, in plan order. Neither order adds a server request.

#### Scenario: Today's mail first
- **GIVEN** an Inbox holding a large mail from last year and a small one from today
- **WHEN** `sync --download-order newest` runs
- **THEN** today's mail is fetched and readable first

## MODIFIED Requirements

None.

## REMOVED Requirements

None.
