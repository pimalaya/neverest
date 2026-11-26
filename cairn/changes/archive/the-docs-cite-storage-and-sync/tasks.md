---
cairn: tasks
change: the-docs-cite-storage-and-sync
---

# Tasks

- [x] "pimdir SPEC" citations in src/, tests/ and cairn/spec/ name STORAGE (§5, §8, §9, §9.2, §13, Annex A), checked against pimdir STORAGE.md of 2026-10-30; the known-empty flag rule cited as STORAGE §13 (Encodings), where it lives, not Annex A. Logs keep their old citations as history.
- [x] src/offline/driver.rs module header, cairn/spec/sync.md `PimdirDeletePolicy::Keep`, src/kind/merge.rs header fixed.
- [x] CHANGELOG [Unreleased] as a net diff: commit hashes, revisions and pimdir draft numbers out, fixes of features added in the same window folded into them, one line per bumped dependency, the upgrade note in (0.3.0 stores migrate in place, master-build stores need `sync --reset --force`, `collection.delete` default, `--reset` refusals and `--force`, exit codes 2 and 3).
- [x] Moot (2026-11-26): the 1.0.0 section, rewritten by docs-aligned-for-the-major (2026-11-25), states the final state and carries no dependency lines. Was: Dependency lines checked against the published versions once the user drops the `[patch.crates-io]` pins: io-imap, io-webdav, io-http and pimalaya-cli ride unreleased revisions of an unchanged minor and get a line only if their release bumps it. 2026-11-19: no `[patch]` is left, every dependency is a crates.io release; io-imap 0.12 and pimalaya-cli 0.7 (with io-smtp, io-sasl, io-pimdir 0.18, ical-rs) have their line, io-webdav, io-http, io-gcal, io-gmail, io-gpeople, io-msgraph, vcard-rs and pimalaya-stream, moved since 0.3.0, still have none.
- [x] Spec folded, log written. The delta changes no requirement (citations and wording only); checked 2026-11-26: no "pimdir SPEC" nor `PimdirDeletePolicy` left in src/, tests/ or cairn/spec/; log 2026-11-26-the-docs-cite-storage-and-sync.
