---
cairn: log
change: a-graph-message-is-read-raw
date: 2026-11-13
---

# A Graph message is read raw

pimdir audit line D2, its Graph half, left open by a-message-is-keyed-by-the-formats-reading (3451138): a Graph mail member is named off its raw header fields through io-pimdir's `derive_meta`, as an IMAP and a Gmail one are, wherever Graph answers them, and from the values Graph decodes only where it answers none (pimdir STORAGE Annex A.1 as corrected on 2026-11-13).

## What landed

- **The pin** (Cargo.toml, Cargo.lock): io-msgraph by git at 92c28d5 in `[patch.crates-io]`, the dependency line staying `0.7`. Cargo.lock held 0.7.0 from crates.io, so `cargo update -p io-msgraph` moved that one package, to 0.7.1 at the commit: 0.7.1's own changes come with it: the To Do surface, which neverest does not use, and a stashed alarm restored whole by `MsgraphEvent::to_ical_series` (io-msgraph 67ab457), which the calendar source reads every event through. That fix has no CHANGELOG.md line here and no test of neverest's own.
- **The select** (src/msgraph/client/mail.rs): `SUMMARY_SELECT` names `internetMessageHeaders`. It is the one `$select` of every read naming a member: the whole-folder delta (`delta_first`), a band page (`band_first`), a listing by arrival (`received_first`), the summary batch a delta or the pass reads its unbound members by (`metas`) and the `Meta` fetch (`fetch_metas`). `IDS_SELECT`, the pass closing a round under a scope, is unchanged: it names no member.
- **The reading** (src/msgraph/client.rs, `message_meta`): a row listing header fields has the nine a summary reads (`SUMMARY_FIELDS`: `Message-ID`, `In-Reply-To`, `Subject`, `From`, `To`, `Cc`, `Bcc`, `Date`, `Content-Type`) laid out as a header block, each from its first entry of that name, compared without regard to case, and handed to `derive_meta` with the MAPI size and Graph's `hasAttachments`. So `link_id`, `subject`, `date`, `sender`, `sender_name`, `in_reply_to` and the address lists are the format's reading; the size and the flags are Graph's own as before, and the mark is `hasAttachments`, the top-level `Content-Type`'s only on a row stating none, which `derive_meta` reads then (Annex A.1). A row listing none keeps the reading of 3451138 unchanged, in the same function.
- **The `Meta` fetch** (`fetch_metas`): over `MsgraphClientStd::message_get`, which takes a select and an expand since 92c28d5, where it built its URL by hand over `MsgraphSend::get`. The request line is the same octet for octet for an id of Graph's making (`every_read_naming_a_member_selects_its_header_fields` states it whole, and passed on the hand-built URL before the move); the id is now percent-encoded as one path segment, and an empty one refused.
- **A delta's kept rows** (src/msgraph/client/mail.rs, `Named`): a delta following a summary link kept the whole row of each member it names until its last page. A row now carries every header field, so it keeps what the page lists of it (the entry, the stated size, the row's octets), built as the row is read. `Listing::named` builds the same `Named` and lists it at once. It is what "A Graph listing keeps a few octets a message" (cairn/spec/sync.md) already required, the summary and not the row; no listing test changed.

## What io-msgraph moved, and what each required

