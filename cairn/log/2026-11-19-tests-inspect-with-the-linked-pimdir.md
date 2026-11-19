---
cairn: log
change: tests-inspect-with-the-linked-pimdir
date: 2026-11-19
---

# The suites read the store through the linked io-pimdir

## What landed

- tests/common/mod.rs: `pimdir_cli` gone; `store_items` (`PimdirReader::list_summaries`), `retained_items` (`list_retained`), `pending_actions` (`list_pending_actions`), `link_ids` and `summary_text` read the store through the io-pimdir neverest links.
- tests/caldav.rs, tests/carddav.rs, tests/calendars.rs, tests/endpoints.rs, tests/submit.rs: no `pimdir` binary spawned; assertions on link ids, item counts, summaries and queue row ids instead of substrings of the CLI's JSON. The chronological check of caldav reads the order `list_summaries` answers.
- tests/pimdir.sh stays, for inspecting a store by hand; CONTRIBUTING.md says the suites never spawn it.

The five suites build; none was run: the flake provides no Radicale, tests/radicale.sh running a Docker image, and submit.rs needs Stalwart as well. The change stays active for that run.

No capability moved: tests only.
