---
cairn: log
change: a-refusal-carries-its-code
date: 2026-11-14
---

# A refusal carries its code

neverest follows Himalaya's "a result carries its errors" (2026-11-14) where its report already listed what each item cost: a server's refusal is coded from the vocabulary Himalaya shares, and every failing row carries pimalaya-cli's item error keys beside its own.

## What landed

- Dependencies (c1837c5, the commit before this one): pimalaya-stream 0.4, pimalaya-config 0.4.1, io-gmail 0.8, io-http 0.8, io-imap 0.10, io-msgraph 0.9, io-smtp 0.9, io-oauth 0.7, and pimalaya-cli 0be1b1f. io-webdav, io-gcal and io-gpeople were still on pimalaya-stream 0.3, so each got the same bump on its master (185ab63, dbb4481, e4a9cac, pushed 2026-11-14) and neverest pins those revisions until they release; one copy of every family crate is in the graph. `tls.cert` feeds `Tls::server`, mutual TLS staying unset; `Config` implements config 0.4's `sample_url` and `accounts`.
- src/error.rs: `permission-denied`, `quota-exceeded`, `rate-limited` and `recipient-refused`; `Failure` (`code`, `error`, `sources`), `Failure::of`, `error::code` (a coded error first, then a refusal) and the chain walk over one reader per protocol.
- The readers, each beside its client: src/dav/client.rs (a 507 or `quota-not-exceeded`, a 429, a 403 naming no precondition or `need-privileges`), src/imap/backend.rs (`NOPERM`, `OVERQUOTA` through `ImapClientError::status`), src/msgraph/client.rs (`ErrorQuotaExceeded` or 507, 429, 403), src/gmail/client.rs, src/gcal/client.rs and src/gpeople/client.rs through `throttle::google_code` (429 or a Google rate limit, else a 403), src/offline/submit.rs (a 5yz naming a sending limit, else a refused `RCPT TO`).
- src/sync/report.rs: the patch entries, `rejected` (`reason` renamed `error`), `submitted`, `intents`, `parked` and `unreached` carry `code`, `error` and `sources`; the text prints the chain as before. `RejectedPush` carries a `Failure`, the remote's `reject` taking one where it took a formatted string, so the typed error reaches the reader; the send channel's refused credentials (`Unopened::Refused`) and `SubmitFailure::failure` do the same, an unconfirmed send's `error` keeping its `unconfirmed: ` prefix. src/cli/drain.rs: `parked` the same.
- Account rows (src/cli/{sync,drain,init}.rs) keep `{code, error}` with the whole chain, coded by `error::code`; the index refresh gains `code`, the store's.
- src/imap/fake.rs: `refuse_next_write`, the next `APPEND` or `UID STORE` answered with a given completion.

## Evidence

Tests: a reader per protocol through its client's scripted server, src/dav/client.rs (403 bare, `need-privileges`, `valid-calendar-data`, 507, `quota-not-exceeded`, 409), src/imap/backend.rs (`NO [NOPERM]`, `NO [OVERQUOTA]`, `NO [CANNOT]` on a `UID STORE` and an `APPEND`), src/msgraph/client.rs (403 `ErrorAccessDenied`, 403 `ErrorQuotaExceeded`, 507, 400), src/gmail/client.rs (403 `insufficientPermissions`, 400), src/throttle.rs (`google_code` over seven answers); src/offline/driver.rs, a coded rejected row in JSON and in text; tests/intents.rs, a recipient refused `550 5.1.1` parking its send coded `recipient-refused` and exiting 3 (new, a `RefuseRcpt` step in the scripted SMTP server), and a daily limit coded `quota-exceeded`. Against the old code the new tests do not build (`Failure`, `refuse_next_write`) or read a `code` key its rows did not print. The three tests/intents.rs tests reading the whole chain from `error` now join `error` and `sources`.

`cargo fmt`, `cargo test --all-features` (every target, no failure), clippy with all targets over default, `--all-features`, `--no-default-features` and each backend alone (imap, smtp, dav, msgraph, gmail, gcal, gpeople over rustls-ring): no warning. `neverest json-schema neverest-sync` lists the new keys as required and the four codes. The server suites (Stalwart, Radicale, live) were not run.

## Capabilities moved

- sync (cairn/spec/sync.md): added "A server's refusal carries its code" and "A failing row carries its code and its causes"; modified "A refused write is reported, never counted as applied" and "An unreached endpoint is named in the report".

## Left

- io-webdav, io-gcal and io-gpeople release on pimalaya-stream 0.4, then neverest drops the three pins, as cardamum and calendula move.
- A credential refusal (SMTP 535, HTTP 401) has no code in the vocabulary.
- io-imap's partial FETCH and io-gmail's batched writes, which Himalaya pins unreleased, are not taken.
