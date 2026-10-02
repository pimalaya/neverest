---
cairn: tasks
change: a-graph-source-declares-its-domain
---

# Tasks

- [x] Move `cardamum/src/msgraph/project.rs` into io-msgraph beside the contacts resource, and point cardamum at it.
- [x] Carry the vCard `UID` through the extended-property stash: read it back as the link id, mint from the Graph id only for a contact Graph itself created, and refuse a write whose stash the server did not accept.
- [x] Round-trip tests: a card keeps its `UID` across a write and a read, an edit keeps its identity, and the stash remainder survives fields Graph has no slot for.
- [x] `GraphKind` on `GraphClient`, `Client::media_type` forwarding to it rather than answering a constant.
- [x] `list_collections` over `contact_folders_list` and `contact_child_folders_list`, the sentinel Contacts folder mapped to the omitted segment. Keyed by folder id rather than `Parent/Child` names.
- [x] `enumerate` over `contacts_delta`, the same `@odata.deltaLink` checkpoint and HTTP 410 restart mail uses, `revision` from `change_key`, flags known-empty.
- [x] ~~`fetch_bodies` projects the cached delta row, no second round trip; `add_item_stream` and `update_item_stream` push through `contact_create` and `contact_update` conditionally on `change_key`.~~ Bodies are read per contact with the stash expanded instead (the delta's `$expand` support is undocumented); writes are guarded by reading the current `changeKey`, Graph having no conditional write.
- [x] ~~The contacts delta carries the `$expand` clause the stash needs, held by a test.~~ Superseded by the per-contact read above.
- [x] Kind-aware `sends_natively()`, `carries_mail()` and the `smtp` refusal, so a contacts Graph source is never the send channel.
- [x] `Kind::Vcard` and `Kind::Ical` gate on the kind, not on the `dav` cargo feature.
- [x] `msgraph-contacts` backend and sugar, reusing `MsgraphConfig` and `MsgraphAuthConfig` rather than a twin type.
- [ ] Wizard entry naming the `Contacts.ReadWrite` scope the token needs, and an ortie README fix for the Graph scope set.
- [ ] End-to-end test against a real tenant, in the shape of `tests/carddav.rs`.
- [x] README.md, CHANGELOG.md and config.sample.toml, saying outright that Graph carries no calendar.
- [x] Put `msgraph` back in the default feature set, reverting `msgraph-waits-for-its-domains`.
- [x] Fold the delta into cairn/spec/sync.md; log; land.
