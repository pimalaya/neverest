---
cairn: tasks
change: dry-run-names-the-bodies
---

# Tasks

- [x] Read the side rather than the hub projection in `itemize_fetches`, and key
      on the stored object rather than on the detail level.
- [x] Thread the run's dryness into `upgrade_probed` and skip a `Full` upgrade.
- [x] Unit-test both reads against a probed placement and a dropped object.
- [x] Cover both in the live CardDAV suite: a first dry run names the cards, and
      a dry run after a server-side edit names the re-fetch.
- [x] CHANGELOG.md.
- [x] Fold the delta into cairn/spec/sync.md and log it.
