---
cairn: change
id: adopt-the-format-conventions
status: landed
created: 2026-08-27
---

# The link id and the date were this crate's spelling, not the format's

## Why

pimdir SPEC Annex A.1 and the format's own `vectors/meta.json` give a message's identity as the **bare** `Message-ID` (`basic-1@example.org`) and its `meta.date` as the **UTC instant** (`2026-08-01T10:00:00Z`, the vector saying so in as many words: "the Date carries a +02:00 offset, so the key and meta.date are the UTC instant, never the local reading"). This crate wrote `mid:basic-1@example.org` and kept the sender's offset. A card's `UID` was `uid:`-prefixed for the same reason, and its `hash:` fallback was hex where the format's is decimal.

io-pimdir landed `conventions` on 2026-08-25 as the one implementation of Annex A and named this crate's prefix as the divergence, leaving the downstream adoption unchecked because it costs a resync. Two writers disagreeing about one item's id link it twice and store one body twice, which is the failure the module exists to end; until this lands, anything else writing into a neverest store has to guess which spelling it will meet, and Himalaya's pimdir backend was translating at its seam to cope.

## What

- The link id is the bare `Message-ID` / `UID`. Only the fallbacks stay marked (`alt:`, `hash:`), which is the one case a prefix is for: a name no server has heard of. Nothing is lost by dropping the prefix on the primary, RFC 5322 `atext` admitting no colon before the `@`.
- `meta.date` is the UTC instant, through one formatter both mail tiers share, so the `alt:` id they each derive stays identical.
- The summary type is `io_pimdir::conventions::{mail::PimdirMailMeta, card::PimdirCardMeta}`, so the schema cannot drift from the format's by a field or a spelling. This crate's own `MetaSummary`s are gone, and a card gains the `emails` the format's shape carries.
- `link_hint` reads the id itself rather than stripping a prefix.

## What is not adopted, and why

The **conventions** are the format's; the **scanners** are not, on either kind, because io-pimdir's lose data this crate's do not:

- `conventions::mail` reads headers raw, so an RFC 2047 encoded subject stays `=?utf-8?q?D=C3=A9p=C3=B4t?=` and a list of a real mailbox is mojibake. Measured on a real store: adopting it turned every non-ASCII subject into encoded-words.
- `conventions::card` splits a property on the first colon, so a legal quoted parameter holding one (`FN;LANGUAGE="x:y":Jane Doe`, RFC 6350 §3.3) cuts the value in half, and it leaves RFC 6350 §3.4 escaping in place, so a reader displays `Doe\, Jane`.

The format's vectors are ASCII-only and cover neither, by choice, so nothing upstream says otherwise and nothing upstream catches it. Delegating today would trade a correct reader for a nominal deduplication. Three tests here hold the line and name the condition for deleting the scanners.

## Cost

Every existing store re-links on the next sync: `neverest sync --reset -a <account>`, which is the owner's to run.
