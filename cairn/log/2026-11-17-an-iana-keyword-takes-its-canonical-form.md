---
cairn: log
change: an-iana-keyword-takes-its-canonical-form
date: 2026-11-17
---

# An IANA keyword takes its canonical form

Finding NEV-16 of the CLI family analysis of 2026-11-15: a message flagged `\Junk`, `\Important` or `\Forwarded` (a spelling the store's gate accepts and a server may list) made the IMAP push panic on every run, `flag_from` building an atom from the stored spelling and `expect`ing it valid.

## What landed

- src/imap/backend.rs: `flag_from` writes an IANA keyword as stored when it is an atom, else in its canonical `$` form (`Flag::from_iana`).
- Test `an_iana_keyword_no_atom_spells_takes_its_canonical_form`, panicking on the old arm.
- Trivial fix: no proposal.

## Capabilities moved

- [sync](../spec/sync.md): *A flag push names what it adds and removes* amended.
