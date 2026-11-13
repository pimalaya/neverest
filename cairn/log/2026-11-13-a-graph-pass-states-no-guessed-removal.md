---
cairn: log
change: a-graph-pass-states-no-guessed-removal
date: 2026-11-13
---

# A Graph pass states no guessed removal

pimdir audit line Y1. The pass closing a scoped Graph round states gone only what its delta marks removed (pimdir SYNC §4). The way that would also have Graph state a removal made during the round was not taken: Microsoft does not document it for a mail folder.

## The documentation that decided

The other way out takes the delta link before the band pages and follows it at the round's end, which needs a link made without enumerating the folder: `$deltatoken=latest`. Read on 2026-11-13:

- "Use delta query to track changes in Microsoft Graph data" (learn.microsoft.com/en-us/graph/delta-query-overview): "Microsoft Entra ID resources such as users and groups support "sync from now" scenarios. […] Append `$deltatoken=latest` to the `delta` function and the response contains a `@odata.deltaLink` and no resource data. Resources in OneDrive and SharePoint also support this feature but require `token=latest` instead." Its table of supported resources lists `message` and `mailFolder` for delta query, and says nothing of `latest` for them.
- "message: delta" (learn.microsoft.com/en-us/graph/api/message-delta?view=graph-rest-1.0): its query parameters are `$deltatoken`, `$skiptoken` and `changeType`, the first "a state token returned in the `@odata.deltaLink` URL of the previous delta function call"; `latest` is not mentioned.
- "Get incremental changes to messages in a folder" (learn.microsoft.com/en-us/graph/delta-query-messages): no mention either.

So nothing documented says a mail folder's message delta takes `latest`, and the fallback the owner named landed.

## What landed

- src/msgraph/client/mail.rs, `pass`: `gone` is the ids whose last row of the fresh delta is marked `@removed` (`Seen::Removed`), where it was every handle the store binds or the band listed that the pass did not list. What the pass does not list is left to the engine, which finds it absent from the ids the pass hands as its unnarrowed enumeration (`enumerated`), in the round's scope and under the guards on a mass absence and an empty listing.
- The module's and the function's documentation say what the pass states, and the limit below.
- The test fake (`FakeGraph`) can have a fresh delta fail to list messages the folder holds (`unlisted`).

## The limit, now in the spec

A message a band page of a round listed, whose binding that page stamped, and that Graph dropped before the round's pass: the engine finds absent only a binding the round did not stamp, and the fresh delta reports no removal older than itself, so no row states it. Its row stays, bound and readable, until a later round over a scope holding its date closes without listing it: one an expired link (410) opens, or a full sync. A rerun, a delta and a wider scope (whose round lists the band it lacks alone) do not drop it.

## Tests

- `a_pass_failing_to_list_a_member_states_no_removal` (new): a folder synced since June, its link expired, a fresh delta failing to list `2026-06-03` and `2026-07-04`; the round under `--since 2026-07-01` closes with every member kept. Seen failing with the old statement put back by hand for one run: the stored set lacked both, the June one out of the round's scope and the July one a band page of the same round had listed.
- `a_message_dropped_before_the_pass_goes_with_a_later_round` (new): the limit and what ends it. A round interrupted after a band page listed `2026-09-10`, which Graph then drops: the round closes and it stays; the link expired, the next round drops it. On the old statement it failed at "stamped, never absent", the pass having stated it gone.
- `an_interrupted_round_resumes_its_band_and_passes_again` (changed): the one the withdrawn fix broke. It asserted the forbidden statement in two places and is changed there alone: its expected set no longer removes `2026-09-10` (`expected.remove("2026-09-10")` is gone, the assertion message reads "the pass adds to the band"), and its doc comment says the message stays and why. Every other assertion stands: the band resumed and not restarted, 29 band rows, one pass, `late` named by the pass, the round closed.
- The other fifteen tests of the module pass unchanged, `a_scoped_round_lists_its_head_and_its_pass_keeps_it` among them.

## Evidence

As the log of a-message-is-keyed-by-the-formats-reading states, the three changes verified together: 508 unit tests, every clippy set clean. No Graph tenant was reached.

## Not done

- **The stale row.** Three ways to end it, each the owner's:
  1. Pass first, with no undocumented token: run the ids-only pass as the round's first step (it enumerates the folder, as it does today at the end), keep its link in the round's cursor, list the band, and follow that link as the round's last page, so every removal of the round is one Graph states. It moves the cursor's format, the band head's last listing and the `enumerated` hand-over, and a resumed round has to follow the stored link rather than pass again.
  2. Try `$deltatoken=latest` on a mail folder against a test tenant, and write down what it answers: if Graph serves it, way 1 costs no enumeration.
  3. Have the standard let an unfiltered enumeration of the same round speak for a member an earlier page of it listed.
- **A member out of the round's scope that Graph dropped** is no longer dropped by a scoped round's pass either: the old statement applied whatever the date, the engine's inference stays in scope (pimdir SYNC §5). A link followed from before the removal still states it; a link made by a later pass does not.
- The whole-folder round and a followed delta already stated only `@removed` rows, and are unchanged.

## Capabilities moved

- sync (cairn/spec/sync.md): modified "A Graph mail folder keeps one delta link over the whole folder" (the pass's statement, the rule and the limit, two scenarios).
