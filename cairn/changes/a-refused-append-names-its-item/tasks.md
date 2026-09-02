---
cairn: tasks
change: a-refused-append-names-its-item
---

- [x] Find where the synthetic handle is made (io-replica `created_placement`: the link id with `\u{1}hub` appended) and confirm the report prints it raw
- [x] `itemize_rejected` (src/offline/driver.rs) takes the item's name from the part before the marker, for the warning and for the retraction alike
- [x] `names` matches a `Copy` by `target_side` and `source_id`, so a refused append takes its copy hunk back; a `Fetch` still matches nothing, reaching no server
- [x] Unit test `a_refused_append_is_named_by_its_link_id_and_its_copy_is_taken_back`, which also asserts the marker never reaches the rendered text
- [x] Re-run against the read-only iCloud calendar that produced the defect: names read plainly, the patch is empty, the run exits 2
- [x] `cargo test --all-features`, the whole live suite, `cargo clippy --all-features --all-targets`, `cargo fmt`
- [x] Fold the delta into cairn/spec/sync.md and write the cairn/log entry
