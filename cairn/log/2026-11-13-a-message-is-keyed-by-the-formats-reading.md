---
cairn: log
change: a-message-is-keyed-by-the-formats-reading
date: 2026-11-13
---

# A message is keyed by the format's reading

pimdir audit line D2. Every read of a message's identity fields a source serves raw goes through io-pimdir 0.11's own derivation (pimdir STORAGE Annex A.1), and neverest parses no header field itself.

## What landed

- **IMAP** (src/imap/backend.rs): the listing already named a member by `derive_meta` over `BODY.PEEK[HEADER.FIELDS (…)]`. The `Meta` fetch did not: `fetch_envelopes` asked `ENVELOPE` and read it through chrono (`parse_from_rfc2822`), rfc2047_decoder and `normalize_message_id`. It is `fetch_summaries` now, over the same `fetch_metas` the listing uses, so a member is named one way whichever request reads it. `envelope_from`, `address_from`, `body_structure_has_attachment`, `build_item_names`, `parse_rfc2822_date`, `bytes_to_string` and `decode_mime_bytes` are gone, and the rfc2047-decoder dependency with them (Cargo.lock loses it and what it alone pulled: charset, quoted_printable, base64 0.22, and four more; io-pimdir unmoved at 0.11.0).
- **Gmail** (src/gmail/client.rs): `message_meta` lays the metadata headers out as a header block, each of `SUMMARY_HEADERS` from its first entry, and hands it to `derive_meta` with Gmail's size estimate, the way the Android app does. It replaces `message_summary`, which read each value through io-pimdir's decoders but the `Message-ID` through `normalize_message_id`, and unfolded nothing. The import's search of the label's history compares the hint as it stands, a derived id already, with the raw field read by `message_id`.
- **Graph** (src/msgraph/client.rs): `message_meta` builds `PimdirMailSummary` from a row, `internetMessageId` read by io-pimdir's `message_id`. The subject, `sentDateTime` and the addresses are Graph's own decoding and stand as it states them (Annex A.1), the date spelled as the UTC instant as before.
- **No summary struct of neverest's own**: src/item/summary.rs (`ItemSummary`, `normalize_message_id`, `parse_message_ids`) and src/item/address.rs (`Address`) are deleted, with `kind::mail::parse_summary` and `Kind::parse_summary`. `Client::fetch_summaries` answers each handle with its `PimdirDerivation`, which `PimRemote::fetch_meta` (src/offline/remote.rs) hands on.
- **The fixtures**: ten of pimdir's mail fixtures are copied byte for byte under tests/vectors/fixtures (mail-id-comment, mail-bracketless-id, mail-repeated-fields, mail-folded-subject, mail-military-zone and the five mail-charset ones; unchanged between pimdir ae8b231 and 4f42564), and src/kind/mail.rs holds, for tests, what vectors/summaries.json states for each at ae8b231: `link_id`, `message_id`, `in_reply_to`, `subject`, `sender`, `sender_name`, `date`, `sort_key` and the mark read without the body.
- src/imap/fake.rs: `FakeImap::set_header` has a message state a whole header block where a `FETCH` asks for header fields.

## Tests

New, each seen failing on the old reading before the change, in one run of three temporary tests feeding the fixtures to the old code (written and removed by hand, never through git):

- src/imap/backend.rs, `a_message_fetched_alone_is_keyed_as_the_vectors_state`: the ten fixtures through `fetch_summaries` over the fake server. The old path read another FETCH item, so the failing run had the fake answer the `ENVELOPE` a server builds from each fixture (RFC 3501 §7.4.2, the field values as written) to `fetch_envelopes` and `parse_summary`. It failed on all ten: mail-id-comment keyed `(via <relay@example.net>) <comment-1@example.org> (trailing)` where the vector states `comment-1@example.org`; mail-folded-subject keyed `alt: Café crème and\t  notes  |…` with its padding kept; mail-charset-replacement read `� kept` where the vector keeps the word literal; and every one stated no attachment mark where the vector states `0`.
- src/gmail/client.rs, `a_metadata_get_is_keyed_as_the_vectors_state`: the ten through `named_entry`, each fixture's fields served as metadata headers, a folded value with its line breaks. The old `message_summary` failed two: mail-id-comment keyed by its whole commented text, and mail-folded-subject keyed `alt:Café\r\n  crème and\r\n\t  notes|…`.
- src/msgraph/client.rs, `a_row_is_keyed_by_its_message_id_as_the_vectors_state`: the seven fixtures stating a `Message-ID`, the field's value as `internetMessageId`. The old `message_envelope` failed mail-id-comment the same way.

Moved with the code they test, their assertions kept: `a_metadata_get_folds_into_a_summary` (Gmail, over `named_entry`), `a_delta_row_folds_into_a_summary`, `the_attachment_mark_is_the_one_graph_states` and `the_summary_carries_the_expanded_size` (Graph, over `message_meta`). The three tests of `parse_summary` in src/kind/mail.rs went with it; what they held for the one connector still naming a member from decoded values is in src/msgraph/client.rs as `a_row_and_its_body_derive_one_key` (the `alt:` key, the sort key and the whole summary equal on both tiers) and `a_row_without_a_date_sorts_last`.

## Evidence

`cargo fmt --check` clean; `cargo clippy --all-targets -- -D warnings` clean with `--all-features`, the default features, `--no-default-features`, and `--no-default-features` with `search`, with `rustls-ring,dav,vendored`, with `rustls-ring,imap`, with `rustls-ring,gmail` and with `rustls-ring,msgraph`; `cargo check --all-features --tests` clean; `cargo test --all-features --bin neverest` 508 passed with the two other changes of the day (503 before: three tests of `parse_summary` removed, five added here, one by a-capped-source-says-so, two by a-graph-pass-states-no-guessed-removal). tests/accounts, check, drain, intents and usage pass (7, 1, 9, 9, 2). No server suite was run.

## Not done

- **Graph's `internetMessageHeaders`**, the raw read Annex A.1 and pimdir SYNC §4 name for Graph: io-msgraph 0.7.0 and 0.7.1 model no such property on `MsgraphMessage`, which every listing of the connector deserialises, so it cannot be asked for without a change to io-msgraph. Microsoft's message resource page says it "requires `$select` to retrieve" and says nothing of a delta. Until then a Graph member whose decoded subject, date or sender differs from Annex A.1's reading is minted apart from its own message where another reading wrote its row. The owner's: add the property to io-msgraph, then read it at least for a member whose decoded summary disagrees with a row holding its hint (SYNC §4).
- **What Gmail serves of a folded field** is not verified against Gmail: the test serves the line breaks kept, the case the old code got wrong, and `derive_meta` reads both that and a value served unfolded.
- **JMAP**: neverest opens no JMAP source, so nothing reads `header:…:asRaw:all` yet.
- A message an earlier build keyed otherwise keeps its key (a key is never derived again): a store holding one, written since the draft-10 follow, holds it under the old reading until it is reinstalled.
- The Android app strips `internetMessageId` with a parser of its own (android rust/src/mail/msgraph.rs, `graph_summary`) and runs `decode` over Graph's `subject`, which neverest does not: its own line of the audit.

## Capabilities moved

- sync (cairn/spec/sync.md): modified "The derivations are the format's" (the `Meta` tier per source, two scenarios), "Bodies are content-addressed and deduped" and "The summary is the format's typed row" (their wording of the `Meta` tier).
