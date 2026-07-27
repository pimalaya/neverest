---
cairn: delta
change: object-bytes-by-reference
---

## ADDED Requirements

### Requirement: Bodies transfer with bounded memory
A body SHALL be fetched and appended by streaming — fetched straight into the
blob store and appended straight from it — so no full message is held in memory;
peak memory is bounded to a chunk regardless of message size. The `Message-ID`
link id and the summary SHALL be read from the streamed header prefix, so no
extra pass over the body is needed.

## MODIFIED Requirements

(none)

## REMOVED Requirements

(none)
