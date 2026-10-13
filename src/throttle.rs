//! # Throttle
//!
//! What a source does when its provider says "not now": follow the wait the
//! provider states, back off otherwise (bounded exponential, with jitter),
//! and give up past a bound, the run keeping what it already wrote and
//! naming the source in its report (`throttled`). One [`Throttle`] is shared
//! by every connection a source opens, so once it gave up, the others stop
//! asking too until the wait it noted is over.
//!
//! It also paces a source whose provider meters requests (Gmail), by the
//! second and over a sliding minute, holding it below the quota instead of
//! waiting for refusals.
//!
//! Google's refusals are read by their reason ([`google_refusal`]): a burst
//! (a concurrency 429, a per-second rate) is gone within a second or two, a
//! per-minute quota within a few seconds more, a daily quota not before
//! Google's midnight, where the source gives up.

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
    cell::Cell,
    collections::{VecDeque, hash_map::RandomState},
    hash::BuildHasher,
    sync::Mutex,
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use chrono::{DateTime, Datelike, Days, NaiveDate, SecondsFormat, TimeZone, Utc, Weekday};
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
    /// The units the source may spend over any sixty seconds, when its
    /// provider meters it by the minute too.
    minute: Option<(u32, Mutex<MinuteBudget>)>,
    /// Until when the source gave up, once it did.
    gave_up: Mutex<Option<SystemTime>>,
}

