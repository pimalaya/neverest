---
cairn: tasks
change: conflicts-merge-before-they-park
---

- [x] Depend on vcard-rs and ical-rs behind a feature, default on
- [x] Merge the three bodies on a marked conflict, dispatched on collection kind
- [x] Stage an empty report as an edit through the queue; park anything else
- [x] Count outstanding conflicts from the store, not from the run's report
- [x] Exit 2 when a run synced and left conflicts behind
- [x] Notify on entry into conflict, opt-in, log by default
- [x] `neverest conflict list|show`, with output types (Display and Serialize, neverest carrying no schema registry)
- [x] `neverest conflict resolve --prefer-local|--prefer-remote`
- [x] A config key naming the interactive merger, reusing the command adapter
- [x] `neverest conflict resolve --interactive`, positional paths, base first
- [x] Refuse a resolution whose recorded revision moved, and re-export
- [x] Test: disjoint edits on both sides resolve with no report
- [x] Test: a same-field collision parks and the run still exits 2
- [x] Test: a second run over an unresolved conflict notifies nothing
- [x] Test: a resolution against a moved revision is refused
- [x] Test: a merger exiting non-zero, or leaving its output untouched, changes nothing
