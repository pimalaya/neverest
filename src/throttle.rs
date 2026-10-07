//! # Throttle
//!
//! What a source does when its provider says "not now": follow the wait the
//! provider states, back off otherwise (bounded exponential, with jitter),
//! and give up past a bound, the run keeping what it already wrote and
//! naming the source in its report (`throttled`). One [`Throttle`] is shared
//! by every connection a source opens, so once it gave up, the others stop
//! asking too until the wait it noted is over.
//!
//! It also paces a source whose provider meters requests by the second
//! (Gmail), holding it below the quota instead of waiting for refusals.

// NOTE: each HTTP backend uses its part; a build leaving some out leaves
// theirs unused.
#![cfg_attr(
    not(all(
        feature = "msgraph",
        feature = "gmail",
        feature = "gcal",
        feature = "gpeople",
        feature = "dav"
    )),
    allow(dead_code)
)]

use std::{
    collections::hash_map::RandomState,
    hash::BuildHasher,
    sync::Mutex,
    thread,
    time::{Duration, Instant, SystemTime},
};

use chrono::{DateTime, SecondsFormat, Utc};
use log::warn;

/// How many times a throttled request is sent again before the source gives
/// up on it.
const MAX_RETRIES: u32 = 5;

/// The first back-off wait, doubled at each retry when the provider states
/// none: 1, 2, 4, 8, then 16 seconds, about half a minute in all.
const FIRST_WAIT: Duration = Duration::from_secs(1);

/// The longest single wait. A provider asking for more (`Retry-After`)
/// makes the source give up at once, rather than hold the run that long.
const MAX_WAIT: Duration = Duration::from_secs(120);

/// A source's answer to throttling, shared by its connections.
#[derive(Debug)]
pub struct Throttle {
    /// The source, as the account names it.
    source: String,
    /// The token bucket pacing the source, when its provider meters it.
    pace: Option<Mutex<Bucket>>,
    /// Until when the source gave up, once it did.
    gave_up: Mutex<Option<SystemTime>>,
}

impl Throttle {
    /// A source that backs off when refused, unpaced.
    pub fn new(source: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            pace: None,
            gave_up: Mutex::new(None),
        }
    }

    /// A source also paced at `rate` units a second, a second's worth of
    /// them available at once.
    pub fn paced(source: impl Into<String>, rate: f64) -> Self {
        Self {
            pace: Some(Mutex::new(Bucket::new(rate))),
            ..Self::new(source)
        }
    }

    /// Until when the source gave up, if it did this run.
    pub fn gave_up(&self) -> Option<SystemTime> {
        *self.gave_up.lock().unwrap_or_else(|err| err.into_inner())
    }

    /// Records that the source gave up until `until`, keeping the latest.
    pub fn give_up(&self, until: SystemTime) {
        let mut gave_up = self.gave_up.lock().unwrap_or_else(|err| err.into_inner());
        if gave_up.is_none_or(|noted| noted < until) {
            *gave_up = Some(until);
        }
    }

    /// Until when a request is refused without being sent: the source gave
    /// up, and the wait it noted is not over.
    pub fn blocked(&self) -> Option<SystemTime> {
        self.gave_up().filter(|until| *until > SystemTime::now())
    }

    /// Takes `units` from the bucket, sleeping until they are there; nothing
    /// for an unpaced source.
    pub fn pace(&self, units: u32) {
        let Some(bucket) = &self.pace else {
            return;
        };
        let wait = bucket
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .take(f64::from(units), Instant::now());
        if !wait.is_zero() {
            thread::sleep(wait);
        }
    }

    /// Runs `attempt`, paced at `units`, sending it again while `throttled`
    /// reads its error as the provider throttling.
    ///
    /// `throttled` answers `None` for any other error, which is returned at
    /// once, or the wait the provider stated, if any. Past [`MAX_RETRIES`],
    /// or on a stated wait longer than [`MAX_WAIT`], the source gives up and
    /// the last error is returned. While it has given up, `blocked` builds
    /// the error returned without sending anything.
    pub fn call<T, E>(
        &self,
        units: u32,
        mut attempt: impl FnMut() -> Result<T, E>,
        throttled: impl Fn(&E) -> Option<Option<Duration>>,
        blocked: impl FnOnce(String) -> E,
    ) -> Result<T, E> {
        if let Some(until) = self.blocked() {
            return Err(blocked(format!(
                "{} gave up after being throttled, until {}",
                self.source,
                rfc3339(until)
            )));
        }

        let mut retry = 0;
        loop {
            self.pace(units);
            let err = match attempt() {
                Ok(out) => return Ok(out),
                Err(err) => err,
            };
            let Some(stated) = throttled(&err) else {
                return Err(err);
            };

            let wait = stated.unwrap_or_else(|| backoff(retry));
            if retry >= MAX_RETRIES || wait > MAX_WAIT {
                let until = SystemTime::now() + wait;
                warn!(
                    "{} throttled, giving up until {}",
                    self.source,
                    rfc3339(until)
                );
                self.give_up(until);
                return Err(err);
            }

            warn!(
                "{} throttled, retrying in {}ms",
                self.source,
                wait.as_millis()
            );
            thread::sleep(wait);
            retry += 1;
        }
    }
}

