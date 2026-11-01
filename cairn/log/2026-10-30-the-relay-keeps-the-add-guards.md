---
cairn: log
change: the-relay-keeps-the-add-guards
date: 2026-10-30
---

# The relay keeps the add guards

## What landed

- src/offline/driver.rs: `relay_targets` reads the collection's coverage per source (`list_coverage`) and relays a message only into a target that closed a round over the collection, has none open, and whose covered scope holds the message's date (an undated message in every scope); one held back waits for a later pass, logged at debug.
- Tests: `a_relay_waits_for_the_target_to_close_a_round`, `a_relay_keeps_to_the_target_s_scope`, both failing on the old code (the message was relayed into a target that never listed the inbox, and into one whose scope left it out); the two existing relay tests give their target a closed round.

## Not done

- An idempotency key on the relayed append (SYNC §5): needs the engine to own the relay (the-engine-pins-the-agreed-body).

## Capabilities moved

- sync: a relay appends only where the target's listing is closed (added).
