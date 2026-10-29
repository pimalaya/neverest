---
cairn: change
id: a-gateway-error-is-waited-out
status: landed
created: 2026-10-29
---

# A gateway error is waited out like a 503

## Why

The live Google People test failed on 2026-10-29: a connections listing answered `502` "Error 502 (Server Error)!!1", and the run ended incomplete on the first answer, no retry, the listing being a request that creates nothing. The throttle sends a request again on 429 and, for one that creates nothing, 503; a 502 or a 504 is the same transient gateway failure, which Google, Microsoft and the DAV servers ask to be retried with back-off.

## What

- A request that creates nothing SHALL be sent again on 502 and 504 as on 503, on every source (Graph, Gmail, Google Calendar, Google People, CalDAV, CardDAV), Google reading them as a burst.
- A request that creates something stays sent again on 429 and a Google rate limit only, never on a 5xx, after which it may have landed.
- A 500 stays an error: it states a failure of the request, not of the way to the server.
