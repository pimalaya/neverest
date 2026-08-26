---
cairn: log
change: adopt-the-format-conventions
landed: 2026-08-27
---

# The link id and the date are the format's spelling now

pimdir SPEC Annex A.1 and `vectors/meta.json` give a message's identity as the
bare `Message-ID` and its `meta.date` as the UTC instant, the vector saying so in
as many words: "the Date carries a +02:00 offset, so the key and meta.date are
the UTC instant, never the local reading". This crate wrote `mid:<id>` and kept
the sender's offset; a card's `UID` was `uid:`-prefixed and its `hash:` fallback
hex where the format's is decimal. io-pimdir named the prefix as the divergence
when it landed `conventions` on 2026-08-25 and left the adoption unchecked,
because it costs a resync.

Adopted: the bare id on both kinds, the UTC date through one formatter both mail
tiers share (so the `alt:` id they each derive stays identical), and
`PimdirMailMeta` / `PimdirCardMeta` as the summary types, which deletes this
crate's two `MetaSummary`s and gives a card the `emails` the format's shape
carries. `link_hint` reads the id itself instead of stripping a prefix. Only the
fallbacks stay marked, that being the one case a prefix is for.

**The scanners did not move, and that is the finding.** Delegating `parse_body`
to `conventions::mail` was written, run against a real 8824-message store, and
reverted: io-pimdir reads headers raw, so every RFC 2047 subject came back as
`=?utf-8?q?D=C3=A9p=C3=B4t_de_votre_Lettre_recommand=C3=A9e?=` and the mailbox
listing was mojibake. Delegating `parse_body` to `conventions::card` failed two
tests this crate already had: a legal quoted parameter holding a colon
(`FN;LANGUAGE="x:y":Jane Doe`, RFC 6350 §3.3) came back cut in half, and RFC 6350
§3.4 escaping was left in, so `Doe\, Jane` reached the reader. The format's
vectors are ASCII-only and cover neither, deliberately, so nothing upstream
reports the difference and nothing upstream would have caught it here either.

Both scanners stay, each behind a test naming the gap
(`a_subject_is_decoded_not_shown_encoded`,
`a_parameter_may_hold_a_colon_of_its_own`,
`display_values_are_unescaped_but_the_body_is_not_touched`) and a module header
naming the deletion condition. When io-pimdir closes a gap its `derive` replaces
the scanner; adopting a worse reader to claim a shared one would have been the
wrong half of the trade.

**Cost**: every existing store re-links on the next sync. `neverest sync --reset
-a <account>`, which is the store owner's to run.

Verified: 94 tests green, fmt and clippy clean, every backend feature subset
compiles. Checked live on a real store: 8824 items, 8812 objects, 2.3 GiB, link
ids bare and dates in `Z`.

Spec updated: `sync` (ADDED "The conventions are the format's, the readers are
not"; MODIFIED "Bodies are content-addressed and deduped" and "The mail summary
is a versioned schema").
