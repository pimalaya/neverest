---
cairn: tasks
change: a-graph-source-declares-its-domain
---

# Tasks

- [ ] Move `cardamum/src/msgraph/project.rs` into io-msgraph beside the contacts resource, and point cardamum at it.
- [ ] Carry the vCard `UID` through the extended-property stash: read it back as the link id, mint from the Graph id only for a contact Graph itself created, and refuse a write whose stash the server did not accept.
- [ ] Round-trip tests: a card keeps its `UID` across a write and a read, an edit keeps its identity, and the stash remainder survives fields Graph has no slot for.
- [ ] `GraphKind` on `GraphClient`, `Client::media_type` forwarding to it rather than answering a constant.
- [ ] `list_collections` over `contact_folders_list` and `contact_child_folders_list`, the sentinel Contacts folder mapped to the omitted segment.
- [ ] `enumerate` over `contacts_delta`, the same `@odata.deltaLink` checkpoint and HTTP 410 restart mail uses, `revision` from `change_key`, flags known-empty.
- [ ] `fetch_bodies` projects the cached delta row, no second round trip; `add_item_stream` and `update_item_stream` push through `contact_create` and `contact_update` conditionally on `change_key`.
- [ ] The contacts delta carries the `$expand` clause the stash needs, held by a test.
- [ ] Kind-aware `sends_natively()`, `carries_mail()` and the `smtp` refusal, so a contacts Graph source is never the send channel.
- [ ] `Kind::Vcard` and `Kind::Ical` gate on the kind, not on the `dav` cargo feature.
- [ ] `MsgraphContactsConfig` and its `msgraph-contacts` direct-backend sugar, sharing `MsgraphAuthConfig`.
- [ ] Wizard entry naming the `Contacts.ReadWrite` scope the token needs, and an ortie README fix for the Graph scope set.
- [ ] End-to-end test against a real tenant, in the shape of `tests/carddav.rs`.
- [ ] README.md, CHANGELOG.md and config.sample.toml, saying outright that Graph carries no calendar.
- [ ] Put `msgraph` back in the default feature set, reverting `msgraph-waits-for-its-domains`.
- [ ] Fold the delta into cairn/spec/sync.md; log; land.