- 7480799, ids percent-encoded as one path segment, `user_path` included: no edit. Every id neverest passes a coroutine is one Graph minted (base64url, which io-msgraph's own log says passes unchanged) or a well-known folder name, and the scripted-server tests assert the request lines they asserted before. An id holding another byte is now encoded, and an empty one refused before any request. The URLs neverest builds itself still splice the id raw: the raw-body batch and the size batch (src/msgraph/client.rs), the summary batch (src/msgraph/client/mail.rs) and `message_gone`. Not changed here.
- 7480799, `MsgraphMessage::categories` a `MsgraphField`: no edit. neverest reads and writes no category; a flag patch is built with `..Default::default()`, the field unset and left out of the body (`a_flags_patch_touches_only_the_flag_fields` unchanged).
- 7480799, `MsgraphMessagesListResponse::count`: two struct literals of the fake Graph folder (src/msgraph/client/mail.rs tests, `band_page` and `received_page`) take `..Default::default()`. No production line.
- 92c28d5, `MsgraphMessageGet::new` and `message_get` take a select and an expand: neverest called neither. `fetch_metas` now does (above).
- 92c28d5, `MsgraphMessage::internet_message_headers`: no literal broke, every one neverest writes ending in `..Default::default()`.

## What a row now carries

Every header field of its message as a name and a value, in Graph's order: the nine read, and all the others (the trace fields, the authentication results, the server's own), `$select` naming the collection whole or not at all. The octets a page reports (`Enumeration::bytes`, the source's download count) are the row's as listed, so they grow by as much.

The page sizes are unchanged: 1000 asked on a delta page (Graph caps a message delta at 512) and on a band page, 512 on a listing by arrival, 20 a summary batch. No row and no page was measured: no live suite was run, and the three seconds a delta page of a thousand took on 2026-10-07 were measured with the select of then (the comment on `PAGE_SIZE` says so now). What bounds a page here is io-msgraph's 256 MiB a body and the 30 seconds a read waits (`wire::READ_TIMEOUT`), after which a listing fails and is not sent again.

## Tests

New, in src/msgraph/client.rs, each seen failing on the old code in one run of `cargo test --all-features --bin neverest -- msgraph::client::tests::a_row_listing msgraph::client::tests::every_read`, the tests written before `message_meta` was touched:

- `a_row_listing_its_header_fields_is_named_as_the_vectors_state`: the ten fixtures, each row listing the fixture's fields as name and value pairs in document order, repeated names kept, beside a `subject`, a `sentDateTime`, a `from` and an `internetMessageId` stating another message. On the old `message_meta` it failed on the first fixture, keyed `decoded@example.net` where the vector states `comment-1@example.org`: the old code read the decoded members alone.
- `a_row_listing_its_header_fields_keeps_the_size_and_the_mark_graph_states`: a row listing `In-Reply-To` and a `multipart/mixed` `Content-Type`, its size expanded: the size is Graph's, the mark `hasAttachments` when stated and the `Content-Type`'s when not. On the old code: `in_reply_to` empty where the field states `p-1@example.org`.
- `every_read_naming_a_member_selects_its_header_fields`: over the scripted server, a band page, a listing by arrival, a whole-folder delta, a summary batch and a `Meta` fetch each ask for `internetMessageHeaders`, the pass's delta does not, and the batch's and the fetch's answers are named as the vector states. On the old `message_meta` it failed on the naming; then, `message_meta` in place and `internetMessageHeaders` taken out of `SUMMARY_SELECT` by hand for one run, on the band page's request line, which named no such member. The constant was put back by hand, never through git.

A row listing no header field keeps the decoded reading: the tests of 3451138 hold it and pass unchanged, their rows listing none (`a_delta_row_folds_into_a_summary`, `a_row_is_keyed_by_its_message_id_as_the_vectors_state`, `a_row_and_its_body_derive_one_key`, `a_row_without_a_date_sorts_last`, `the_attachment_mark_is_the_one_graph_states`, `the_summary_carries_the_expanded_size`), as do the listing tests over the fake Graph folder (src/msgraph/client/mail.rs), whose rows list none either. No test changed its assertions; the two struct literals above are the only test lines edited.

## Evidence

Each as `nix develop --command <cmd>`, on 2026-11-13:

- `cargo fmt --check`: clean.
- `cargo clippy --all-targets -- -D warnings`: clean with `--all-features`, with the default features, with `--no-default-features`, and with `--no-default-features --features` `search`, `rustls-ring,dav,vendored`, `rustls-ring,imap`, `rustls-ring,gmail` and `rustls-ring,msgraph`.
- `cargo check --all-features --tests`: clean.
- `cargo test --all-features --bin neverest`: 512 passed (509 on the new pin before any test was added, the count of 37fe6cf; three added).
- `cargo test --all-features --test accounts --test check --test drain --test intents --test usage`: 7, 1 (2 ignored), 9 (1 ignored), 9 and 2 passed.

No server suite and no live suite was run, and nothing connected to an account.

## Not done

- **A draft**: what Graph answers for `internetMessageHeaders` on a draft is not known (io-msgraph's probe found Drafts empty). An empty list or no member reads as a row listing none. A `null` reads the same since io-msgraph 92c28d5, the commit pinned: 9a203bb refused it (`invalid type: null, expected a sequence`, seen in a throwaway test), which would have failed the whole page such a row was listed on.
- **A row listing header fields but none a summary reads** is named off them, so with no subject, date or sender, whatever Graph decodes beside them. Annex A.1 reads it so; whether Graph answers such a row is not known.
- **A stored delta link**: Graph's delta link carries the `$select` it was made under, so a `summary` link stored before this change keeps answering rows without header fields, and its new members keep the decoded reading until a round makes a new link. Only a store a build at 37fe6cf wrote can hold one, every earlier store being refused (draft-11), so it was left; refusing such a link (a tag of its own, at the cost of one round) or reading by the summary batch the unbound rows that list none are the two ways to close it.
- **The scope under Graph** is still read off `sentDateTime` (`in_scope`, the band filter, the cursor below a second): it is what Graph filters and orders by. A row's `date` is now its `Date` field's, which the engine's own check reads (`keep_in_scope`), so a message whose `Date` Graph reads otherwise than Annex A.1 is in scope only where both agree, as an IMAP one under `SENTSINCE` already is.
- **What Graph serves in a value**: whether a folded field comes folded, and an encoded word encoded, is not verified against Graph. `derive_meta` reads both.
- **The page sizes** against Graph with the larger rows, above.
- The Android app's Graph reading (android rust/src/mail/msgraph.rs) is its own line of the audit.
- A message an earlier build keyed otherwise keeps its key.

## Capabilities moved

- sync (cairn/spec/sync.md): modified "The derivations are the format's" (what it says of Graph, the scenario of the folded subject, two scenarios added).
