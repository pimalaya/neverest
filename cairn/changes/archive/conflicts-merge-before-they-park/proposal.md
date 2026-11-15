---
cairn: change
id: conflicts-merge-before-they-park
status: landed
created: 2026-08-28
---

# Every divergence is reported as if it were a disagreement

## Why

Neverest resolves no content conflict by itself, on purpose, and that is the right instinct applied one step too early. Most divergences are not disagreements. One side changed a phone number, the other changed a note, and there is nothing for anyone to decide: a three-way merge against the stored base takes both, because the base says which side touched which field. Reporting those to a human is not caution, it is a background tool asking to be turned off. On a first sync of two long-lived replicas it is dozens of prompts, none of which has a wrong answer.

What is left after that merge is the real thing: both sides changed the same field to different values. No amount of cleverness settles it. Neither format helps either, and it is worth writing down why, because it is the question everyone asks first. vCard carries `REV`, one optional timestamp for the whole card on the editing client's clock. iCalendar carries `LAST-MODIFIED` per component and `SEQUENCE`, a counter only organiser-significant changes bump. WebDAV adds an opaque `getetag` and a `getlastmodified` that is the server's clock at write time and therefore moves on our own pushes. None of it is per field, and across two spokes none of it shares a clock. The stored base gives causality instead, which is strictly stronger for deciding who changed what, and says nothing at all about who is right.

So the collision goes to a person, and everything about how it goes to them follows from neverest running unattended. It cannot open an editor: a run under cron has no terminal, and a run with one attached is as likely to be a wrapper script or an abandoned tmux pane as a person waiting. It cannot fail either: a conflict on one contact is not a failed sync, and exiting non-zero both stops the other ten thousand items and, under `Restart=on-failure`, produces a restart loop over something systemd cannot fix.

## What

The automatic half, inside every run:

- A three-way merge on the diverging bodies, dispatched on the collection's kind, resolving when it reports no collision and parking when it does. It is built in rather than configurable: it is a pure function over bodies already in the store, there is no taste in it, and the format vocabulary is closed, mail being immutable-content and never reaching this path.
- Because it cannot be swapped, it is strictly conservative. An empty report resolves. Anything else parks.

The reporting half:

- A distinct exit code for a run that synced and left conflicts behind, so a caller can tell without the run having failed.
- A notification on entry into conflict, opt-in, defaulting to a log. The engine already emits nothing for an already-parked placement, so this needs no deduplication of its own.

The deciding half, never from a run:

- `neverest conflict list`, `show` and `resolve`, the only place a decision is made.
- `resolve --prefer-local` and `--prefer-remote` discard a side, which is acceptable when a person asks for it by name and is exactly what a background process must never do.
- `resolve --interactive` hands the bodies to a configured program, base first, and takes back one body. Every interactive merger works this way, and a program that opens an editor, a form or nothing at all is none of neverest's business.
- A resolution is refused when the remote moved while it was being made, because an editor left open for an hour outlives the state it was computed against.
