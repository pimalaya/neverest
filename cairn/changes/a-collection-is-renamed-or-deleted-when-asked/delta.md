---
cairn: delta
change: a-collection-is-renamed-or-deleted-when-asked
---

# Delta

## ADDED Requirements

### Requirement: A collection is renamed or deleted through the queue

Folded into cairn/spec/sync.md under that heading.

## MODIFIED Requirements

### Requirement: A performed intent leaves a receipt

`collection-rename` and `collection-delete` join the acknowledged intents.

### Requirement: A check reports what each source declares it can do

`collection.rename` and `collection.delete` read as `collection.create` does.

### Requirement: Gmail is a native mail source

Gmail declares `mail.flags.important` `full` and `mail.flags.junk` `none`.

### Requirement: A declaration syncs no item

It performs the rename and delete intents with the creations.

### Requirement: A run may perform the queued intents alone

It performs the rename and delete intents with the creations.

## REMOVED Requirements

None.
