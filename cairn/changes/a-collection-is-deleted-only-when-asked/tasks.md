---
cairn: tasks
change: a-collection-is-deleted-only-when-asked
---

# Tasks

- [x] `config`: `collection.delete` defaults to false (`CollectionSourceConfig`, `CollectionPermissions`); docs, sample configuration and README.
- [x] `gcal`: `calendarList` asked with `showHidden`.
- [x] `msgraph`: contact folders listed page by page, children at every depth.
- [x] Tests, failing first: a source configured without `collection.delete` takes no collection delete; a Google calendar list request asks for hidden calendars; a second page of Graph contact folders and a grandchild folder are listed.
- [x] Spec folded, log written, CHANGELOG (Changed, breaking).
- [x] Verify: `cargo fmt`, `cargo clippy --all-features --all-targets -- -D warnings`, `cargo test --all-features`.
