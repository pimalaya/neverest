---
cairn: tasks
change: a-dav-key-keeps-the-segment-served
---

# Tasks

- [x] `cargo update -p io-webdav` (0.9.1 to 0.11.0, nothing else moved); Cargo.toml requires "0.11.0"
- [x] src/dav/client.rs: keys from the href (`listed_key`, `served_segment`, `webdav_id`) in `list_collections`, `collection_key`, `reports`, `fetch_bodies`, `create`, `move_item`; `rename_collection` builds a `CaldavCalendarPatch`; `propfind_href` through `WebdavClientStd::run`, neverest's `run` removed
- [x] Searched every other place an io-webdav id becomes a key or is compared with one: none (see the proposal)
- [x] Tests, src/dav/client.rs: `a_default_calendar_url_is_keyed_as_a_listing_keys_it` (two escaped segments), `a_listed_collection_is_keyed_by_the_segment_served` (escaped, raw space, raw non-ASCII, a path id; the reports found under the key), `a_multiget_answers_each_member_under_the_segment_served` (CalDAV and CardDAV, bodies and gone rows, escaped and raw), `a_created_member_keeps_the_segment_its_location_served` (both kinds, escaped, raw, absolute with a query), `a_moved_member_keeps_the_segment_it_was_sent_under`; src/offline/remote.rs `a_batch_of_served_hrefs_finds_each_body`
- [x] Checked by a temporary edit: keyed on io-webdav's decoded `id`, all six tests fail; with `reports` looking the key up as it stands, `a_listed_collection_is_keyed_by_the_segment_served` fails (a REPORT sent first)
- [x] Fold into cairn/spec/sync.md, log, CHANGELOG
