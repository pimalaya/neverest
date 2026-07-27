---
cairn: log
change: mail-in-reply-to
landed: 2026-08-16
---

# Carry In-Reply-To in the mail summary

pimdir SPEC Annex A.1 gained `in_reply_to`, so a reader can pair a reply with its parent from a listing rather than by fetching and parsing bodies ([himalaya issue 734](https://github.com/pimalaya/himalaya/issues/734)). himalaya reads it; this is the writer that fills the store it reads.

## What landed

`ItemSummary.in_reply_to`, a list of bare msg-ids, written by both derivations: the `Meta` tier reads the 9th `ENVELOPE` element (RFC 3501 §7.4.2), which the enumerating FETCH already returns, and the `Full` tier reads the parsed header.

A list rather than a scalar, since RFC 5322 §3.6.4 spells the field `1*msg-id`, and each id normalised exactly as `message_id` is: that equality is the whole point, and it only holds if both ends strip their brackets the same way.

Microsoft Graph leaves it empty. `In-Reply-To` lives in `internetMessageHeaders`, which a listing selection does not return, and an absent optional already reads as unknown, so the alternative was one request per row for a field a reader can do without.

## Also in this pass

Citations follow pimdir's [section renumbering](https://github.com/pimalaya/pimdir/blob/master/cairn/log/2026-08-16-spec-restructure.md): the meta conventions moved to Annex A.

## Capabilities moved

- **sync**: the mail summary schema now lists `in_reply_to`, with its shape, its per-backend source and the Graph gap.
