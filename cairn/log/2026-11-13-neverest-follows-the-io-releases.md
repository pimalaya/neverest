---
cairn: log
change: neverest-follows-the-io-releases
date: 2026-11-13
---

# neverest follows the io-* releases

neverest moves onto the family releases of 2026-11-13 and closes, with what they hand over, the defects the family audit of the same day recorded on its side: a batched IMAP fetch that failed was read again on a connection still holding its answer (IMAPB-S1), the SMTP hand-over built its envelope unchecked and reset the transaction itself, a Google contact update erased what its vCard cannot show (GPEOPLE-02), and a DAV body that is no multistatus was a shorter listing (DAV-01).

## What landed

- **The requirements** (Cargo.toml, Cargo.lock): io-gcal 0.4, io-gmail 0.7 (from 0.5), io-gpeople 0.7, io-http 0.7, io-imap 0.9, io-msgraph 0.8, io-sasl 0.2, io-smtp 0.8 (from 0.6), io-webdav 0.8, and io-oauth 0.6 (from 0.3) for the live suites. The git pins of io-http (e181530) and io-msgraph (92c28d5) are gone from `[patch.crates-io]`, their commits being in io-http 0.7.0 and io-msgraph 0.8.0; the pimalaya-cli pin is unchanged. io-sasl's `saslprep` feature no longer exists, preparation being unconditional. The lock holds one io-http (0.7.0), one io-sasl (0.2.0) and one io-oauth (0.6.0): io-pimdir, pimalaya-config, pimalaya-cli and pimalaya-stream link none of them. `cargo update` refused the name `io-http` as ambiguous, the old lock holding 0.5.0 from the pin and 0.6.0 from the registry, so the lock was re-resolved by the first `cargo tree`: it moved the ten crates, and rsa to 0.10.0 with crypto-bigint to 0.7.11, which io-oauth 0.6 requires for the live suites alone.
- **A failed IMAP batch** (src/imap/client.rs, src/imap/backend.rs, src/client.rs, src/offline/remote.rs): `ImapClient` keeps what it opened from and `reconnect` opens it again in place, nothing selected, the UIDs a round has left kept. `ImapClient::recover` reads the error of a batched body fetch: one carrying a tagged `NO` or `BAD` (`ImapClientError::status`) leaves the connection as it is, the answer having been read whole; any other opens it again. `hydrate_batch` calls it through `Client::recover` before it reads the batch's items one by one, and fails the batch when the connection does not open again. The other backends answer nothing there: each reopens its own HTTP connection.
- **An IMAP answer read at one seam** (src/imap/backend.rs): io-imap 0.9 returns the data of a command with the server's alerts and the lines it skipped for not decoding, which 0.8 dropped. `rows_of` fails on a skipped line and reads every `UID SEARCH` of a listing, the `UID FETCH` of a page's flags and header fields, and the search of `find_message`; `data_of` warns of the count and reads `SELECT`, `LIST`, `STATUS` and the search after an `APPEND`. Both warn of an alert. This was not on the audit's list for neverest: it is the same loss as a listing read short, a page left without a row stating its message removed, and io-imap's own watch refuses to infer a removal from such an answer.
- **The SMTP hand-over** (src/offline/submit.rs): `smtp_mailbox` is `SmtpMailbox::try_from`, so an address that is no mailbox or holds a line break fails its intent as permanent, where a string cut at its last `@` went to the command line as it was. `reply_of` reads `RcptTo { error, .. }`. neverest's own `RSET` after a refusal is gone, io-smtp sending one after a refused recipient or a refused `DATA` command; a `421` reply, after which io-smtp sends none, loses the channel. `hand_over_smtp` takes the stream alone and counts the hand-over on a write ending with the data's `.` line: the `RSET` after a refused `DATA` command is written at the place of the data, and a connection failing under it was read as a message perhaps delivered. `SmtpSessionOpenOptions` takes its defaults for the SCRAM bounds.
- **A Google contact update** (src/gpeople/client.rs): `from_vcard` returns the person with the properties too long for the stash, which a create and an update name in a warning. The update calls `merge_unprojected` with the server's person and the mask, in place of the merge of `clientData` neverest wrote itself, which covered that field alone.
- **The Calendar throttle** (src/gcal/client.rs, src/throttle.rs): `GcalSendError::Api` carries a `GcalApiError`, whose reasons, detail reasons and canonical status the throttle reads as it reads Gmail's and People's, where it had the message alone.
- **Graph** (src/msgraph/client.rs and its modules): `MsgraphSendError::Api` holds a boxed `MsgraphApiError`; `user_path` returns a `Result`, read once in `fetch_messages` and handed to `stated_sizes`; the recipients of a message are `MsgraphField`s; a page pattern names `count`.

## What the releases moved, and what each required

