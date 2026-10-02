---
cairn: log
change: gmail-source
landed: 2026-10-02
---

# Gmail is a native mail source

`gmail` syncs `message/rfc822` over io-gmail 0.4, in src/gmail/client.rs, behind a new default `gmail` feature and a `Client::Gmail` arm.

Collections are the user labels plus `INBOX`, `SENT`, `DRAFT`, `SPAM`, `TRASH`, keyed and named by label name, the adapter mapping names to ids. The proposal keyed them by label id; that was reversed with the user before coding, the driver pairing endpoints and creating missing collections by key, so ids would never have met a Gmail account's IMAP endpoint.

A full round reads the profile `historyId` first, then id-only listings of the label and of the label with `UNREAD`, `STARRED`, `IMPORTANT`. A delta reads `history.list` scoped to the label and re-reads each touched message's labels (`format=minimal`), a 404 restarting a full round. Pushes are label modifies (`batchModify` beyond one id); only a delete from `TRASH` is permanent. Imports verify the answered id and, when it 404s, find the message again in the label's history by `Message-ID`. The `Meta` tier decodes headers with io-pimdir's own mail decoders, so it agrees with the `Full` derivation.

Live-tested against `google@pimalaya.org` (tests/google.rs, two replicas of one account over a throwaway label). Not done: the full-round measurement on a large mailbox, and a Gmail-and-IMAP two-source run.

Capabilities moved: **sync** (Gmail is a native mail source, added; sources are remote backends only, every remote backend is a cargo feature, a collection is keyed by its backend id, modified).
