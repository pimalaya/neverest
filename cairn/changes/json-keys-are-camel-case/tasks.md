---
cairn: tasks
change: json-keys-are-camel-case
---

Held until 2.0, unless the open option is taken while 1.0.0 is still a release candidate.

- [ ] `#[serde(rename_all = "camelCase")]` on the six registered types and everything they carry: `CheckOutput`, `SourceCheck`, `InitOutput`, `SyncOutput` with its hunks and nested reports, and the three conflict outputs
- [ ] Drop the kebab-case rename from `ItemSummary`, `Collection`, `Flag` and `Address` under src/item/
- [ ] Keep the `fn` rename in src/kind/vcard.rs and any other passthrough spelling
- [ ] Leave src/config.rs kebab-case: TOML keys are not `--json` keys
- [ ] Update the README notifier recipes, `.outstanding_conflicts` becoming `.outstandingConflicts`
- [ ] Regenerate the JSON Schemas and check no key kept a hyphen or an underscore
- [ ] CHANGELOG under `Changed`, as breaking, in the release that lands it
