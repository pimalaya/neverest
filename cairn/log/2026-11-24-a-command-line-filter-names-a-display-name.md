---
cairn: log
change: the-live-night-findings
landed: 2026-11-24
---

# A command-line filter names a display name

Task N15 of the-live-night-findings, under the owner's ruling of 2026-11-24: `-m`/`-x` on the command line also match a collection's display name, only when no id matches; the configured filter is unchanged. On the live night of 2026-11-23, iCloud and Google calendars, listed under opaque ids, could not be named with `-m`.

`resolve_names` (src/sync/driver/collections.rs) rewrites each command-line entry no listed id matches into the ids listed under that display name, ASCII case aside, before `filter_collections` applies the filter on ids as before. A one-source run resolves it over its listing (src/sync/driver/local.rs); a pair over both listings at once (src/sync/driver/pair.rs, `PairFilters`), so both sides keep the same ids whatever the target calls the collection. An entry matching neither keeps nothing, as before. Capability moved: sync.md, "A collection is keyed by its backend id and named by its display name".

## Checked

- tests/pair.rs `a_command_line_filter_names_a_collection_by_its_display_name` (source book `book` named `Friends`, target listing it `Book`): `-m friends` carries the card, `-x Friends` carries none, `-m book` still matches the id, a configured `include = ["Friends"]` keeps nothing. Before the fix it failed on `-m friends` (`left: [] right: ["a.vcf"]`).
- tests/pair.rs `a_lone_source_s_command_line_filter_names_a_collection_by_its_display_name`: before the fix `itemPatch` was empty (`left: 0 right: 1`).
