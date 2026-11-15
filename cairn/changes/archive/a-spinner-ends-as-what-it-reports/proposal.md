---
cairn: change
id: a-spinner-ends-as-what-it-reports
status: landed
created: 2026-11-15
---

# A spinner ends as what it reports

## Why

Live testing of 2026-11-15 (ledger lines N13 and N19 of pimalaya/.github cairn/changes/cli-alignment-2026-11-15): `neverest check` printed a source that did not answer with the success mark (`✓ Source imap failed (…)`), src/cli/check.rs calling the spinner's `success` on both arms; and `neverest -a fastmail check 2>file` wrote colour escapes into the file, `NO_COLOR=1` changing nothing, which was pimalaya-cli's spinner (released fixed in 0.5.2, line L10).

## What

- The failed arms of a check's source and send channel call `Spinner::failure` (`✗`).
- pimalaya-cli 0.5.2 (`cargo update -p pimalaya-cli`, the lock moving that crate alone): no colour off a terminal nor under `NO_COLOR`.
