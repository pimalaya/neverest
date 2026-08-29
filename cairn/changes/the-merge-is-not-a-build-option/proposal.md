---
cairn: change
id: the-merge-is-not-a-build-option
status: landed
created: 2026-08-29
---

# A cargo feature configured the one thing the spec says is not configurable

## Why

The sync capability requires, without condition, that "a run SHALL three-way merge the base, local and diverging bodies of a marked conflict", and says in the same breath that "the merge SHALL be built in rather than configured". A `merge` cargo feature gated exactly that, so a build made without it violated an unconditional SHALL, and violated it in the one dimension the requirement singles out. Building is configuring; doing it at compile time only makes the configuration harder to see.

The feature also could not earn its keep. It was declared `merge = ["dav", ...]`, so it turned another feature on, and every mutable-content kind is already `dav`-gated: `Kind::Vcard` and `Kind::Ical` do not exist without it, and mail is immutable-content and reaches no merge. The feature was therefore co-extensive with `dav` across everything it could ever act on. A mail-only build excluded the merge by excluding `dav` and never needed a second switch, which leaves one consumer the feature could serve: someone syncing contacts or calendars who wants every divergence to reach a person untouched. That is a policy, the spec refuses it deliberately, and a cargo feature is the worst place to express one: invisible at runtime, absent from the configuration, and enough to make two binaries of one version disagree about whether a card merges.

It cost more than nothing. A second `impl Kind` had to be kept in step with the first, and it was already load-bearing: the settled-body validator reads a body with a hand-rolled scanner rather than vcard-rs or ical-rs specifically because those sat behind this feature, and a build without it was where `--interactive` became the only way anything was settled.

## What

- The `merge` feature is removed. `dep:ical-rs` and `dep:vcard-rs` move to `dav`, which is the feature that decides whether a mutable-content kind exists at all.
- `Kind::merge` is no longer gated as a whole. Its two mutable arms carry `#[cfg(feature = "dav")]`, as the arms of `Kind::from_media_type` and `Kind::media_type` already do, and the mail arm answers in every build.
- The `#[cfg(not(feature = "merge"))]` impl, whose `merge` refused every body and told the operator to rebuild, is deleted. Nothing can now be built that would need it.

No behaviour moves for any build anyone would have made: `default` carried `merge`, and `--features merge` pulled `dav` in regardless. What changes is that the hole is gone.