impl Throttle {
    /// A source that backs off when refused, unpaced.
    pub fn new(source: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            pace: None,
            minute: None,
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

    /// A source paced at `rate` units a second and at most `per_minute`
    /// units over any sixty seconds.
    pub fn paced_per_minute(source: impl Into<String>, rate: f64, per_minute: u32) -> Self {
        Self {
            minute: Some((per_minute.max(1), Mutex::new(MinuteBudget::new()))),
            ..Self::paced(source, rate)
        }
    }

    /// Whether a wait is short enough to be slept in the run rather than
    /// make the source give up.
    pub fn waits_in_process(&self, wait: Duration) -> bool {
        wait <= MAX_WAIT
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

    /// Takes `units` from the bucket, then from the minute's budget,
    /// sleeping until they are there; nothing for an unpaced source.
    pub fn pace(&self, units: u32) {
        let Some(bucket) = &self.pace else {
            return;
        };
        let now = Instant::now();
        let mut wait = bucket
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .take(f64::from(units), now);
        if let Some((limit, minute)) = &self.minute {
            wait += minute
                .lock()
                .unwrap_or_else(|err| err.into_inner())
                .reserve(now + wait, units, *limit);
        }
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

/// What a Google refusal asks of the source, read by its reason.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GoogleRefusal {
    /// A per-user concurrency limit (`Too many concurrent requests for
    /// user`), a per-second rate, a 503: gone within a second or two.
    Burst,
    /// A per-minute quota (`Quota exceeded for quota metric 'Total Query
    /// Cost' and limit 'Units per minute per user'`): a bucket refilling by
    /// the second, waited a few seconds more each time, then to the next
    /// minute.
    Minute,
    /// A daily quota: nothing before Google's midnight lifts it, so the
    /// source gives up until then.
    Daily,
}

/// A Google error, as the io-gmail, io-gpeople and io-gcal envelopes keep
/// it: io-gcal keeps the status and message alone, so the reason is also
/// read from the text.
#[derive(Clone, Copy, Debug, Default)]
pub struct GoogleAnswer<'a> {
    /// The effective HTTP status.
    pub status: u16,
    /// The message of the envelope, or the raw body.
    pub message: &'a str,
    /// The reasons of `error.errors[]` (`rateLimitExceeded`).
    pub reasons: &'a [String],
    /// The reasons of `error.details[]` (`RATE_LIMIT_EXCEEDED`).
    pub detail_reasons: &'a [String],
    /// The canonical status (`RESOURCE_EXHAUSTED`).
    pub google_status: Option<&'a str>,
}

impl GoogleAnswer<'_> {
    fn has_reason(&self, reason: &str) -> bool {
        self.reasons
            .iter()
            .chain(self.detail_reasons)
            .any(|r| r == reason)
    }

    /// The message and the reasons, the text a quota is named in.
    fn text(&self) -> String {
        let mut text = String::from(self.message);
        for reason in self.reasons.iter().chain(self.detail_reasons) {
            text.push(' ');
            text.push_str(reason);
        }
        text
    }
}

/// How a Google answer throttles: `None` for any other answer, which is no
/// reason to wait.
///
/// A 429, a 503, or a 403 or 429 naming a rate limit by reason
/// (`rateLimitExceeded`, `userRateLimitExceeded`, `quotaExceeded`,
/// `RATE_LIMIT_EXCEEDED`) or by message, is a [`GoogleRefusal::Burst`],
/// unless it names a per-minute quota ([`GoogleRefusal::Minute`]) or a
/// daily one ([`GoogleRefusal::Daily`]). Gmail's per-minute refusal carries
/// the reason `rateLimitExceeded`, like the concurrency one: only its
/// wording tells them apart.
pub fn google_refusal(answer: &GoogleAnswer<'_>) -> Option<GoogleRefusal> {
    let text = answer.text();
    let squeezed = squeeze(&text);
    let rate = answer.status == 429
        || answer.google_status == Some("RESOURCE_EXHAUSTED")
        || [
            "rateLimitExceeded",
            "userRateLimitExceeded",
            "quotaExceeded",
            "RATE_LIMIT_EXCEEDED",
        ]
        .iter()
        .any(|reason| answer.has_reason(reason))
        || squeezed.contains("ratelimitexceeded")
        || squeezed.contains("quotaexceeded");

    match answer.status {
        429 | 403
            if answer.has_reason("dailyLimitExceeded")
                || (names_daily_quota(&text) && !squeezed.contains("unauthenticated")) =>
        {
            Some(GoogleRefusal::Daily)
        }
        429 | 403 if names_minute_quota(&text) => Some(GoogleRefusal::Minute),
        429 | 503 => Some(GoogleRefusal::Burst),
        403 if rate => Some(GoogleRefusal::Burst),
        _ => None,
    }
}

/// Whether a text names one of Google's per-minute quotas, as Gmail's
/// `Quota exceeded for quota metric 'Total Query Cost' and limit 'Units per
/// minute per user'` does, in its message or in its `ErrorInfo` metadata
/// (`…PerMinutePerUser`); the older APIs' hundred seconds count alike.
pub fn names_minute_quota(text: &str) -> bool {
    let squeezed = squeeze(text);
    squeezed.contains("perminute") || squeezed.contains("per100seconds")
}

/// Whether a text names a daily quota, which no wait within a run lifts.
pub fn names_daily_quota(text: &str) -> bool {
    let squeezed = squeeze(text);
    squeezed.contains("dailylimit") || squeezed.contains("perday")
}

/// The ASCII letters and digits of a text, lowercased.
fn squeeze(text: &str) -> String {
    text.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// The waits of one Google request, a few seconds more at each per-minute
/// refusal it meets.
#[derive(Debug, Default)]
pub struct GoogleWaits {
    minutes: Cell<u32>,
}

impl GoogleWaits {
    pub fn new() -> Self {
        Self::default()
    }

    /// Forgets the per-minute refusals met so far: the next one is the
    /// first in a row again.
    pub fn reset(&self) {
        self.minutes.set(0);
    }

    /// Whether an answer of `request` throttles it, and the wait before it
    /// is sent again, as [`Throttle::call`] takes it: a 503 throttles an
    /// idempotent request only, a create maybe having landed.
    pub fn throttled(
        &self,
        request: Request,
        answer: &GoogleAnswer<'_>,
    ) -> Option<Option<Duration>> {
        if request == Request::Create && answer.status == 503 {
            return None;
        }
        let refusal = google_refusal(answer)?;
        let stated = google_retry_after(answer.message);
        Some(Some(self.wait(refusal, stated, SystemTime::now())))
    }

    /// The wait after `refusal` at `now`: the one the answer stated, else a
    /// second or two after a burst, two to five seconds after a per-minute
    /// refusal and the next minute after the fifth, and Google's next
    /// midnight after a daily one.
    pub fn wait(
        &self,
        refusal: GoogleRefusal,
        stated: Option<Duration>,
        now: SystemTime,
    ) -> Duration {
        match refusal {
            GoogleRefusal::Burst => stated.unwrap_or_else(burst_pause),
            GoogleRefusal::Minute => {
                let count = self.minutes.get() + 1;
                self.minutes.set(count);
                stated.unwrap_or_else(|| minute_pause(count, now))
            }
            GoogleRefusal::Daily => until_pacific_midnight(now),
        }
    }
}

/// How many per-minute refusals in a row are waited a few seconds each
/// before the next one waits for the next minute.
pub const MINUTE_PAUSES: u32 = 4;

/// The wait after a burst refusal: a second or two, drawn at random so
/// requests refused together spread out.
pub fn burst_pause() -> Duration {
    Duration::from_millis(1_000 + jitter(1_001))
}

/// The wait after the `count`th per-minute refusal in a row: 2 s, then a
/// second more each time up to 5 s, Gmail's quota being a bucket measured
/// clearing within 2 s; past [`MINUTE_PAUSES`], until the next minute plus
/// up to a second.
pub fn minute_pause(count: u32, now: SystemTime) -> Duration {
    if count > MINUTE_PAUSES {
        return until_next_minute(now) + Duration::from_millis(jitter(1_000));
    }
    Duration::from_secs(u64::from(count.max(1)) + 1)
}

/// The wait from `now` to the start of the next minute, at least a second.
pub fn until_next_minute(now: SystemTime) -> Duration {
    let millis = now
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        % 60_000;
    Duration::from_millis(60_000 - millis as u64).max(Duration::from_secs(1))
}

/// The wait from `now` to the next midnight in America/Los_Angeles, when
/// Google resets its daily quotas.
pub fn until_pacific_midnight(now: SystemTime) -> Duration {
    let at = DateTime::<Utc>::from(now);
    let local = (at + pacific_offset(at)).date_naive();
    let Some(next) = local.checked_add_days(Days::new(1)) else {
        return Duration::from_secs(86_400);
    };
    let Some(midnight) = next.and_hms_opt(0, 0, 0) else {
        return Duration::from_secs(86_400);
    };
    // NOTE: at midnight the offset is the one of the day it starts, both
    // switches happening at 2 a.m.
    let midnight = Utc.from_utc_datetime(&midnight) - pacific_offset_on(next);
    (midnight - at).to_std().unwrap_or_default()
}

/// The offset of America/Los_Angeles at `at`: daylight time from the second
/// Sunday of March at 2 a.m. standard time to the first Sunday of November
/// at 2 a.m. daylight time.
fn pacific_offset(at: DateTime<Utc>) -> chrono::Duration {
    let year = at.year();
    let start = nth_sunday(year, 3, 2).and_then(|day| day.and_hms_opt(10, 0, 0));
    let end = nth_sunday(year, 11, 1).and_then(|day| day.and_hms_opt(9, 0, 0));
    let naive = at.naive_utc();
    match (start, end) {
        (Some(start), Some(end)) if naive >= start && naive < end => chrono::Duration::hours(-7),
        _ => chrono::Duration::hours(-8),
    }
}

/// The offset of America/Los_Angeles at the start of the local day `day`.
fn pacific_offset_on(day: NaiveDate) -> chrono::Duration {
    let dst = match (nth_sunday(day.year(), 3, 2), nth_sunday(day.year(), 11, 1)) {
        (Some(start), Some(end)) => day > start && day <= end,
        _ => false,
    };
    chrono::Duration::hours(if dst { -7 } else { -8 })
}

/// The `n`th Sunday of `month` in `year`.
fn nth_sunday(year: i32, month: u32, n: u32) -> Option<NaiveDate> {
    NaiveDate::from_weekday_of_month_opt(year, month, Weekday::Sun, n as u8)
}

/// A number below `span`, at random.
fn jitter(span: u64) -> u64 {
    RandomState::new().hash_one(Instant::now()) % span.max(1)
}

/// The wait a Google message states as `Retry after <RFC 3339 instant>`.
pub fn google_retry_after(message: &str) -> Option<Duration> {
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

/// The units spent over the last sixty seconds, a sliding window, so no
/// sixty seconds ever carry more than the budget, whichever minute the
/// provider counts by.
#[derive(Debug, Default)]
struct MinuteBudget {
    /// What was spent and when, oldest first, each instant at or after the
    /// one before it.
    spent: VecDeque<(Instant, u32)>,
    /// The sum of `spent`'s units.
    total: u32,
}

impl MinuteBudget {
    const WINDOW: Duration = Duration::from_secs(60);

    fn new() -> Self {
        Self::default()
    }

    /// Books `units` at the earliest instant from `now` at which the last
    /// sixty seconds hold room for them under `limit`, and answers how long
    /// from `now` that is.
    ///
    /// Spends are booked in order, never one before the one booked ahead
    /// of it, so concurrent takers queue. A spend larger than the whole
    /// budget counts as the whole budget, waiting for an empty minute
    /// rather than forever.
    fn reserve(&mut self, now: Instant, units: u32, limit: u32) -> Duration {
        let units = units.min(limit);
        let mut at = self.spent.back().map_or(now, |(last, _)| now.max(*last));

        while let Some(&(oldest, spent)) = self.spent.front() {
            let aged = oldest + Self::WINDOW <= at;
            if !aged && self.total + units <= limit {
                break;
            }
            at = at.max(oldest + Self::WINDOW);
            self.spent.pop_front();
            self.total -= spent;
        }

        self.spent.push_back((at, units));
        self.total += units;
        at.saturating_duration_since(now)
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

    /// A Google answer read from its status and message alone, as io-gcal
    /// keeps it.
    fn text(status: u16, message: &str) -> Option<GoogleRefusal> {
        google_refusal(&GoogleAnswer {
            status,
            message,
            ..Default::default()
        })
    }

    #[test]
    fn google_refusals_read_from_status_and_message() {
        use GoogleRefusal::*;
        assert_eq!(
            text(429, "Too many concurrent requests for user"),
            Some(Burst)
        );
        assert_eq!(text(503, "Backend Error"), Some(Burst));
        assert_eq!(text(403, "Rate Limit Exceeded"), Some(Burst));
        assert_eq!(text(403, "User Rate Limit Exceeded"), Some(Burst));
        assert_eq!(
            text(
                403,
                "Quota exceeded for quota metric 'Total Query Cost' and limit 'Units per minute per user' of service 'gmail.googleapis.com'"
            ),
            Some(Minute),
            "the wording names the minute"
        );
        assert_eq!(
            text(
                429,
                "Quota exceeded for quota metric 'Queries' and limit 'Queries per minute per user'"
            ),
            Some(Minute)
        );
        assert_eq!(text(403, "Daily Limit Exceeded"), Some(Daily));
        assert_eq!(
            text(
                403,
                "Quota exceeded for quota metric 'Queries' and limit 'Queries per day'"
            ),
            Some(Daily)
        );
        assert_eq!(
            text(
                403,
                "Daily Limit for Unauthenticated Use Exceeded. Continued use requires signup."
            ),
            None,
            "a missing credential, not a quota"
        );
        assert_eq!(text(403, "Insufficient Permission"), None);
        assert_eq!(text(404, "Not Found"), None);
        assert_eq!(text(500, ""), None);
    }

    #[test]
    fn google_refusals_read_from_reasons() {
        use GoogleRefusal::*;
        let reasons = |status, message, reasons: &[&str]| {
            let reasons: Vec<String> = reasons.iter().map(|r| r.to_string()).collect();
            google_refusal(&GoogleAnswer {
                status,
                message,
                reasons: &reasons,
                ..Default::default()
            })
        };
        assert_eq!(reasons(403, "Too many", &["quotaExceeded"]), Some(Burst));
        assert_eq!(reasons(403, "No", &["userRateLimitExceeded"]), Some(Burst));
        assert_eq!(reasons(403, "Quota", &["dailyLimitExceeded"]), Some(Daily));
        assert_eq!(reasons(403, "No", &["insufficientPermissions"]), None);
        assert_eq!(
            google_refusal(&GoogleAnswer {
                status: 429,
                message: "",
                detail_reasons: &[
                    "RATE_LIMIT_EXCEEDED".into(),
                    "ReadRequestsPerMinutePerUser".into()
                ],
                ..Default::default()
            }),
            Some(Minute),
            "an ErrorInfo naming the minute"
        );
        assert_eq!(
            google_refusal(&GoogleAnswer {
                status: 403,
                message: "Too many",
                google_status: Some("RESOURCE_EXHAUSTED"),
                ..Default::default()
            }),
            Some(Burst)
        );
    }

    #[test]
    fn a_minute_refusal_waits_two_to_five_seconds_then_the_next_minute() {
        let waits = GoogleWaits::new();
        let now = UNIX_EPOCH + Duration::from_secs(1_790_000_000) + Duration::from_millis(15_250);
        let pauses: Vec<Duration> = (0..4)
            .map(|_| waits.wait(GoogleRefusal::Minute, None, now))
            .collect();
        assert_eq!(
            pauses,
            [2, 3, 4, 5].map(Duration::from_secs),
            "a few seconds more each time"
        );
        let fifth = waits.wait(GoogleRefusal::Minute, None, now);
        let boundary = until_next_minute(now);
        assert!(
            fifth >= boundary && fifth < boundary + Duration::from_secs(1),
            "{fifth:?} against {boundary:?}"
        );
    }

    #[test]
    fn a_burst_waits_a_second_or_two_or_what_google_states() {
        let waits = GoogleWaits::new();
        for _ in 0..20 {
            let wait = waits.wait(GoogleRefusal::Burst, None, SystemTime::now());
            assert!((Duration::from_secs(1)..=Duration::from_secs(2)).contains(&wait));
        }
        let stated = Some(Duration::from_secs(7));
        assert_eq!(
            waits.wait(GoogleRefusal::Burst, stated, SystemTime::now()),
            Duration::from_secs(7)
        );
        assert!(
            waits
                .throttled(
                    Request::Idempotent,
                    &GoogleAnswer {
                        status: 403,
                        message: "User-rate limit exceeded.  Retry after 2099-01-01T00:00:00.000Z",
                        ..Default::default()
                    }
                )
                .flatten()
                .is_some_and(|wait| wait > Duration::from_secs(3600)),
            "Gmail's stated instant"
        );
    }

    #[test]
    fn the_next_minute_is_at_least_a_second_away() {
        let at = |secs: u64, millis: u64| {
            until_next_minute(
                UNIX_EPOCH + Duration::from_secs(secs) + Duration::from_millis(millis),
            )
        };
        assert_eq!(at(120, 0), Duration::from_secs(60));
        assert_eq!(at(150, 500), Duration::from_millis(29_500));
        assert_eq!(at(179, 800), Duration::from_secs(1));
    }

    /// Google resets its daily quotas at midnight Pacific time, standard or
    /// daylight.
    #[test]
    fn a_daily_quota_gives_up_until_pacific_midnight() {
        let at = |instant: &str| {
            let now = SystemTime::from(DateTime::parse_from_rfc3339(instant).unwrap());
            rfc3339(now + until_pacific_midnight(now))
        };
        // NOTE: daylight time, UTC-7.
        assert_eq!(at("2026-10-13T12:00:00Z"), "2026-10-14T07:00:00Z");
        assert_eq!(at("2026-10-14T06:59:59Z"), "2026-10-14T07:00:00Z");
        assert_eq!(at("2026-10-14T07:00:00Z"), "2026-10-15T07:00:00Z");
        // NOTE: standard time, UTC-8.
        assert_eq!(at("2026-12-01T00:00:00Z"), "2026-12-01T08:00:00Z");
        // NOTE: the night daylight time ends (2026-11-01): its midnight is
        // still daylight, the next one standard.
        assert_eq!(at("2026-10-31T20:00:00Z"), "2026-11-01T07:00:00Z");
        assert_eq!(at("2026-11-01T12:00:00Z"), "2026-11-02T08:00:00Z");
        // NOTE: the night daylight time starts (2026-03-08).
        assert_eq!(at("2026-03-08T12:00:00Z"), "2026-03-09T07:00:00Z");
        assert_eq!(at("2026-03-07T12:00:00Z"), "2026-03-08T08:00:00Z");

        let waits = GoogleWaits::new();
        let throttle = Throttle::new("gmail");
        let calls = Cell::new(0);
        let out: Result<(), u16> = throttle.call(
            1,
            || {
                calls.set(calls.get() + 1);
                Err(403)
            },
            |_| {
                waits.throttled(
                    Request::Idempotent,
                    &GoogleAnswer {
                        status: 403,
                        message: "Quota exceeded for quota metric 'Queries' and limit 'Queries per day'",
                        ..Default::default()
                    },
                )
            },
            |_| 0,
        );
        assert_eq!(out, Err(403));
        assert_eq!(calls.get(), 1, "never sent again in the run");
        assert!(
            throttle
                .gave_up()
                .is_some_and(|until| until > SystemTime::now() + Duration::from_secs(60))
        );
    }

    #[test]
    fn a_minute_budget_holds_sixty_seconds_under_its_limit() {
        let start = Instant::now();
        let mut budget = MinuteBudget::new();
        for _ in 0..60 {
            assert_eq!(budget.reserve(start, 100, 6_000), Duration::ZERO);
        }
        assert_eq!(
            budget.reserve(start, 100, 6_000),
            Duration::from_secs(60),
            "the minute is spent"
        );
        let later = start + Duration::from_secs(30);
        assert_eq!(
            budget.reserve(later, 100, 6_000),
            Duration::from_secs(30),
            "booked behind the one before it"
        );
        assert_eq!(
            budget.reserve(start, 10_000, 6_000),
            Duration::from_secs(120),
            "a spend past the budget waits for an empty minute"
        );
    }

    #[test]
    fn a_paced_source_takes_from_both_budgets() {
        let throttle = Throttle::paced_per_minute("gmail", 1_000.0, 100);
        let start = Instant::now();
        throttle.pace(50);
        throttle.pace(50);
        assert!(
            start.elapsed() < Duration::from_secs(1),
            "within the minute"
        );
        let budget = throttle.minute.as_ref().unwrap();
        let wait = budget.1.lock().unwrap().reserve(Instant::now(), 1, 100);
        assert!(wait > Duration::from_secs(58), "{wait:?}");
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

        let throttled = |request, status, message| {
            GoogleWaits::new()
                .throttled(
                    request,
                    &GoogleAnswer {
                        status,
                        message,
                        ..Default::default()
                    },
                )
                .is_some()
        };
        assert!(throttled(Request::Idempotent, 503, "Backend Error"));
        assert!(!throttled(Request::Create, 503, "Backend Error"));
        assert!(
            throttled(Request::Create, 403, "Rate Limit Exceeded"),
            "a rate limit refuses the request before doing it"
        );
        assert!(throttled(Request::Create, 429, "Too many requests"));
    }
}
