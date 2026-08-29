---
cairn: log
change: conflicts-merge-before-they-park
landed: 2026-08-28
---

# Every divergence was reported as if it were a disagreement

Neverest resolved no content conflict by itself, on purpose, and that was the right instinct applied one step too early. Most divergences are not disagreements: one side changed a phone number, the other a note, and the stored base says which side touched which field, so a three-way merge takes both and nobody has to be asked. Reporting those was a background tool asking to be turned off, and on a first sync of two long-lived replicas it was dozens of prompts with no wrong answer.

What is left after that merge is the real thing, both sides setting one field two ways, and it now has a place to go that is not the run.

## What landed

**The automatic half, inside every run.** A three-way merge of the base, local and diverging bodies of each marked conflict, dispatched on the collection's kind: vcard-rs for contacts, ical-rs for calendars, tasks and journals, and nothing for mail, which is immutable-content and reaches none of this. It lives in the driver rather than in io-replica, so the engine keeps knowing nothing about formats, and it is built in rather than configured: it is a pure function over bodies the store already holds, there is no taste in it, and the format vocabulary is closed. Because nobody can swap it out it is strictly conservative, resolving on an empty report and on nothing else. The remote side comes from the store, the engine's upgrade pass having fetched it onto the conflict object, so a conflict whose body has not landed yet is left exactly as it is rather than merged against a body nobody holds. A merged body is staged as an ordinary `update` through the pimdir queue and drained in the same breath, which is the path whoever owns an edit already resolves a conflict by.

**The reporting half.** Exit code 2 for a run that reconciled its collections and left conflicts behind, beside 0 for clean and 1 for failed. A conflict is one item wide and halts nothing: failing the run would stop the other ten thousand items over one duplicated phone number, and under a supervisor restarting on failure it would loop over a state no supervisor can fix. The count that answers the exit code is read from the store rather than from the run's own tally, because the engine emits nothing for a placement it already parked, so the two numbers are two different questions. That same early return is the notification semantics wanted, so `conflict.notify` needs no deduplication of its own: it fires on entry into conflict and never again, and unset, which is the default, it leaves a warning in the log rather than shelling out unasked.

**The deciding half, never from a run.** `neverest conflict list`, `show` and `resolve`, with `conflicts` as a hidden plural alias. A conflict is addressed by the item's public id, the store-global `seq` every other neverest command already shows, narrowed by `--source` for an item that diverged on more than one. `list` names every divergence the account's store holds, marking the ones whose diverging body no run has fetched yet as not resolvable; `show` prints the three bodies a decision is made from. `resolve --prefer-local` and `--prefer-remote` discard a side, which is acceptable because a person asked for it by name and is exactly what a background process must never do. `resolve --interactive` hands the bodies to the program `conflict.merger` names, as filesystem paths, base first, then the divergent sides, then the path to write.

The merger contract follows git mergetool. Paths are appended positionally, which is tcal's own argument order and makes `conflict.merger = "tcal merge"` the whole configuration; a command carrying any of `{base}`, `{local}`, `{remote}` and `{output}` is substituted instead, for a tool with an argument shape of its own, which is how tcard's `--output` is reached. The result is taken only on a zero exit with the output written, and written means its bytes differ from the ones neverest seeded it with, which no clock skew and no timestamp granularity can get wrong. An editor exits zero on a bare quit, so a zero exit alone is not a decision, and reading it as one would discard a side by accident.

**The staleness guard**, which only becomes load-bearing once a decision can outlive the state it was computed against. `conflict resolve` records the revision the divergence was recorded at, and re-reads the store under the sync lock before staging anything. A revision that moved is reported as moved and nothing is pushed: an unresolved conflict tracks the newest remote revision on every run, so a decision left in an editor for an hour can be a decision about a version nobody holds any more, and pushing it would overwrite everything that arrived meanwhile, which is the loss the whole design exists to prevent arriving at the last step instead of the first. Under `--interactive` the fresh bodies are exported again and the merger asked once more, up to three times; under the two prefer flags nothing was exported, so there is nothing to re-export and the decision is refused outright.

The store lock is deliberately not held across the merger. A person may sit in an editor for an hour and a sync must not be blocked behind them, and what that costs is exactly the question the guard answers.

## Not changed

The engine's conflict policy stays `Manual`, and `KeepBoth` stays unmapped: forking one card into two is a worse outcome than parking it. No timestamp entered anywhere. vCard's `REV`, iCalendar's `LAST-MODIFIED` and `SEQUENCE` and WebDAV's `getlastmodified` are per-object at best, on clocks two spokes do not share, and the stored base gives causality instead, which is strictly stronger for deciding who changed what and says nothing at all about who is right. No one-step `sync --resolve` exists: a tty is not consent, since a run has one when a wrapper script drives it, when a pane nobody is sitting at watches it and when a person is waiting, and the three are indistinguishable from inside.

## Capabilities moved

- **sync**: a run merges what nobody disagreed about before parking anything, a run that parked conflicts exits with a code of its own and reports the outstanding count from the store, entering a conflict notifies once, deciding is a command and never a run, and a decision whose revision moved under it is refused.
