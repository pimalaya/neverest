---
cairn: tasks
change: a-sent-copy-takes-the-providers-flags
---

# Tasks

- [x] Reproduce live on the Google test account (google@pimalaya.org, token from `google_token(&["https://mail.google.com/"])` in tests/common, XOAUTH2 on IMAP and SMTP): a `submit` to itself with `copy` set to `[Gmail]/Sent Mail` left the message read; without `copy` it stayed `UNREAD`. Gmail filed its copy before the listing (no race, Sent held it once) and kept the `Date` and `Message-ID`
- [x] Write what the run showed into proposal.md, and settle A, B or C: C, keyed on both hosts; delta.md rewritten without the search
- [x] Regression test first, failing on 8c62eaa: `a_message_sent_to_oneself_over_gmail_smtp_stays_unread` in tests/google.rs (`#[ignore]`, cleanup by `rfc822msgid`), through the new `Replica::with_config` and `Replica::send`; a unit test of the host pairs in src/offline/capability.rs. The fake IMAP test of a landing is moot under C
- [x] `files_sent_copy` (src/offline/capability.rs) answers true for an IMAP source whose IMAP and SMTP hosts are both Gmail's; the `Message-ID` search before uploading a sent copy is dropped (see proposal)
- [x] If B: not chosen
- [x] CHANGELOG (Fixed)
- [x] Fold delta.md into cairn/spec/sync.md, log, land
