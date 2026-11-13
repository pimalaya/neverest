---
cairn: tasks
change: neverest-follows-the-io-releases
---

# Tasks

- [x] Cargo.toml: io-gcal 0.4, io-gmail 0.7, io-gpeople 0.7, io-http 0.7, io-imap 0.9, io-msgraph 0.8, io-sasl 0.2 (its `saslprep` feature is gone), io-smtp 0.8, io-webdav 0.8, io-oauth 0.6; the io-http and io-msgraph pins removed, the pimalaya-cli pin kept
- [x] Cargo.lock moved for those crates, one io-http, one io-sasl, one io-oauth left
- [x] The CHANGELOG section of every release crossed, read against each use neverest makes of it (see the log)
- [x] io-imap: `ImapOutput` read at one seam (`data_of`, `rows_of` in src/imap/backend.rs)
- [x] IMAPB-S1: `ImapClient::reconnect`, `ImapClient::recover`, `Client::recover`, called by `hydrate_batch`; three tests over the fake server, two seen failing first
- [x] io-smtp: `SmtpMailbox::try_from`, `RcptTo { .. }`, no `RSET` of neverest's own, a `421` loses the channel, the hand-over counted on the data's own write; three tests seen failing first
- [x] io-gpeople: `GpeoplePersonVcard`, `merge_unprojected` in the update; one test seen failing first
- [x] io-gcal: `GcalApiError` in the throttle's reading; no id was encoded on neverest's side
- [x] io-msgraph: `MsgraphApiError`, `MsgraphPage::count`, `MsgraphField` recipients, `user_path` a `Result`
- [x] io-webdav: a test that a body that is no whole multistatus fails a report, a listing and a collection listing, and one that a delete answered 207 with a failed member fails
- [x] Spec, CHANGELOG.md, log
- [x] `cargo fmt`, clippy on every feature set of the log, `cargo test --all-features`
- [ ] An undecodable line in a `SELECT`, `LIST` or `STATUS` answer: the owner's to make a failure or to leave a warning
- [ ] The Graph URLs neverest builds itself through io-msgraph's `path_segment`
- [ ] io-msgraph 0.8's `retry_after` and `keep_alive` in place of the stream watcher
- [ ] The server suites and the live suites on the new libraries: the owner's, none was run
