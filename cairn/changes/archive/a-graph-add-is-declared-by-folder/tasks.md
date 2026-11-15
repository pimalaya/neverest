---
cairn: tasks
change: a-graph-add-is-declared-by-folder
---

# Tasks

- [x] (a) `capability::drafts_only`, called by `declare` with the collections stating the drafts role; test `a_graph_source_takes_an_add_in_its_drafts_alone` (a fake Graph store, the gate refusing an add into `Archive` and queuing one into Drafts), failing without the call
- [x] (b) `add_message` codes the rejection `unsupported`; test `an_add_outside_drafts_is_unsupported`, failing on the code before
- [ ] (c) a never-pushed item droppable whatever the source's rights: io-pimdir's gate (reported, no code here)
- [x] Fold into cairn/spec/sync.md, log, CHANGELOG
