---
cairn: log
change: the-live-night-findings
landed: 2026-11-24
---

# The released libraries

neverest moves off its git pins onto the libraries released on 2026-11-24: io-pimdir 0.20.0, io-webdav 0.14.0, io-msgraph 0.11.0, io-gpeople 0.9.1 and pimalaya-cli 0.8.1. No `[patch.crates-io]` entry remains.

- N4's rest: a send still pending after the run that renamed its copy's folder (an unreached SMTP server, say) named an id gone from the store, and the later run's `add` failed its foreign key. io-pimdir 0.20.0's `rename_collection` carries a pending `submit`'s `copy` to the new id, so the later run files the copy in the renamed folder. Capability moved: sync.md, "A sent copy is filed once sent" (the earlier-run limitation dropped, a scenario added).
- io-msgraph 0.11.0 leaves an empty body out of an event create itself, so neverest's own `is_empty_body` in src/msgraph/client/calendar.rs is gone; behaviour unchanged.
- io-webdav 0.14.0's `WebdavSyncDelta::unreadable` was already read through the pin neverest asked for (6ff2e8b): a member row answered with an error makes the enumeration incomplete and keeps its checkpoint. Nothing moved.

## Checked

- src/sync/driver/collection_intent_tests.rs `a_send_pending_across_a_rename_files_its_copy_on_a_later_run`: renames the copy's folder in one store session, reopens the store, sends; the copy is an `add` in the renamed folder.
- src/msgraph/client/calendar.rs `an_event_without_description_is_created_with_no_body` passes without the workaround.
