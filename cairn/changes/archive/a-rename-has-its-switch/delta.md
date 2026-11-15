---
cairn: delta
change: a-rename-has-its-switch
---

# Delta

## ADDED Requirements

## MODIFIED Requirements

### Requirement: Every source declares its capabilities before the drain
"A one-way source SHALL support no write" becomes: no item or flag write; a collection action only where its switch allows it; the scheduling intents stay allowed.

### Requirement: A collection is renamed or deleted through the queue
`collection.rename` is `none` where the source's switch is off, unset reading the account's `one-way`; scenario *A rename on a one-way account* added.

## REMOVED Requirements
