---
cairn: log
change: json-keys-are-camel-case
landed: 2026-09-02
---

# `--json` keys are camelCase

One report printed two conventions: `SyncOutput` carried serde's snake_case (`dry_run`, `outstanding_conflicts`) while the item types it nests carried kebab-case (`message-id`, `in-reply-to`). Neither is dot-accessible in jq or JavaScript, which is what the README's own notifier recipe does with the payload, and the family standardised on camelCase to match the wire formats the endpoints speak.

The switch was held for 2.0 because renaming a published key is breaking. It landed now instead: 1.0.0 has not shipped, so nothing published carries the old spelling and no major is owed the break. That window closes with the release.

## What landed

`#[serde(rename_all = "camelCase")]` on the seven registered output types and everything they carry, and the kebab-case rename dropped from [ItemSummary](../../src/item/summary.rs), [Collection](../../src/item/collection.rs) and [Address](../../src/item/address.rs).

The two tagged enums needed `rename_all_fields` beside their existing `rename_all`, which reaches variant *names* only: [ItemHunk](../../src/sync/hunk.rs) has the multi-word fields of the whole report (`sourceSide`, `targetSide`, `sourceId`), and they were the ones a notifier reads.

What travels as a value kept its spelling, since only keys are dot-accessed: the hunk `kind`, the resolve `outcome` and the `IanaFlag` keywords stay kebab-case, and the vCard `fn` stays the name RFC 6350 gave it. Configuration types stay kebab-case, TOML keys being read rather than printed.

The proof is the schema registry rather than an assertion per field: `neverest json-schema -d` writes all seven documents, and no property name in them carries a hyphen or an underscore.

## Capabilities moved

- sync: every key of every `--json` payload is camelCase, and none is unreachable by dot access
