---
cairn: change
id: a-mass-absence-waits-for-a-person
status: landed
created: 2026-10-30
---

# A mass absence waits for a person

## Why

pimdir sync-draft-05 (SYNC §5, §8) and io-pimdir's a-listing-proves-what-it-drops: a round whose last page finds more than half of at least 20 based bindings in its scope absent parks rather than closing, dropping nothing, and a rekey whose listing leaves most of the collection out parks, writing nothing, unless the caller confirms the absences (`PimdirSyncOptions::accept_absences`, `PimdirRekey::accept_absences`). neverest had no way to confirm them and reported nothing: a parked round read as a quiet run, and a parked rekey left the renumbered mailbox failing its listing on every sync.

## What

- `sync --accept-absences`, off by default and for one run, never stored: the sync and the rebuild of a renumbered IMAP mailbox drop the absences they find.
- A parked round or rebuild is reported under `absences` (JSON `absences: [{ side, collection, absent, bound, rebuild }]`, text in the warnings), naming `--accept-absences`.
- Exit code 2, not 3: a rerun alone meets the same listing and parks again, so the run waits for a person, who confirms the absences or finds why the server answered short. The round a park leaves open does not count as incomplete; any other open round still does, and exit 3 still wins over 2.
- A parked rebuild ends the collection's sync for the run, its old handles and checkpoint kept, where the sync would list it again and fail on the renumbering.
