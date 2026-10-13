---
cairn: log
change: pimalaya-libs-from-pushed-revisions
date: 2026-10-13
---

# Pimalaya libraries at their latest pushed revisions

The dependency lines name released versions (io-pimdir 0.8, io-msgraph 0.6, io-gcal 0.2, io-gmail 0.5, io-gpeople 0.5, io-smtp 0.6, ical-rs 0.6), and `[patch.crates-io]` takes the unreleased fixes from pushed revisions, as the Android client does: ical-rs `caa1422`, io-http `e181530`, io-imap `b1ab38e`, io-msgraph `2ae2637`, io-pimdir `189db40`, io-webdav `62fad89`. io-gcal 0.2 replaces the git revision `1268ef6`, io-msgraph 0.6 the git revision `00ef069`.

## What landed

- src/gmail/client.rs, src/gpeople/client.rs: `{Gmail,Gpeople}SendError::Api` carries the whole Google error (`GmailApiError`, `GpeopleApiError`). The throttle still reads its status and message, the same text as before (`parse_api_error` is unchanged), and the throttle's own 429 is built with no reason. Classifying by Google's reasons rather than the text is left for later.
- src/kind/mail.rs: an envelope derivation states no `invitation`, read with the body alone. src/msgraph/client/mail.rs: the message delta asks no `$expand`.
- No code for the behaviour the libraries change: response size limits (head 64 KiB, body 256 MiB, an IMAP batched literal 100 MiB, each falling back to single fetches), a connection closed mid-body failing rather than hanging, Graph `EXDATE` no longer stashed, Google Calendar reading a series from its master, iCalendar lines folded and merges settling stamps.
- The batch decoding of src/msgraph/client.rs stays: io-msgraph's `MsgraphBatchResponse::bytes` reads standard base64 only and fails on a non-2xx status, and `retry_after` reads seconds only, where neverest also takes the URL-safe alphabets and an HTTP date.
- Tests run with the default features, with `--all-features` as CI does, and with `--no-default-features --features rustls-ring,imap,smtp,dav,msgraph,gpeople,gcal,gmail`, as MOA builds it; the Stalwart and Radicale suites run too, the Google and Graph live ones need secrets.

## Capabilities moved

None.
