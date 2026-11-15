---
cairn: tasks
change: the-merge-is-not-a-build-option
---

- [x] Fold `dep:ical-rs` and `dep:vcard-rs` into `dav` and drop the `merge` feature
- [x] Gate the two mutable arms of `Kind::merge` on `dav`, leaving the mail arm unconditional
- [x] Delete the `cfg(not(feature = "merge"))` impl and the rebuild message it carried
- [x] Repoint every remaining `feature = "merge"` gate, tests included, at `dav`
- [x] Correct the module header and the notes that named the feature
- [x] Verify a dav-less build still compiles, lints clean and answers mail unmergeable
