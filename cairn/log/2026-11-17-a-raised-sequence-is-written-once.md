---
cairn: log
change: a-raised-sequence-is-written-once
date: 2026-11-17
---

# A raised sequence is written once

Finding NEV-11 of the CLI family analysis of 2026-11-15: the merge raising a component's `SEQUENCE` to the remote's wrote the line before every `END` of a component that held none, so an event with an alarm got it inside its `VALARM` and again before `END:VEVENT`.

## What landed

- src/conflict/merge.rs: `with_sequences_of` writes the missing line before the dated component's own `END` alone.
- Test `a_raised_sequence_is_written_once_outside_the_alarm`, failing on the old arm.
- Trivial fix: no proposal, no spec change (the spec states no placement).
