---
cairn: log
change: a-gateway-error-is-waited-out
date: 2026-10-29
---

# A gateway error is waited out like a 503

The live Google People test failed on 2026-10-29 on a connections listing answered `502` "Error 502 (Server Error)!!1": the run ended incomplete on the first answer. A request that creates nothing is now sent again on 502 and 504 as on 503, on every source (`Request::retries`), Google reading them as a burst (`google_refusal`), and a create is still never sent again on a 5xx (`GoogleWaits::throttled`). A 500 stays an error.

The live harness (tests/common/mod.rs) now waits out a run a provider throttled past what neverest waits itself: a run ending incomplete (3) whose report names a `throttled` source is run again once its `until` has passed, three times at most and for at most 150 s each, `sync_until` moving its deadline as far; any other incomplete run fails as before. The Google contacts cleanup sends its own People calls again after a 429, so a run that spent the per-minute read quota no longer leaves its card behind. CI's attempt of the same day had failed on that quota ("Critical read requests (Contact and Profile Reads) per minute per user"), the throttle having given up at the minute's end as it should.

Capabilities moved: sync (modified A throttled provider is waited out, then named).

Verified: `cargo test` 425 green, fmt and clippy clean; the two new throttle tests fail on the code before; locally, the contacts test passed 6 of 8 runs on the harness change alone, one failing on that 502 and one before its output was kept, and the live Google suite passed 6/6 with both changes.
