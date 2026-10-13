//! # Gmail batched reads
//!
//! Reads many messages through Gmail's HTTP batch endpoint, a batch at a
//! time: Gmail answers each read of a batch on its own, so some may land
//! while others are refused (a 429 for concurrency, a 403 for a quota) or
//! gone (404). [`read_all`] hands each message to its caller once, puts a
//! refused read back at the head of the next batch after a short pause, and
//! gives the source up on a daily quota.
//!
//! The sender is the [`Batcher`] trait, so the reads can be counted and
//! refused without a network.

use std::{
    collections::{HashMap, VecDeque},
    time::{Duration, SystemTime},
};

use anyhow::{Result, anyhow};
use io_gmail::v1::{
    batch::{GmailBatchResponse, GmailBatchResponseError},
    rest::messages::{GmailMessage, GmailMessageFormat},
};
use log::debug;

use crate::throttle::{
    GoogleAnswer, GoogleRefusal, GoogleWaits, burst_pause, google_refusal, rfc3339,
};

/// About how many bytes of messages one raw batch carries, by their sizes:
/// the answer is their base64, a third larger, and is held whole.
pub const RAW_BATCH_BYTES: u64 = 8 * 1024 * 1024;

/// The size past which a message is read alone rather than in a batch.
pub const ALONE_BYTES: u64 = 5 * 1024 * 1024;

/// How many times one read is sent in all before the read fails.
pub const MAX_TRIES: u32 = 6;

/// The headers a metadata read asks for, what a `Meta`-tier summary reads.
pub const SUMMARY_HEADERS: &[&str] = &[
    "Message-ID",
    "In-Reply-To",
    "Subject",
    "From",
    "To",
    "Cc",
    "Bcc",
    "Date",
    "Content-Type",
];

/// What a message is read as.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Read {
    /// Its labels and the headers a summary takes.
    Metadata,
    /// Its labels and size alone.
    Minimal,
    /// Its bytes.
    Raw,
}

impl Read {
    pub fn format(self) -> GmailMessageFormat {
        match self {
            Self::Metadata => GmailMessageFormat::Metadata,
            Self::Minimal => GmailMessageFormat::Minimal,
            Self::Raw => GmailMessageFormat::Raw,
        }
    }

    pub fn headers(self) -> &'static [&'static str] {
        match self {
            Self::Metadata => SUMMARY_HEADERS,
            Self::Minimal | Self::Raw => &[],
        }
    }
}

/// How one inner answer of a batch is taken.
#[derive(Debug)]
pub enum Inner {
    /// Read.
    Read(Box<GmailMessage>),
    /// Gone since it was listed (404).
    Gone,
    /// A concurrency or rate refusal, a transient failure, or no answer:
    /// sent again in the next batch after a second or two.
    Busy,
    /// A per-minute quota refusal: sent again after a few seconds.
    Minute,
    /// A daily quota refusal: nothing within the run lifts it.
    Daily,
    /// Any other refusal, which fails the read.
    Failed(String),
}

/// How one inner answer of a batch is taken ([`Inner`]).
pub fn inner(result: Result<GmailMessage, GmailBatchResponseError>) -> Inner {
    let err = match result {
        Ok(message) => return Inner::Read(Box::new(message)),
        Err(err) => err,
    };
    if err.is_not_found() {
        return Inner::Gone;
    }
    if let Some(api) = err.api() {
        let answer = GoogleAnswer {
            status: api.status,
            message: &api.message,
            reasons: &api.reasons,
            detail_reasons: &api.detail_reasons,
            google_status: api.google_status.as_deref(),
        };
        match google_refusal(&answer) {
            Some(GoogleRefusal::Daily) => return Inner::Daily,
            Some(GoogleRefusal::Minute) => return Inner::Minute,
            Some(GoogleRefusal::Burst) => return Inner::Busy,
            None => {}
        }
    }
    if err.is_retryable() {
        return Inner::Busy;
    }
    Inner::Failed(err.to_string())
}

