---
cairn: change
change: pimdir-errors-carry-their-code
---

# Delta

## ADDED Requirements

### Requirement: A store error carries its code
A store failure that ends a command SHALL carry, in its JSON failure object, the code io-pimdir gives it (`PimdirError::code`), spelled the same: `store-newer`, `store-older`, `store-uncreated`, `store-corrupt`, `store-not-current`, `store-stale`, `store-hash-algo`, `store-busy`, `store-owned`, `store-staging`, `store-in-flight`, `unsupported`. A failure a caller cannot act on (an SQL or I/O error, a malformed action) SHALL carry none. No message SHALL advise deleting the store: the remedies are an upgrade, `sync --reset` on a store holding no pending work, and `sync --reset --force`, which moves it aside.

#### Scenario: A downgrade under `--json`
- **GIVEN** a store written by a newer neverest
- **WHEN** an older neverest runs `--json drain` on the account
- **THEN** it exits 1 printing a failure object whose `code` is `store-newer`

### Requirement: A body reaches the blob tree under a lock
A body neverest writes SHALL go through the write side io-pimdir hands a lock holder: a fetched body streams into the owner's stage (`PimdirStore::blob_stage`), which hashes it as it passes, and a settled or merged conflict body is staged by the producer that enqueues it (`PimdirProducer::stage_body`). neverest SHALL NOT hash or write a body itself.

## MODIFIED Requirements

### Requirement: A queued submission is a `submit` queue intent
In its second paragraph, the payload is read by io-pimdir's `PimdirSubmit` (pimdir STORAGE Annex B.2), whatever the order of its keys: `v: 1`, `object` (the body hash, required, and the hash the row pins), `from` (empty means the null reverse path), `rcpts` (at least one), and an optional `subject`, `source` and `copy`. A payload `PimdirSubmit` refuses, or one naming another body than the row pins, SHALL park as undecodable. In its first paragraph, the kind is defined by pimdir (Annex B.2) and typed by io-pimdir, and an owner lacking the capability still skips the row.

## REMOVED Requirements

None.
