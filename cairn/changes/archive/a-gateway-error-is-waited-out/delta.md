---
cairn: change
change: a-gateway-error-is-waited-out
---

# Delta

## MODIFIED Requirements

### Requirement: A throttled provider is waited out, then named
A request Microsoft Graph, Gmail, Google Calendar, Google People, CalDAV or CardDAV answers with 429, or with 502, 503 or 504 for a request that creates nothing, or Google with a 403 rate limit, SHALL be sent again after the wait the provider states, else after a bounded exponential back-off with jitter. Google's refusals SHALL be read by their reasons and wording: a burst (a concurrency 429, a per-second rate, a 502, 503 or 504) SHALL wait one to two seconds; a per-minute quota, whose answer names the minute (`Units per minute per user`) whatever its reason, two, three, four then five seconds, then the next minute; a daily quota SHALL give the source up until the next midnight in America/Los_Angeles. Past the bound, or on a single wait longer than `sync --max-wait <SECONDS>` (120 by default), the source SHALL give up: no connection opened from it sends anything until the wait is over, what the run already wrote stays, the report lists the source under `throttled` with `until` (RFC 3339 UTC), and the run is incomplete (exit 3). Graph's body batches and Gmail's batch pauses SHALL wait no longer than that bound.

#### Scenario: A server answering 503
- **GIVEN** an account whose CalDAV source answers every request 503
- **WHEN** it is synced
- **THEN** the run exits 3 and `throttled` names `caldav` with an `until`

#### Scenario: A gateway error on a listing
- **GIVEN** a Google People connections listing answered `502` "Error 502 (Server Error)!!1"
- **WHEN** it is synced
- **THEN** the listing is sent again a second or two later, rather than failing the run

#### Scenario: Gmail's per-minute quota
- **GIVEN** a Gmail read answered 403 `Quota exceeded for quota metric 'Total Query Cost' and limit 'Units per minute per user'`, reason `rateLimitExceeded`
- **WHEN** it is synced
- **THEN** the read is sent again two seconds later, rather than failing the run

## ADDED Requirements

## REMOVED Requirements
