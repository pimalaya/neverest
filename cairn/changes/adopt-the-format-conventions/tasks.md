---
cairn: tasks
change: adopt-the-format-conventions
---

- [x] Mail links by the bare `Message-ID`, `alt:` unchanged
- [x] A card links by its bare `UID`, `hash:` unchanged
- [x] `meta.date` is the UTC instant, one formatter for both mail tiers
- [x] `PimdirMailMeta` / `PimdirCardMeta` replace this crate's `MetaSummary`s
- [x] `link_hint` reads the id rather than stripping a prefix
- [x] Test: the body tier matches the format's vector, id and date
- [x] Test: both tiers link one message the same way
- [x] Test: an encoded subject is decoded, which is why the scanner stays
- [x] Test: a quoted parameter and an escaped value survive, same reason
