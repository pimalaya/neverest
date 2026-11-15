---
cairn: change
id: the-report-rows-speak-the-family-nouns
status: landed
created: 2026-11-15
---

# The report rows speak the family nouns

## Why

The CLI alignment round of 2026-11-15 (pimalaya/.github cairn/changes/cli-alignment-2026-11-15, lines N5 and N6; audit findings F15 and F16, which hold lines A1, A3, A4 and B9 of the JSON shape review of 2026-11-14) found neverest's rows apart from the family's nouns:

- F15: a `check` capability row was `{name, support, detail}`, `support` a free string, where pimalaya-cli's capability row reads `level`, the shared `CapabilityLevel` enum. A queue row was keyed `id` (`applied` and `parked` of a drain, `parked`, `submitted`, `intents` of a sync) or `action` (an overruled body), where himalaya's `pimdir queue list` keys it `row`; a sync's parked row named its kind `action`, documented as the raw `set_flags`, and a rejected write spelled `set flags` with a space.
- F16: the account rows of `sync` and `drain` and the index refresh carried the whole error chain in `error` with no `sources`; no row used pimalaya-cli's `ItemError`; `sync` did not print through the streaming printer carillon's `watch` uses.

## What

- `CheckedCapability.level: CapabilityLevel` in place of `support: String`.
- Every queue row is keyed `row`; a parked row (drain and sync alike) reads `{row, collection, kind, producer, queuedAt, attempts, code, error, sources}`, `kind` the store's kebab-case spelling; a rejected flag push is `set-flags`.
- `AccountSync`, `DrainOutput` and `IndexRefresh` carry `sources`, `error` becoming the outermost message.
- `RejectedWrite` holds pimalaya-cli's `ItemError` flattened, its JSON unchanged.
- `sync` prints through `StdoutPrinter::streaming()`.

## Decisions

- **`init` keeps its whole chain in `error`.** `InitOutput.sources` already names the endpoints `init` opened; giving it the causes would need that key renamed, a break this round does not rule. Left for the owner.
- **The check rows stay the family's backend check row**, `{backend, ok, code, error}` with no `sources`, as himalaya, cardamum and carillon print it (L4 moves it into pimalaya-cli).
- **`ItemError` only where the row is one item's error**: a rejected write. The patch entries, the parked rows and the account rows name no item or an integer row, so they keep `Failure` (pimalaya-cli's item error less its id).
- **The text form is unchanged**: every row still prints its message and causes as one chain.
