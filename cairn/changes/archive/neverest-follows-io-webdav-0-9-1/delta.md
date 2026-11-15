---
cairn: delta
change: neverest-follows-io-webdav-0-9-1
---

# Delta

## ADDED Requirements

### Requirement: A DAV id stays inside what it names
A collection key or a member id that would address the server root, the home set or a parent of either is refused before any request, its row failing with io-webdav's reason, uncoded. One scenario.

## MODIFIED Requirements

### Requirement: DAV collections enumerate by sync token and resolve at Full
A body a multiget or a listing reads is stored as a GET returns it; one stored short before stays until its ETag moves.

## REMOVED Requirements