/// What a request does to the provider, which decides which refusals it is
/// sent again on.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Request {
    /// A read, a flag set, a move by id, a delete: sending it twice does what
    /// sending it once does, so a 503 is retried like a 429.
    Idempotent,
    /// A create, an upload, a send, an invitation: sent again only when the
    /// provider refused it unprocessed (429, a Google rate limit), never on a
    /// 503 or another 5xx, after which it may have landed already and a
    /// second one would land a second copy.
    Create,
}

impl Request {
    /// Whether an answer of `status` throttles this request: one it is sent
    /// again on.
    pub fn retries(self, status: u16) -> bool {
        match self {
            Self::Idempotent => matches!(status, 429 | 503),
            Self::Create => status == 429,
        }
    }
}

/// The wait before retry `retry` (from 0) when the provider states none:
/// [`FIRST_WAIT`] doubled each time, capped at [`MAX_WAIT`], drawn between
/// its half and its whole so connections throttled together spread out.
fn backoff(retry: u32) -> Duration {
    let full = FIRST_WAIT.saturating_mul(1 << retry.min(16)).min(MAX_WAIT);
    let jitter = RandomState::new().hash_one(retry) % 1_000;
    full / 2 + full / 2 * u32::try_from(jitter).unwrap_or(0) / 1_000
}

/// `at` as RFC 3339 UTC at seconds precision.
pub fn rfc3339(at: SystemTime) -> String {
    DateTime::<Utc>::from(at).to_rfc3339_opts(SecondsFormat::Secs, true)
}

/// Reads an HTTP `Retry-After` value (RFC 9110 §10.2.3): a number of
/// seconds, or an HTTP date, as the wait from now.
pub fn retry_after(value: &str) -> Option<Duration> {
    let value = value.trim();
    if let Ok(seconds) = value.parse::<u64>() {
        return Some(Duration::from_secs(seconds));
    }
    let at = DateTime::parse_from_rfc2822(value).ok()?;
    let at = SystemTime::from(at);
    Some(at.duration_since(SystemTime::now()).unwrap_or_default())
}

/// Whether a Google API error message is a rate limit, which Google answers
/// with 403 (`rateLimitExceeded`, `userRateLimitExceeded`) as well as 429,
/// and the wait it states, when it ends with `Retry after <instant>`.
///
/// io-gmail, io-gcal and io-gpeople keep the status and message of Google's
/// error envelope, not its `reason`, so the reason is read from the message
/// ("Rate Limit Exceeded", "User-rate limit exceeded"). A daily limit is
/// not a rate limit: waiting seconds does not lift it.
/// [`google_throttled`] for a request of `request`: a 503 throttles an
/// idempotent one only.
pub fn google_throttled_for(
    request: Request,
    status: u16,
    message: &str,
) -> Option<Option<Duration>> {
    match (request, status) {
        (Request::Create, 503) => None,
        _ => google_throttled(status, message),
    }
}

