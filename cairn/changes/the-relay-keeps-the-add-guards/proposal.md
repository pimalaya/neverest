---
cairn: change
id: the-relay-keeps-the-add-guards
status: landed
created: 2026-10-30
---

# The relay keeps the add guards

## Why

Pimdir stable-release review (2026-10-30), engine boundary. The relay (src/offline/driver.rs, `relay_targets`, `relay_one`, `bind_relayed`) streams a one-sided message to the other endpoint and binds the copy by hand, around the engine. None of pimdir SYNC §5's add guards applies to it: it appends while the target's round is still open, or before the target ever closed one, and for a message outside the target's scope. A target that already holds the message but has not listed it yet then gets a second copy.

## What

- A message is relayed to a target only once the target closed a round over the collection and has none open, and only when the message's date (undated mail included) is inside the scope that round covered: the store then knows the target's holdings in that scope, and a message it holds is bound there, not appended again.
- A message the guard holds back waits for a later pass or run, the relay never forcing it.

## Not done

- An idempotency key for the append (SYNC §5): the relay appends through the connector's add, which carries the link id hint and mint as a pushed add does; a stronger key needs the engine to own the relay.
