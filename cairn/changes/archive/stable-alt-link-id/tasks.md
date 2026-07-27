---
cairn: tasks
change: stable-alt-link-id
---

- [x] `envelope_date` helper: `to_rfc3339_opts(SecondsFormat::Secs, true)` (Z for
      UTC), matching `mail_parser`.
- [x] `envelope_link_id` and `envelope_meta` use it (Meta date now matches Full).
- [x] Unit test: Meta and Full link ids byte-identical for UTC and offset dates.
- [x] Diagnose in the live store (13 stuck level-1 items, each twinned with a
      level-2 Full item under a `+00:00`/`Z`-diverging link).
- [x] Verify a cleanup query removes exactly the 13 ghosts, cascades bindings,
      keeps the 8305 healthy items (tested on a copy; production untouched).
- [x] fmt/clippy clean; 15 tests pass.
- [ ] Fold delta into `cairn/spec/sync.md`; write log entry.
