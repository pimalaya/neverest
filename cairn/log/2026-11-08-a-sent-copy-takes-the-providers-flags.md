---
cairn: log
change: a-sent-copy-takes-the-providers-flags
date: 2026-11-08
---

# A sent copy takes the provider's flags

An IMAP source sending through Gmail's SMTP with `copy` set to Sent marked a message sent to oneself read in the inbox. Reproduced live on the Google test account on 8c62eaa: Gmail keeps one message labelled `SENT` and `INBOX`; with `copy` it ended without `UNREAD`, without `copy` it kept it. The trace showed the mechanism: Gmail's SMTP files its copy before the send returns, the same run's listing of `[Gmail]/Sent Mail` names it, io-pimdir lands neverest's staged `add` (`\Seen` set) on it, and the landing pushes `\Seen` onto the one message. No race and no second copy: Sent held the message once, and Gmail's SMTP kept the `Date` and `Message-ID` it was handed, unlike its API (android cairn/log/2026-11-07-gmail-sent-copy-redated.md).

## What landed

- `files_sent_copy` (src/offline/capability.rs) answers true for an IMAP source whose IMAP host is `imap.gmail.com` or `imap.googlemail.com` and whose SMTP host is `smtp.gmail.com` or `smtp.googlemail.com`, so the submit pass stages no copy there, as for the Gmail API and Graph. Both hosts, not the IMAP `X-GM-EXT-1` capability: the SMTP server is what files the copy, and the submit pass opens SMTP alone.
- tests/google.rs `a_message_sent_to_oneself_over_gmail_smtp_stays_unread`, live: failed on 8c62eaa (the inbox copy read), passes now (unread, Sent once, the store binding Gmail's copy). tests/common gains `Replica::with_config`, for a source the token line of `open` does not fit, and `Replica::send`.
- The proposal's `Message-ID` search before uploading a sent copy was dropped: the race never showed, and a search before every mail upload costs a command each.

## Left

- Options A (stage the copy's flags as unknown) and B (io-pimdir lands an owner's create with the arrival's flags) stay open for a second server known to file SMTP submissions.
- An account whose Gmail IMAP and Gmail SMTP sign in as different users would lose its Sent copy, Gmail filing it in the SMTP user's mailbox (reasoned, not tested).

Capability moved: sync ("A sent copy is filed once sent").