/// How reads are sent: a batch, one read alone, and the waits between
/// batches. The live one is the Gmail client; the tests count and refuse.
pub trait Batcher {
    /// One batch of reads, answered in any order, one in flight across the
    /// source.
    fn batch(
        &mut self,
        ids: &[String],
        read: Read,
    ) -> Result<Vec<GmailBatchResponse<GmailMessage>>>;

    /// One read alone, through the throttled path: `None` when gone.
    fn single(&mut self, id: &str, read: Read) -> Result<Option<GmailMessage>>;

    /// Waits `wait` before the next batch, or gives the source up when the
    /// wait is too long to hold the run.
    fn pause(&mut self, wait: Duration) -> Result<()>;

    /// Gives the source up until `until`, so its connections send nothing
    /// before.
    fn give_up(&mut self, until: SystemTime);
}

/// The size of a message, for a raw read.
pub type Sizes<'a> = &'a dyn Fn(&str) -> u64;

/// Reads every message of `ids` as `read`, handing each to `each` once,
/// `None` for one gone: batches of at most `batch_size` sent one after
/// another; for a raw read, a batch holds about [`RAW_BATCH_BYTES`] by the
/// sizes `sizes` names, and a message past [`ALONE_BYTES`] is read alone.
///
/// A read refused for concurrency or rate is put back at the head of the
/// next batch, sent after a second or two; one refused a per-minute quota
/// likewise after two to five seconds, then the next minute. A daily quota
/// gives the source up until Google's midnight, and a read refused
/// [`MAX_TRIES`] times gives it up for the last wait: the read fails, what
/// `each` took staying taken.
pub fn read_all<B: Batcher>(
    batcher: &mut B,
    read: Read,
    ids: &[String],
    batch_size: usize,
    sizes: Sizes<'_>,
    mut each: impl FnMut(String, Option<GmailMessage>) -> Result<()>,
) -> Result<()> {
    let size = |id: &str| match read {
        Read::Raw => sizes(id),
        _ => 0,
    };
    let batch_size = batch_size.max(1);
    let mut queue: VecDeque<String> = ids.iter().cloned().collect();
    let mut tries: HashMap<String, u32> = HashMap::new();
    let waits = GoogleWaits::new();
    let mut pause = Duration::ZERO;

    while let Some(front) = queue.front() {
        if !pause.is_zero() {
            batcher.pause(pause)?;
            pause = Duration::ZERO;
        }
        if size(front) > ALONE_BYTES {
            let Some(id) = queue.pop_front() else {
                break;
            };
            let message = batcher.single(&id, read)?;
            each(id, message)?;
            continue;
        }

        let mut batch = Vec::new();
        let mut bytes = 0;
        while let Some(id) = queue.front() {
            let next = size(id);
            if batch.len() == batch_size
                || next > ALONE_BYTES
                || (!batch.is_empty() && bytes + next > RAW_BATCH_BYTES)
            {
                break;
            }
            bytes += next;
            batch.extend(queue.pop_front());
        }

        let mut answers: HashMap<String, Result<GmailMessage, GmailBatchResponseError>> = batcher
            .batch(&batch, read)?
            .into_iter()
            .map(|answer| (answer.id, answer.result))
            .collect();

        let mut again = Vec::new();
        let mut minute = false;
        for id in batch {
            let answer = answers
                .remove(&id)
                .unwrap_or(Err(GmailBatchResponseError::Missing));
            match inner(answer) {
                Inner::Read(message) => each(id, Some(*message))?,
                Inner::Gone => each(id, None)?,
                Inner::Busy => {
                    pause = pause.max(burst_pause());
                    again.push(id);
                }
                Inner::Minute => {
                    minute = true;
                    again.push(id);
                }
                Inner::Daily => {
                    let until = SystemTime::now()
                        + waits.wait(GoogleRefusal::Daily, None, SystemTime::now());
                    batcher.give_up(until);
                    return Err(anyhow!(
                        "Gmail's daily quota is spent, the source gave up until {}",
                        rfc3339(until)
                    ));
                }
                Inner::Failed(err) => {
                    return Err(anyhow!("Read message {id} error: {err}"));
                }
            }
        }
        // NOTE: one wait a batch refused for the minute, however many of its
        // reads were, growing while the batches after it are refused too.
        if minute {
            let wait = waits.wait(GoogleRefusal::Minute, None, SystemTime::now());
            pause = pause.max(wait);
        } else {
            waits.reset();
        }

        if again.is_empty() {
            continue;
        }
        debug!(
            "gmail batch: {} refused, sent again in {} ms",
            again.len(),
            pause.as_millis()
        );
        for id in again.into_iter().rev() {
            let tried = tries.entry(id.clone()).or_default();
            *tried += 1;
            if *tried >= MAX_TRIES {
                let until = SystemTime::now() + pause;
                batcher.give_up(until);
                return Err(anyhow!(
                    "Gmail refused message {id} {MAX_TRIES} times, the source gave up until {}",
                    rfc3339(until)
                ));
            }
            queue.push_front(id);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use io_gmail::{
        coroutine::*,
        v1::{
            batch::{GmailBatch, GmailBatchRequest},
            client::GmailClientStd,
            rest::messages::encode_raw,
            send::{GmailSendError, GmailSendOutput},
        },
    };

    use super::*;

    /// The per-minute quota refusal Gmail answered on 2026-10-07, its
    /// project number made up.
    pub const MINUTE_QUOTA: &str = r#"{"error":{"code":403,"message":"Quota exceeded for quota metric 'Total Query Cost' and limit 'Units per minute per user' of service 'gmail.googleapis.com' for consumer 'project_number:123'.","errors":[{"message":"Quota exceeded","domain":"usageLimits","reason":"rateLimitExceeded"}],"status":"PERMISSION_DENIED"}}"#;

    /// Gmail's per-user concurrency limit, as a batch's inner read meets it.
    pub const CONCURRENCY: &str = r#"{"error":{"code":429,"message":"Too many concurrent requests for user","errors":[{"message":"Too many concurrent requests for user","domain":"global","reason":"rateLimitExceeded"}],"status":"RESOURCE_EXHAUSTED"}}"#;

    /// Gmail's daily quota refusal.
    pub const DAILY_QUOTA: &str = r#"{"error":{"code":403,"message":"Quota exceeded for quota metric 'Queries' and limit 'Queries per day'","errors":[{"message":"Daily Limit Exceeded","domain":"usageLimits","reason":"dailyLimitExceeded"}],"status":"PERMISSION_DENIED"}}"#;

    /// The three refusals, as io-gmail reads their envelopes, classed by
    /// their reasons and wording.
    #[test]
    fn real_refusals_are_classed_by_reason() {
        let class = |status, body: &str| {
            let api = io_gmail::v1::send::GmailApiError::parse(status, body.as_bytes());
            google_refusal(&GoogleAnswer {
                status: api.status,
                message: &api.message,
                reasons: &api.reasons,
                detail_reasons: &api.detail_reasons,
                google_status: api.google_status.as_deref(),
            })
        };
        assert_eq!(class(403, MINUTE_QUOTA), Some(GoogleRefusal::Minute));
        assert_eq!(class(429, CONCURRENCY), Some(GoogleRefusal::Burst));
        assert_eq!(class(403, DAILY_QUOTA), Some(GoogleRefusal::Daily));
    }

    /// A metadata answer as Gmail writes it, for one message.
    fn metadata_json(id: &str) -> String {
        serde_json::json!({
            "id": id,
            "threadId": "t1",
            "labelIds": ["INBOX", "Label_7", "UNREAD"],
            "sizeEstimate": 2048,
            "payload": {
                "mimeType": "multipart/alternative",
                "headers": [
                    {"name": "Date", "value": "Tue, 14 Nov 2023 22:13:20 +0100"},
                    {"name": "From", "value": "Ren\u{e9} <rene@example.org>"},
                    {"name": "Subject", "value": "=?UTF-8?Q?R=C3=A9union?="},
                    {"name": "Message-ID", "value": format!("<{id}@example.org>")},
                ]
            }
        })
        .to_string()
    }

    /// The source the fake account serves for one message.
    pub fn source_of(id: &str) -> String {
        format!(
            "Date: Tue, 14 Nov 2023 22:13:20 +0100\r\nFrom: rene@example.org\r\nSubject: Mail {id}\r\nMessage-ID: <{id}@example.org>\r\n\r\nBody of {id}\r\n"
        )
    }

    fn raw_json(id: &str) -> String {
        serde_json::json!({
            "id": id,
            "labelIds": ["INBOX"],
            "sizeEstimate": source_of(id).len(),
            "raw": encode_raw(source_of(id).as_bytes()),
        })
        .to_string()
    }

    fn minimal_json(id: &str, size: u64) -> String {
        serde_json::json!({ "id": id, "labelIds": ["INBOX"], "sizeEstimate": size }).to_string()
    }

    /// Runs a Gmail coroutine against one canned answer.
    fn drive<C, T>(mut coroutine: C, answer: &[u8]) -> Result<GmailSendOutput<T>, GmailSendError>
    where
        C: GmailCoroutine<Yield = GmailYield, Return = Result<GmailSendOutput<T>, GmailSendError>>,
    {
        let mut answered = false;
        let mut arg: Option<&[u8]> = None;
        loop {
            match coroutine.resume(arg.take()) {
                GmailCoroutineState::Complete(result) => return result,
                GmailCoroutineState::Yielded(GmailYield::WantsWrite(_)) => {}
                GmailCoroutineState::Yielded(GmailYield::WantsRead) => {
                    arg = Some(match answered {
                        false => answer,
                        true => &[],
                    });
                    answered = true;
                }
            }
        }
    }

    /// A batch answer: one part per `(id, status, body)`, in the order
    /// given, which Google does not promise to keep.
    pub fn batch_answer(parts: &[(&str, &str, String)]) -> Vec<u8> {
        let mut body = String::new();
        for (id, status, json) in parts {
            body.push_str("--batch_x\r\nContent-Type: application/http\r\n");
            body.push_str(&format!("Content-ID: <response-{id}>\r\n\r\n"));
            body.push_str(&format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json; charset=UTF-8\r\n\r\n{json}\r\n"
            ));
        }
        body.push_str("--batch_x--\r\n");
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: multipart/mixed; boundary=batch_x\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        )
        .into_bytes()
    }

    /// Runs one batch of `ids` against the parts of its answer.
    pub fn batched(
        ids: &[&str],
        read: Read,
        parts: &[(&str, &str, String)],
    ) -> Vec<GmailBatchResponse<GmailMessage>> {
        let requests: Vec<GmailBatchRequest> = ids
            .iter()
            .map(|id| GmailBatchRequest::message_get(id, "me", id, read.format(), read.headers()))
            .collect::<Result<_, _>>()
            .unwrap();
        let client = GmailClientStd::new(
            std::io::Cursor::new(Vec::new()),
            "token",
            Default::default(),
        );
        let batch = GmailBatch::new(&client.auth, &requests).unwrap();
        drive(batch, &batch_answer(parts)).unwrap().response
    }

    #[test]
    fn an_inner_answer_is_read_gone_busy_minute_daily_or_failed() {
        let parts = [
            ("a1", "200 OK", metadata_json("a1")),
            ("b2", "404 Not Found", r#"{"error":{"code":404}}"#.into()),
            ("c3", "429 Too Many Requests", CONCURRENCY.into()),
            ("d4", "403 Forbidden", MINUTE_QUOTA.into()),
            ("f6", "403 Forbidden", DAILY_QUOTA.into()),
            ("g7", "401 Unauthorized", "{}".into()),
            ("h8", "503 Service Unavailable", "{}".into()),
        ];
        // NOTE: e5 has no part in the answer at all.
        let ids = ["a1", "b2", "c3", "d4", "e5", "f6", "g7", "h8"];
        let taken: BTreeMap<String, String> = batched(&ids, Read::Metadata, &parts)
            .into_iter()
            .map(|answer| {
                let kind = match inner(answer.result) {
                    Inner::Read(_) => "read",
                    Inner::Gone => "gone",
                    Inner::Busy => "busy",
                    Inner::Minute => "minute",
                    Inner::Daily => "daily",
                    Inner::Failed(_) => "failed",
                };
                (answer.id, kind.to_string())
            })
            .collect();
        let expected: BTreeMap<String, String> = [
            ("a1", "read"),
            ("b2", "gone"),
            ("c3", "busy"),
            ("d4", "minute"),
            ("e5", "busy"),
            ("f6", "daily"),
            ("g7", "failed"),
            ("h8", "busy"),
        ]
        .into_iter()
        .map(|(id, kind)| (id.to_string(), kind.to_string()))
        .collect();
        assert_eq!(taken, expected);
    }

    /// How the fake account answers one message's next read.
    #[derive(Clone, Copy)]
    pub enum Answer {
        Concurrency,
        Minute,
        Daily,
    }

    /// A Gmail answering batches from a script: refusals queued per
    /// message, sizes for the minimal reads, the rest served.
    #[derive(Default)]
    pub struct FakeBatcher {
        /// The largest batch the source takes.
        pub batch_size: usize,
        /// The refusals each message meets before it is served, in order.
        pub refusals: BTreeMap<String, Vec<Answer>>,
        /// Messages gone.
        pub gone: BTreeSet<String>,
        /// Sizes the minimal reads report, 1 KB unless named.
        pub sizes: BTreeMap<String, u64>,
        /// Every batch sent: what it read and its ids.
        pub batches: Vec<(Read, Vec<String>)>,
        /// Every read sent alone.
        pub singles: Vec<String>,
        pub paused: Vec<Duration>,
        pub gave_up: Option<SystemTime>,
    }

    impl FakeBatcher {
        pub fn new() -> Self {
            Self {
                batch_size: 10,
                ..Default::default()
            }
        }

        fn answer(&self, id: &str, read: Read) -> String {
            match read {
                Read::Metadata => metadata_json(id),
                Read::Minimal => minimal_json(id, self.sizes.get(id).copied().unwrap_or(1024)),
                Read::Raw => raw_json(id),
            }
        }
    }

    impl Batcher for FakeBatcher {
        fn batch(
            &mut self,
            ids: &[String],
            read: Read,
        ) -> Result<Vec<GmailBatchResponse<GmailMessage>>> {
            assert!(
                ids.len() <= self.batch_size,
                "{} reads in one batch",
                ids.len()
            );
            assert!(self.gave_up.is_none(), "nothing sent once given up");
            self.batches.push((read, ids.to_vec()));
            let mut parts: Vec<(&str, &str, String)> = Vec::new();
            for id in ids {
                let refusal = self
                    .refusals
                    .get_mut(id)
                    .filter(|queued| !queued.is_empty())
                    .map(|queued| queued.remove(0));
                parts.push(match refusal {
                    Some(Answer::Concurrency) => (id, "429 Too Many Requests", CONCURRENCY.into()),
                    Some(Answer::Minute) => (id, "403 Forbidden", MINUTE_QUOTA.into()),
                    Some(Answer::Daily) => (id, "403 Forbidden", DAILY_QUOTA.into()),
                    None if self.gone.contains(id) => (id, "404 Not Found", "{}".into()),
                    None => (id, "200 OK", self.answer(id, read)),
                });
            }
            let ids: Vec<&str> = ids.iter().map(String::as_str).collect();
            Ok(batched(&ids, read, &parts))
        }

        fn single(&mut self, id: &str, read: Read) -> Result<Option<GmailMessage>> {
            self.singles.push(id.into());
            if self.gone.contains(id) {
                return Ok(None);
            }
            Ok(Some(serde_json::from_str(&self.answer(id, read)).unwrap()))
        }

        fn pause(&mut self, wait: Duration) -> Result<()> {
            self.paused.push(wait);
            Ok(())
        }

        fn give_up(&mut self, until: SystemTime) {
            self.gave_up = Some(until);
        }
    }

    pub fn ids(count: usize) -> Vec<String> {
        (0..count).map(|n| format!("m{n:03}")).collect()
    }

    fn no_size(_: &str) -> u64 {
        0
    }

    /// Reads `ids` as metadata, each answer kept as `(id, read)`.
    fn read_metadata(batcher: &mut FakeBatcher, ids: &[String]) -> Result<Vec<(String, bool)>> {
        let mut read = Vec::new();
        read_all(batcher, Read::Metadata, ids, 10, &no_size, |id, message| {
            read.push((id, message.is_some()));
            Ok(())
        })?;
        Ok(read)
    }

    #[test]
    fn reads_ride_batches_of_ten_the_refused_put_back_into_the_next() {
        let ids = ids(45);
        let mut batcher = FakeBatcher {
            refusals: [("m007".to_string(), vec![Answer::Concurrency])].into(),
            gone: ["m030".to_string()].into(),
            ..FakeBatcher::new()
        };

        let read = read_metadata(&mut batcher, &ids).unwrap();

        let sizes: Vec<usize> = batcher.batches.iter().map(|(_, ids)| ids.len()).collect();
        assert_eq!(sizes, [10, 10, 10, 10, 6], "the refused one rides again");
        assert_eq!(
            batcher.batches[1].1[0], "m007",
            "at the head of the next batch"
        );
        assert!(batcher.singles.is_empty(), "never read again alone");
        assert_eq!(batcher.paused.len(), 1);
        let paused = batcher.paused[0];
        assert!(
            (Duration::from_secs(1)..=Duration::from_secs(2)).contains(&paused),
            "{paused:?}"
        );
        assert_eq!(read.len(), 45, "every id answered once");
        let gone: Vec<&str> = read
            .iter()
            .filter(|(_, read)| !read)
            .map(|(id, _)| id.as_str())
            .collect();
        assert_eq!(gone, ["m030"]);
        assert!(batcher.gave_up.is_none());
    }

    #[test]
    fn a_batch_size_is_followed() {
        let mut batcher = FakeBatcher {
            batch_size: 25,
            ..FakeBatcher::new()
        };
        read_all(
            &mut batcher,
            Read::Minimal,
            &ids(60),
            25,
            &no_size,
            |_, _| Ok(()),
        )
        .unwrap();
        let sizes: Vec<usize> = batcher.batches.iter().map(|(_, ids)| ids.len()).collect();
        assert_eq!(sizes, [25, 25, 10]);
    }

    #[test]
    fn minute_refusals_back_off_more_each_time_and_a_daily_one_gives_up() {
        let mut batcher = FakeBatcher {
            refusals: [
                ("m001".to_string(), vec![Answer::Minute, Answer::Minute]),
                ("m002".to_string(), vec![Answer::Minute]),
            ]
            .into(),
            ..FakeBatcher::new()
        };
        read_metadata(&mut batcher, &ids(3)).unwrap();
        assert_eq!(
            batcher.paused,
            [Duration::from_secs(2), Duration::from_secs(3)],
            "one wait a batch, a second more each time"
        );

        let mut batcher = FakeBatcher {
            refusals: [("m001".to_string(), vec![Answer::Daily])].into(),
            ..FakeBatcher::new()
        };
        let err = read_metadata(&mut batcher, &ids(3)).unwrap_err();
        assert!(err.to_string().contains("daily quota"), "{err}");
        assert_eq!(batcher.batches.len(), 1, "nothing sent after it");
        let until = batcher.gave_up.expect("the source gave up");
        assert!(until > SystemTime::now() + Duration::from_secs(60));
        assert!(until < SystemTime::now() + Duration::from_secs(25 * 3600));
    }

    #[test]
    fn the_fifth_minute_refusal_in_a_row_waits_for_the_next_minute() {
        let mut batcher = FakeBatcher {
            refusals: [("m000".to_string(), vec![Answer::Minute; 5])].into(),
            ..FakeBatcher::new()
        };
        read_metadata(&mut batcher, &ids(1)).unwrap();
        let paused: Vec<u64> = batcher.paused.iter().map(Duration::as_secs).collect();
        assert_eq!(&paused[..4], [2, 3, 4, 5]);
        assert!(
            batcher.paused[4] >= Duration::from_secs(1)
                && batcher.paused[4] <= Duration::from_secs(61),
            "{:?}",
            batcher.paused[4]
        );
        assert_eq!(batcher.batches.len(), 6);
    }

    #[test]
    fn a_message_refused_without_end_gives_the_source_up() {
        let mut batcher = FakeBatcher {
            refusals: [("m000".to_string(), vec![Answer::Concurrency; 10])].into(),
            ..FakeBatcher::new()
        };
        let err = read_metadata(&mut batcher, &ids(1)).unwrap_err();
        assert!(err.to_string().contains("6 times"), "{err}");
        assert_eq!(batcher.batches.len(), MAX_TRIES as usize);
        assert!(batcher.gave_up.is_some());
    }

    #[test]
    fn a_partly_refused_batch_keeps_what_landed() {
        let mut batcher = FakeBatcher {
            refusals: [
                ("m003".to_string(), vec![Answer::Concurrency]),
                ("m004".to_string(), vec![Answer::Minute]),
            ]
            .into(),
            ..FakeBatcher::new()
        };
        let read = read_metadata(&mut batcher, &ids(12)).unwrap();
        let order: Vec<&str> = read.iter().map(|(id, _)| id.as_str()).collect();
        assert_eq!(order.len(), 12);
        assert_eq!(
            &batcher.batches[1].1[..2],
            ["m003", "m004"],
            "in their order"
        );
        assert_eq!(
            batcher.paused,
            [Duration::from_secs(2)],
            "the longer of the two"
        );
    }

    #[test]
    fn raw_batches_hold_about_eight_megabytes_and_a_large_message_goes_alone() {
        const MB: u64 = 1024 * 1024;
        let sizes: BTreeMap<String, u64> = [
            ("m000", MB),
            ("m001", 3 * MB),
            ("m002", 6 * MB),
            ("m003", 4 * MB),
            ("m004", 2 * MB),
            ("m005", MB),
        ]
        .into_iter()
        .map(|(id, size)| (id.to_string(), size))
        .collect();
        let mut batcher = FakeBatcher::new();
        let size = |id: &str| sizes.get(id).copied().unwrap_or(0);

        let mut fetched = Vec::new();
        read_all(
            &mut batcher,
            Read::Raw,
            &ids(6),
            10,
            &size,
            |id, message| {
                let raw = message.and_then(|message| message.raw).unwrap();
                let bytes = io_gmail::v1::rest::messages::decode_raw(&raw).unwrap();
                assert_eq!(bytes, source_of(&id).as_bytes());
                fetched.push(id);
                Ok(())
            },
        )
        .unwrap();

        let strings = |ids: &[&str]| ids.iter().map(|id| id.to_string()).collect::<Vec<_>>();
        let raw: Vec<Vec<String>> = batcher.batches.iter().map(|(_, ids)| ids.clone()).collect();
        assert_eq!(
            raw,
            [
                strings(&["m000", "m001"]),
                strings(&["m003", "m004", "m005"])
            ],
            "4 MB, then the 6 MB one alone, then 7 MB"
        );
        assert_eq!(batcher.singles, ["m002"]);
        assert_eq!(fetched.len(), 6);
    }
}
