---
cairn: change
id: a-sent-copy-takes-the-providers-flags
status: landed
created: 2026-11-08
---

# A sent copy takes the provider's flags

## Why

The Android app staged its Sent copy with `\Seen`, a guess made when queueing. Landing it on the copy Gmail files itself kept that guess as a local edit, so the sync pushed `\Seen`, and Gmail, whose markers belong to the message in every label, marked a message sent to oneself read in the inbox too (android cairn/log/2026-11-06-sent-copy-takes-provider-markers.md). It also found Gmail restamps the `Date` and `Message-ID` of what its API sends, which kept the staged copy from landing and filed a second one (2026-11-07-gmail-sent-copy-redated.md).

neverest has the first shape for an IMAP source sending through an `smtp` table on a server that files its own Sent copy, Gmail over IMAP and SMTP being the known case. `files_sent_copy` is false for IMAP (src/offline/capability.rs), so after the send neverest replaces the intent with `PimdirAction::Add { flags: ["\\Seen"], .. }` into the `copy` collection (src/offline/driver.rs, the submit pass). When a listing names Gmail's copy, io-pimdir lands the staged create on it keeping the staged flags, and marks the item dirty whenever they differ from the arrival's (io-pimdir src/sync.rs, the landing's `PimdirStatus::Dirty`), so the `\Seen` is pushed. A second risk is a race: when the listing does not hold Gmail's copy yet, the push appends neverest's own, and the IMAP append searches nothing first (src/imap/backend.rs `add_message_stream`), so Sent may hold the message twice. Gmail API and Graph sources stage no copy and are not concerned.

Found by reading on 2026-11-08 (neverest d669915), reproduced live the same day (see What).
## What

The live run on the Google test account (Gmail IMAP and SMTP, XOAUTH2, 2026-11-08 on 8c62eaa) settled the facts:

- A message sent to oneself is one Gmail message labelled `SENT` and `INBOX`. With `copy` set to Sent it ended read; the same send without `copy` stayed `UNREAD`.
- Gmail's SMTP files its copy before the send returns: the same run's listing of Sent names it, io-pimdir lands the staged create on it (`land the pending create ... under 1943`), and the landing pushes the staged `\Seen`. No race, no second copy: Sent held the message once.
- Gmail's SMTP keeps the `Date` and `Message-ID` it is handed, unlike its API.

So neverest stages no copy for an IMAP source whose IMAP and SMTP hosts are both Gmail's (option C below), as it already does for the Gmail API and Graph, and the next round lists Gmail's own copy. Keyed on both hosts, not on the IMAP `X-GM-EXT-1` capability: the SMTP server files the copy, and a Gmail IMAP account relaying through another SMTP server, or another IMAP account relaying through Gmail's, would lose its Sent copy otherwise. The submit pass opens SMTP alone, so a capability would need the IMAP session too.

The `Message-ID` search before uploading a sent copy is dropped: the race it guards did not happen on Gmail, Gmail is now excluded, no other server is known to file SMTP submissions, and a search before every mail upload costs one command each.

## Decide with the owner

How the landing takes the server's flags:
- **A. neverest stages the copy's flags as unknown** (`PimdirFlags::Unknown`), if io-pimdir lands an unknown set by taking the arrival's. The upload path then needs its `\Seen` from elsewhere, since the copy neverest appends itself should still read as seen.
- **B. io-pimdir lands a create with the arrival's flags** when the create's flags were set by its owner rather than a frontend. That is a pimdir SYNC §6 rule (pimdir repo), so the spec moves there first.
- **C. neverest stages no copy** where the server is known to file one (Gmail's IMAP advertises `X-GM-EXT-1`, and its SMTP files what it sends), as it already does for the Gmail API and Graph. This also removes the race, but rests on recognising the provider; a configuration key could state it instead.

Chosen on the live evidence: C, keyed on both hosts. A alone would leave the copy neverest uploads elsewhere unread; B is a pimdir spec change with no second server to justify it.

## Compatibility

Not breaking. A Gmail account sending through its IMAP and SMTP channels stops marking messages sent to itself read, and Sent holds one copy.
