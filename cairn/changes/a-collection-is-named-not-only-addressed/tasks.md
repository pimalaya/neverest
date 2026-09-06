---
cairn: tasks
change: a-collection-is-named-not-only-addressed
---

# Tasks

- [x] `list_collections` answers id against display name; `filter_collections` and every `hub_id` key on the id.
- [x] `src/dav/client.rs` keeps `DAV:displayname`, falling back to the path segment when it is blank or absent.
- [x] `declare_name` beside `ensure_collection` on both paths, first source with something to say winning.
- [x] Test: a CalDAV collection is named `Work` while its id keeps `caldav/`, and a silent source falls back to the bare id.
- [x] CHANGELOG under `### Changed`.
- [x] Fold the delta into cairn/spec/sync.md; log; land.