pub fn google_throttled(status: u16, message: &str) -> Option<Option<Duration>> {
    let stated = google_retry_after(message);
    match status {
        429 | 503 => Some(stated),
        403 => {
            let squeezed: String = message
                .chars()
                .filter(|c| c.is_ascii_alphabetic())
                .collect::<String>()
                .to_ascii_lowercase();
            let rate = squeezed.contains("ratelimitexceeded") && !squeezed.contains("daily");
            rate.then_some(stated)
        }
        _ => None,
    }
}

/// The wait a Google message states as `Retry after <RFC 3339 instant>`.
fn google_retry_after(message: &str) -> Option<Duration> {
    let lower = message.to_ascii_lowercase();
    let at = lower.find("retry after ")? + "retry after ".len();
    let instant = message[at..].split_whitespace().next()?;
    let at = DateTime::parse_from_rfc3339(instant.trim_end_matches('.')).ok()?;
    Some(
        SystemTime::from(at)
            .duration_since(SystemTime::now())
            .unwrap_or_default(),
    )
}

/// A token bucket: `rate` units a second, at most a second's worth held.
#[derive(Debug)]
struct Bucket {
    rate: f64,
    tokens: f64,
    at: Option<Instant>,
}

impl Bucket {
    fn new(rate: f64) -> Self {
        Self {
            rate,
            tokens: rate,
            at: None,
        }
    }

