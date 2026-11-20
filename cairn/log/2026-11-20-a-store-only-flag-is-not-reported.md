---
cairn: log
change: a-store-only-flag-is-not-reported
date: 2026-11-20
---

# A store-only flag is not reported

A flag `pimdir tag` kept on a card was reported on every run as `add [starred] to item …`, counted in `synchronized: 1 hunks`, though nothing pushed it: `push_rights` (src/sync/driver/local.rs) withholds the flag right for every kind but mail, while `forbidden_action` (src/sync/driver/itemize.rs) read `perms.flag.update` alone, so the itemized hunk was neither held nor pushed.

## What landed

- src/sync/driver/itemize.rs: `stays_local` drops a flag hunk on a side whose kind is not `message/rfc822` before `forbidden_action`, in `itemize` (pair) and `itemize_single` (one source), so it is neither reported nor counted as forbidden.
- CHANGELOG under Fixed.

## Spec

- [sync](../spec/sync.md): *A flag on a kind other than mail stays in the store* now says the run reports it neither as a hunk nor as a forbidden write, with its new scenario.

## Tests

src/sync/driver/tests.rs `a_store_only_flag_on_a_card_is_not_itemized`: a CardDAV side whose card carries a store-only flag itemizes nothing and forbids nothing. With `stays_local` answering false it fails, the patch holding the `AddFlags` hunk. Checked on a loopback CardDAV server: the run reads "already in sync", `itemPatch` and `forbidden` empty.

## Left open

`sync --reset` still counts such a card as an unpushed change (`pending_work`, src/cli/sync.rs): the flag is work no server holds, which the spec's reset refusal names, so whether a reset refuses over it is the owner's call.
