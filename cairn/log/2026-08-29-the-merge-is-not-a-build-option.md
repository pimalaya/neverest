---
cairn: log
change: the-merge-is-not-a-build-option
landed: 2026-08-29
---

# The merge is not a build option

The sync capability requires without condition that a run three-way merges a marked conflict, and says in the same breath that the merge is built in rather than configured. A `merge` cargo feature gated exactly that, so a build made without it violated an unconditional SHALL, in the one dimension the requirement singles out. Building is configuring; doing it at compile time only makes the configuration harder to see.

## What landed

The feature is gone. `dep:ical-rs` and `dep:vcard-rs` moved to `dav`, which is the feature that decides whether a mutable-content kind exists at all. `Kind::merge` is no longer gated as a whole: its two mutable arms carry `#[cfg(feature = "dav")]`, exactly as the arms of `Kind::from_media_type` and `Kind::media_type` already did, and the mail arm answers in every build. The `cfg(not(feature = "merge"))` impl, whose `merge` refused every body and told the operator to rebuild, is deleted, because nothing can now be built that would need it.

## Why the feature could not earn its keep

It was declared `merge = ["dav", ...]`, so it turned another feature on. And every mutable-content kind was already `dav`-gated: `Kind::Vcard` and `Kind::Ical` do not exist without it, mail is immutable-content and reaches no merge, so the feature was co-extensive with `dav` across everything it could ever act on. A mail-only build excluded the merge by excluding `dav` and never needed a second switch.

That leaves one consumer it could serve: someone syncing contacts or calendars who wants every divergence to reach a person untouched. That is a policy, the spec refuses it deliberately, and a cargo feature is the worst place to express one. It is invisible at runtime, absent from the configuration, and enough to make two binaries of one version disagree about whether a card merges.

It was not free either. A second `impl Kind` had to be kept in step with the first, and it was already load-bearing: the settled-body validator reads a body with a hand-rolled scanner rather than vcard-rs or ical-rs precisely because those sat behind this feature, and a build without it was where `--interactive` became the only way anything was settled.

## Not changed

No behaviour moves for any build anyone would have made. `default` carried `merge`, and `--features merge` pulled `dav` in regardless. What changes is that the hole is gone.

## Capabilities moved

- **sync**: the merge requirement now binds every build, the merge riding on `dav` rather than a feature of its own.
