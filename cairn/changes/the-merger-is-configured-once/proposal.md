---
cairn: change
id: the-merger-is-configured-once
status: active
created: 2026-09-07
---

# The merger is a person's tool, so it is configured once

## Why

`contact.merger` and `calendar.merger` name the programs a person settles a collision in. Those are properties of the **person**, not of the account: whoever merges cards in tcard merges them in tcard for every account they have, the way they have one `$EDITOR` and one `core.editor`. Neverest makes them write them per account, because `Config` is "nothing but named accounts" and has nowhere else to put it.

Two accounts is already two copies of the same line, and the line is long enough to get wrong: it carries four placeholders in a fixed order.

Nothing about the design wanted them per account. They are the only keys in `ContactConfig` and `CalendarConfig`, unset by default, no sync ever reads them, and `conflict resolve --interactive` is the single call site. They ended up under the account because that is the only table there is.

himalaya already carries the shape this needs: document-level keys whose doc comments read "Fallback for `AccountConfig::…`", so a person writes the thing once and an account overrides it when it genuinely differs.

## What

`Config` gains document-level `contact` and `calendar` tables, of the same `ContactConfig` and `CalendarConfig` types the account carries. The account's value wins where it is set, the document's answers where it is not, and neither is required.

Resolution happens where the account config is loaded, so `conflict resolve --interactive` keeps reading one `Option<CommandConfig>` a kind and the call site does not learn there are two places it could have come from.

```toml
contact.merger = "tcard merge {base} {local} {remote} --output {output}"
calendar.merger = "tcal merge {base} {local} {remote} --output {output}"

[accounts.work]
# inherits both

[accounts.calendars]
calendar.merger = "my-tcal merge {base} {local} {remote} --output {output}"
```

The merger is a **command**, and a command in a shared table is worth saying twice: it is run with the terminal's stdin and stdout, on paths under a temporary directory, only from `conflict resolve --interactive`, and never from a sync. Lifting it to the document changes none of that.

## Not in scope

**No other key moves.** `store`, `item`, `connections` and the backends stay per account, being properties of the account rather than of the person. This change is the merger and the table that holds it, and a later one may lift more once there is a second key that wants it.

**No merge policy.** Whether a run merges stays not a setting (`the-merge-is-not-a-build-option`); this is about where the interactive tool is named, not about what the built-in merge decides.

**No `$EDITOR` fallback.** A merger is not an editor: it takes four paths in a contract neither `$EDITOR` nor `$VISUAL` knows, so guessing one from the environment would run a text editor on the base file and take the result as a decision.
