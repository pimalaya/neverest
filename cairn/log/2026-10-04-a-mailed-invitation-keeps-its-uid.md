---
cairn: log
change: a-mailed-invitation-keeps-its-uid
date: 2026-10-04
---

# A mailed invitation keeps its UID

Microsoft 365 files an invitation it took in by mail under Exchange's global object id, which wraps the organizer's UID (the `vCal-Uid` form, `040000008200E00074C5B7101A82E008…` with the UID hex-encoded inside) rather than keeping it. A Graph calendar source stored such an event under that id, so a consumer that knew the invitation by its UID (from the mail's `.ics`, from another calendar) had to rebuild the wrapper to find it, and unwrap it to show it.

## What landed

- io-msgraph `43f65f9`: the iCal projection unwraps a global object id that wraps a UID (`ical::original_uid`); one Exchange minted itself, with no UID inside, stays as it is. A stashed UID still wins.
- Cargo.toml: io-msgraph by git rev until its next release.
- Nothing searches Graph by UID: writes, replies and cancellations address an event by its Graph id, bound through the store, so they match as before.

## Upgrade note

Stored UIDs change at the next sync for the events concerned: their projection now states the original UID, and the store keys them by it. A consumer holding the wrapped id finds the event under the original UID afterwards.

## Capabilities moved

- sync: a Graph event with no stashed UID is keyed by the UID its global object id wraps.