    /// Takes `units` at `now` and answers how long the taker waits for them.
    ///
    /// The units are taken at once, the balance going negative, so the
    /// bucket is reserved rather than polled: concurrent takers queue in
    /// the order they took, each sleeping off its own debt.
    fn take(&mut self, units: f64, now: Instant) -> Duration {
        if let Some(at) = self.at {
            let refill = now.saturating_duration_since(at).as_secs_f64() * self.rate;
            self.tokens = (self.tokens + refill).min(self.rate);
        }
        self.at = Some(now);
        self.tokens -= units;

        if self.tokens >= 0.0 {
            Duration::ZERO
        } else {
            Duration::from_secs_f64(-self.tokens / self.rate)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    #[test]
    fn a_bucket_lets_a_second_through_then_paces() {
        let start = Instant::now();
        let mut bucket = Bucket::new(200.0);
        for _ in 0..40 {
            assert_eq!(bucket.take(5.0, start), Duration::ZERO);
        }
        assert_eq!(bucket.take(5.0, start), Duration::from_millis(25));
        assert_eq!(bucket.take(5.0, start), Duration::from_millis(50));

        let later = start + Duration::from_secs(10);
        assert_eq!(
            bucket.take(5.0, later),
            Duration::ZERO,
            "refilled, never above a second's worth"
        );
        assert_eq!(bucket.tokens, 195.0);
    }

    #[test]
    fn a_backoff_doubles_within_its_jitter_and_its_cap() {
        for retry in 0..8 {
            let full = FIRST_WAIT.saturating_mul(1 << retry).min(MAX_WAIT);
            let wait = backoff(retry);
            assert!(wait >= full / 2 && wait <= full, "{retry}: {wait:?}");
        }
    }

    #[test]
    fn a_throttled_call_is_sent_again_until_it_lands() {
        let throttle = Throttle::new("gmail");
        let calls = Cell::new(0);
        let out = throttle.call(
            1,
            || {
                calls.set(calls.get() + 1);
                if calls.get() < 3 {
                    Err(429)
                } else {
                    Ok("done")
                }
            },
            |status: &u16| (*status == 429).then_some(Some(Duration::ZERO)),
            |_| 0,
        );
        assert_eq!(out, Ok("done"));
        assert_eq!(calls.get(), 3);
        assert_eq!(throttle.gave_up(), None);
    }

    #[test]
    fn a_long_stated_wait_gives_up_and_blocks_the_source() {
        let throttle = Throttle::new("msgraph");
        let calls = Cell::new(0);
        let throttled = |status: &u16| (*status == 429).then_some(Some(Duration::from_secs(600)));
        let out: Result<(), u16> = throttle.call(
            1,
            || {
                calls.set(calls.get() + 1);
                Err(429)
            },
            throttled,
            |_| 0,
        );
        assert_eq!(out, Err(429), "the provider's own error");
        assert_eq!(calls.get(), 1, "no wait of ten minutes");
        let until = throttle.gave_up().expect("gave up");
        assert!(until > SystemTime::now() + Duration::from_secs(590));

        let out: Result<(), u16> = throttle.call(
            1,
            || {
                calls.set(calls.get() + 1);
                Ok(())
            },
            throttled,
            |why| {
                assert!(why.starts_with("msgraph gave up after being throttled, until "));
                999
            },
        );
        assert_eq!(out, Err(999));
        assert_eq!(calls.get(), 1, "nothing sent while blocked");
    }

    #[test]
    fn another_error_is_returned_at_once() {
        let throttle = Throttle::new("caldav");
        let calls = Cell::new(0);
        let out: Result<(), u16> = throttle.call(
            1,
            || {
                calls.set(calls.get() + 1);
                Err(404)
            },
            |status: &u16| (*status == 429).then_some(None),
            |_| 0,
        );
        assert_eq!(out, Err(404));
        assert_eq!(calls.get(), 1);
        assert_eq!(throttle.gave_up(), None);
    }

    #[test]
    fn google_rate_limits_read_from_status_and_message() {
        assert_eq!(
            google_throttled(429, "Too many concurrent requests for user"),
            Some(None)
        );
        assert_eq!(google_throttled(503, "Backend Error"), Some(None));
        assert_eq!(google_throttled(403, "Rate Limit Exceeded"), Some(None));
        assert_eq!(
            google_throttled(403, "User Rate Limit Exceeded"),
            Some(None)
        );
        assert!(
            google_throttled(
                403,
                "User-rate limit exceeded.  Retry after 2099-01-01T00:00:00.000Z"
            )
            .flatten()
            .is_some_and(|wait| wait > Duration::from_secs(3600))
        );
        assert_eq!(google_throttled(403, "Daily Limit Exceeded"), None);
        assert_eq!(google_throttled(403, "Insufficient Permission"), None);
        assert_eq!(google_throttled(404, "Not Found"), None);
    }

    #[test]
    fn a_retry_after_reads_seconds_or_a_date() {
        assert_eq!(retry_after(" 7 "), Some(Duration::from_secs(7)));
        assert_eq!(
            retry_after("Wed, 21 Oct 2015 07:28:00 GMT"),
            Some(Duration::ZERO),
            "a date past is no wait"
        );
        assert_eq!(retry_after("soon"), None);
    }

    /// A create is sent again only on a refusal answered before anything
    /// was done: a 503 may follow a write that landed.
    #[test]
    fn a_create_is_retried_on_a_429_alone() {
        assert!(Request::Idempotent.retries(429));
        assert!(Request::Idempotent.retries(503));
        assert!(!Request::Idempotent.retries(500));
        assert!(Request::Create.retries(429));
        assert!(!Request::Create.retries(503));
        assert!(!Request::Create.retries(502));

        assert_eq!(
            google_throttled_for(Request::Idempotent, 503, "Backend Error"),
            Some(None)
        );
        assert_eq!(
            google_throttled_for(Request::Create, 503, "Backend Error"),
            None
        );
        assert_eq!(
            google_throttled_for(Request::Create, 403, "Rate Limit Exceeded"),
            Some(None),
            "a rate limit refuses the request before doing it"
        );
        assert_eq!(
            google_throttled_for(Request::Create, 429, "Too many requests"),
            Some(None)
        );
    }
}
