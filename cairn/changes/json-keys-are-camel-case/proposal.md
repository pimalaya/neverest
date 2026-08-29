---
cairn: change
id: json-keys-are-camel-case
status: active
created: 2026-08-29
---

# `--json` keys become camelCase at 2.0

## Why

The Pimalaya family standardises the object keys of `--json` on camelCase.

camelCase is what the wire formats these tools wrap already speak: JMAP objects are camelCase by RFC 8620, and so are Microsoft Graph and every Google API. A JMAP mailbox crossing into a neverest output type crosses a case boundary today for no reason but serde's default.

The second reason is the consumer `--json` exists for, which for neverest is named in the README: a notifier piping `neverest sync --json` into jq. Neither jq nor JavaScript can use dot access on a key containing a hyphen, so a kebab-case key has to be written `."has-attachment"` in jq and `obj["has-attachment"]` in JavaScript, and the unquoted form fails quietly rather than loudly.

Neverest emits both conventions today. The six registered output types carry no rename and print serde's snake_case (`dry_run` and `outstanding_conflicts` from `SyncOutput`), while the item types they carry are kebab-case: src/item/summary.rs prints `message-id`, `in-reply-to` and `has-attachment`, and src/item/collection.rs, src/item/flag.rs and src/item/address.rs are the same. One report object holds both spellings.

Neverest is 1.0.0-rc, and the family rule is version-based: `--json` keys are a published contract, renaming one is breaking, and a breaking change waits for the next major, which here is 2.0.

## Open option, not a decision

Nothing stable has shipped from neverest yet. Switching before 1.0.0 goes out would cost nothing, would spare the project owing a 2.0 break almost immediately after its 1.0, and would let the README's jq recipes ship in their final spelling. The version-based rule was chosen deliberately, so this stays an option to raise while the release candidate is still open rather than a plan of record; once 1.0.0 ships, it expires and the switch is 2.0's.

## What changes

Output types only, meaning the six types registered in src/json_schema.rs and everything they carry:

- `CheckOutput` and `SourceCheck` in src/cli/check.rs
- `InitOutput` in src/cli/init.rs
- `SyncOutput` in src/sync/report.rs, with its hunks and its nested reports, where `dry_run` becomes `dryRun` and `outstanding_conflicts` becomes `outstandingConflicts`
- `ConflictListOutput`, `ConflictShowOutput` and `ConflictResolveOutput` in src/conflict/report.rs
- the item types these carry: `ItemSummary`, `Collection`, `Flag` and `Address` under src/item/, which lose their kebab-case rename

The README's notifier recipes move with the keys, `.outstanding_conflicts` becoming `.outstandingConflicts`.

`ConflictResolveOutput` needs one distinction kept straight. Its `#[serde(rename_all = "kebab-case", tag = "outcome")]` renames variant names, which travel as the value of `outcome` rather than as keys, and both are single words in any case. The rename to camelCase is about keys; a value spelling changes only if someone decides it separately.

Config types stay kebab-case. src/config.rs is deserialized from TOML, where hyphenated keys are the family convention (`store.purge-after` and the rest) and no jq expression ever reaches them. The rule governs what the printer emits, never what the loader reads.

Provider passthrough keeps its own spelling. A field carrying a wire name verbatim keeps its explicit `#[serde(rename = "...")]`: `@odata.nextLink` is what Microsoft Graph called it, `nextPageToken` is what Google called it, and `fn` in src/kind/vcard.rs is what RFC 6350 called it. None is derivable from a Rust field name, `rename_all` leaves an explicit `rename` alone, and nobody should later "fix" one.

## The alias trap

`#[serde(alias = "...")]` is a deserialization-only attribute. It teaches `Deserialize` to accept a second spelling and does nothing whatsoever to `Serialize`, so it cannot make an output type emit both `outstanding_conflicts` and `outstandingConflicts` through a transition. An output type only serializes, which makes an alias on one pure decoration. This looks like the obvious way to soften the break and is not; disproving it cost real time once.

The two real options are to twin the keys in the printer (emit both spellings, which grows every report and makes the published schema describe two names for one value) or to accept the break at the major. The decision is to accept it.

## Out of scope

No CHANGELOG entry: nothing changes for a user until the rename lands. The schema files `json-schema` writes change with the keys, so consumers pinned on them regenerate at the same time.