- **io-imap 0.9**: `ImapOutput` (above). COPY and MOVE return `Option<ImapCopyUid>`: neverest reads no COPYUID. `ImapMessageFetchStream::new(id, opts)`, the watch options, `ImapAuthScramSha256::new`: neverest calls the client's methods, none of them changed in shape. PLAIN and SCRAM prepare their credentials: neverest already built io-sasl with `saslprep`, so nothing moves.
- **io-smtp 0.7.0, 0.7.1, 0.8.0**: above. `SmtpMessageSend` returns the refused recipients, empty as neverest skips none. The data is still one write. `Resent-Bcc` is removed with `Bcc`.
- **io-webdav 0.8**: no production line. A body that is no whole multistatus fails `sync-collection`, the `PROPFIND` listing, the collection listing and a multiget; neverest's `enumerate` reads only a rejected token and an unsupported report as something else than a failure, so the listing fails (two tests). A create returns the last segment of the URL sent: neverest names a resource from letters, digits, `-`, `_`, `.` and `%3A`, which that segment equals. A weak ETag is kept whole, so a store holding one cut short by 0.7 meets another revision for that member: what a run makes of it was not run. `UnexpectedRedirect` is matched nowhere.
- **io-gcal 0.4**: ids are encoded by the crate. neverest encoded none: it passes a calendar and an event id as the API returned them, and no line was removed. `from_ical` can fail with `StashFull`, which fails the push of that event with the library's message. The enums that gained `Unknown` are matched whole nowhere.
- **io-gpeople 0.7**: above. A resource name is validated: neverest passes `people/<id>`.
- **io-gmail 0.6, 0.7**: no edit. `decode_raw` fails with its own error, which neverest prints. A label of a type the crate does not know is no collection, where it failed the label listing. neverest uses neither the history watcher nor `GmailSend::get`.
- **io-msgraph 0.8**: above. A link off Graph's origin is refused by every `from_link`: neverest follows the links Graph issued.
- **io-http 0.7**: no edit. neverest names `HttpAuthBearer`, `HttpAuthBasic` and `Http11SendError::Eof` alone.
- **io-oauth 0.4 to 0.6**: no edit, tests/common mints its tokens through the JWT bearer and client credentials grants, which kept their shape.

## Tests

New, each seen failing before its fix, in runs of `cargo test --bin neverest`:

- `offline::remote::imap_hydrate_tests::a_batch_failing_mid_answer_is_read_again_on_a_new_connection` (the fake server answers a body twice, on which io-imap stops reading): `Stream item 1 in INBOX error: stream stopped responding after 60s`.
- `offline::remote::imap_hydrate_tests::a_batch_losing_its_connection_is_read_again_on_a_new_one`: `Stream item 1 in INBOX error: IMAP server closed the connection`.
- `offline::remote::imap_hydrate_tests::a_refused_batch_is_read_one_by_one_on_its_connection` passed before and after: it holds the other half of the rule, one connection for a tagged `NO`.
- `imap::backend::tests::a_listing_row_that_does_not_decode_fails_the_page`: with the check of `rows_of` turned off by hand for one run, `a page without the message 8`.
- `offline::submit::tests::a_reset_cut_short_after_a_refused_data_command_handed_nothing_over`: `only the reset left`.
- `offline::submit::tests::a_server_closing_on_a_refusal_loses_the_channel`: `the session is closing`.
- `gpeople::client::tests::an_update_keeps_what_the_vcard_cannot_show`: the second organization missing from the `PATCH`.
- `dav::client::tests::an_answer_that_is_no_whole_multistatus_fails_the_listing` and `a_delete_answered_with_a_failed_member_is_not_done` pass on io-webdav 0.8 and were not run on 0.7, which this tree no longer links.

Changed:

- `offline::submit::tests::a_refusal_resets_the_transaction_for_the_next_message` counts one `RSET` (it saw two before neverest's own was removed).
- `offline::submit::tests::a_refusal_for_a_sending_limit_stops_the_channel` asserted the session ended on an `RSET` after a refused body, which was neverest's own: it now asserts the two envelope commands and nothing more.
- `offline::submit::tests::envelope_addresses_map_to_smtp_paths` also refuses an address holding a line break.
- Four literals of `MsgraphSendError::Api` and one of a delta page (src/msgraph/client.rs, src/msgraph/client/mail.rs), and `user_path(..).unwrap()` in tests/msgraph.rs.

src/imap/fake.rs gained the faults of a body fetch (`FakeFault`) and a count of its connections.

## Evidence

Each as `nix develop --command <cmd>`, on 2026-11-13:

- `cargo fmt --check`: clean.
- `cargo clippy --all-targets -- -D warnings`: clean with `--all-features`, with the default features, with `--no-default-features`, and with `--no-default-features --features` `search`, `rustls-ring,dav,vendored`, `rustls-ring,imap`, `rustls-ring,gmail` and `rustls-ring,msgraph`.
- `cargo test --all-features`, as the Tests workflow runs it: 549 passed, 70 ignored; 521 in the binary (512 before, nine added).
- `cargo tree -i io-http`, `-i io-sasl`, `-i io-oauth`: 0.7.0, 0.2.0 and 0.6.0, one of each.

No server suite and no live suite was run, no sync was started, and nothing connected to an account.

## Not done

- **A line that does not decode in a `SELECT`, `LIST` or `STATUS` answer** is warned of and the answer read. A `LIST` row lost is a mailbox left out of the collection listing, and a `VANISHED` or a `FETCH` lost in a QRESYNC `SELECT` is a change the checkpoint moves past. Failing there fails a whole account, or a mailbox for good, on a server sending one such line every time: the owner's to decide.
- **A verdict followed by a lost connection**: when the connection fails under io-smtp's `RSET`, the send fails with the I/O error and the refusal's reply is not known to neverest, so a refusal that was permanent is read as transient once, and the next run meets it again.
- **The Graph URLs neverest builds itself** (the body, size and summary batches, `message_gone`, an event action) still splice the id as it is; io-msgraph 0.8 has `path_segment` for them.
- **The stream watcher** of src/msgraph/client/wire.rs stands, where io-msgraph 0.8 now states `retry_after` and `keep_alive` on an error and the headers of an answer.
- **`Retry-After` of Gmail and Calendar**: io-gmail 0.7 and io-gcal 0.4 state it on an error; the throttle still reads the instant Google writes in its message.
- **percent-encoding** is in Cargo.toml and used nowhere since the wizard left.

## Capabilities moved

sync: "A body fetch answers for every handle it was asked about", "A queued submission is a `submit` queue intent", "A send that may have been delivered is never sent again" and "Google contacts and calendars are native sources" modified; "An IMAP listing is read whole or fails" and "A DAV answer that is no whole multistatus fails" added.
