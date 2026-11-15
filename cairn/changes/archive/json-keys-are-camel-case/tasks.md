---
cairn: tasks
change: json-keys-are-camel-case
---

Taken while 1.0.0 was still a release candidate, so nothing published carries the old spelling and no major is owed the break.

- [x] `#[serde(rename_all = "camelCase")]` on the seven registered types and everything they carry: `CheckOutput`, `SourceCheck`, `ConfigureOutput`, `InitOutput`, `SyncOutput` with its hunks and nested reports, and the three conflict outputs
- [x] `rename_all_fields` on the two tagged enums, whose variant fields `rename_all` does not reach: `ItemHunk` and `ConflictResolveOutput`
- [x] Drop the kebab-case rename from `ItemSummary`, `Collection` and `Address` under src/item/
- [x] Keep the variant spellings that travel as values: the hunk `kind`, the resolve `outcome` and the `IanaFlag` keywords
- [x] Keep the `fn` rename in src/kind/vcard.rs and any other passthrough spelling
- [x] Leave src/config.rs kebab-case: TOML keys are not `--json` keys
- [x] Update the README and config.sample.toml notifier recipes, `.outstanding_conflicts` becoming `.outstandingConflicts`
- [x] Regenerate the JSON Schemas and check no key kept a hyphen or an underscore
- [x] CHANGELOG under `Changed`, in the release that lands it
