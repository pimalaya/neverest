//! # Gmail run
//!
//! What every connection of one Gmail source shares for one run: its batch
//! settings and the gate its batches go through, the metadata and sizes it
//! already read, and the mailbox history it read once.
//!
//! Gmail holds one message under several labels, each a collection, so the
//! listing of each label meets the same message again. The run reads it
//! once: a message read under one label, carrying others, is kept until
//! they list it, and the history is read once for the mailbox, then
//! projected on each label. Every cache is bounded, the oldest entries
//! leaving first, a miss costing one read more, never a wrong answer.
//!
//! A head listed on a label (pimdir SYNC §4) is recorded with what it
//! touched, so the label's later pages read those messages anew rather
//! than as they stood before it.

use std::{
    collections::{BTreeSet, HashMap, HashSet, VecDeque},
    sync::{Condvar, Mutex, MutexGuard, PoisonError},
    time::Instant,
};

use anyhow::{Result, bail};
use io_gmail::v1::{
    batch::GMAIL_BATCH_MAX_REQUESTS,
    rest::{history::GmailHistory, messages::GmailMessage},
};

/// The reads one batch carries by default: Gmail runs them side by side,
/// and its per-user concurrency limit is what binds. Measured on
/// 2026-10-10, batches of 10 sent one after another drew no refusal in
/// 2,250 reads, 25 drew 1.2 % and 50 drew 19 %.
pub const BATCH_SIZE: usize = 10;

/// The batches in flight at once across the source's connections by
/// default: two batches of 50 side by side drew 17 to 33 refusals of 50.
pub const BATCHES_IN_FLIGHT: usize = 1;

/// The quota units the source spends over any sixty seconds by default.
///
/// Gmail bills a `messages.get` 5 units (measured on 2026-10-10, the
/// refusal naming the quota it counted against), and a Google project gets
/// 6,000 units a minute a user (read in the console of MOA's project on
/// 2026-10-09): 1,200 reads a minute. Google documents 20 units a read and
/// 15,000 a minute; the units counted here are the ones billed, and a
/// refusal past the quota is waited out all the same.
pub const UNITS_PER_MINUTE: u32 = 6_000;

/// The most messages whose metadata waits for their other labels.
const METAS_CAP: usize = 5_000;

/// The most message sizes kept for the body reads.
const SIZES_CAP: usize = 100_000;

/// A Gmail source's batch settings, from its configuration.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct GmailSettings {
    /// The reads one batch carries.
    pub batch_size: usize,
    /// The batches in flight at once across the source's connections.
    pub batches_in_flight: usize,
    /// The quota units spent over any sixty seconds.
    pub units_per_minute: u32,
}

impl Default for GmailSettings {
    fn default() -> Self {
        Self {
            batch_size: BATCH_SIZE,
            batches_in_flight: BATCHES_IN_FLIGHT,
            units_per_minute: UNITS_PER_MINUTE,
        }
    }
}

impl GmailSettings {
    /// The settings `gmail.batch-size`, `gmail.batches-in-flight` and
    /// `gmail.units-per-minute` give, their defaults where absent.
    pub fn new(
        batch_size: Option<usize>,
        batches_in_flight: Option<usize>,
        units_per_minute: Option<u32>,
    ) -> Result<Self> {
        let settings = Self {
            batch_size: batch_size.unwrap_or(BATCH_SIZE),
            batches_in_flight: batches_in_flight.unwrap_or(BATCHES_IN_FLIGHT),
            units_per_minute: units_per_minute.unwrap_or(UNITS_PER_MINUTE),
        };
        if !(1..=GMAIL_BATCH_MAX_REQUESTS).contains(&settings.batch_size) {
            bail!(
                "gmail.batch-size is {}, it takes 1 to {GMAIL_BATCH_MAX_REQUESTS}",
                settings.batch_size
            );
        }
        if settings.batches_in_flight == 0 {
            bail!("gmail.batches-in-flight is 0, it takes 1 or more");
        }
        // NOTE: a minute's budget below one batch would never let a batch
        // through whole.
        let floor = super::client::MESSAGE_GET_UNITS * settings.batch_size as u32;
        if settings.units_per_minute < floor {
            bail!(
                "gmail.units-per-minute is {}, below the {floor} units a batch of {} reads costs",
                settings.units_per_minute,
                settings.batch_size
            );
        }
        Ok(settings)
    }
}

/// What a Gmail source's connections share for one run.
#[derive(Debug)]
pub struct GmailRun {
    /// The batch settings.
    pub settings: GmailSettings,
    /// The reads the next batch carries, adapted across the run's
    /// connections ([`super::batch::BatchSize`]).
    pub batch_size: Mutex<super::batch::BatchSize>,
    /// The batches in flight now.
    in_flight: Mutex<usize>,
    /// Signalled when a batch lands.
    landed: Condvar,
    /// The metadata of messages still owed to another of their labels.
    metas: Mutex<Bounded<Seen>>,
    /// The sizes of messages read, for their body reads.
    sizes: Mutex<Bounded<u64>>,
    /// The mailbox history read this run, projected on each label.
    history: Mutex<Option<History>>,
    /// The heads listed this run, by label id.
    heads: Mutex<HashMap<String, Headed>>,
}

/// What the heads listed on one label this run touched, which the label's
/// later pages read anew (pimdir SYNC §4).
#[derive(Clone, Debug)]
pub struct Headed {
    /// When the latest head began: a read before it is no evidence of
    /// what a head stated.
    pub at: Instant,
    /// The messages the heads read again or stated deleted.
    pub touched: HashSet<String>,
}

impl GmailRun {
    pub fn new(settings: GmailSettings) -> Self {
        Self {
            settings,
            batch_size: Mutex::new(super::batch::BatchSize::new(settings.batch_size)),
            in_flight: Mutex::new(0),
            landed: Condvar::new(),
            metas: Mutex::new(Bounded::new(METAS_CAP)),
            sizes: Mutex::new(Bounded::new(SIZES_CAP)),
            history: Mutex::new(None),
            heads: Mutex::new(HashMap::new()),
        }
    }

    /// Records a head listed on `label`, begun at `at`, which touched
    /// `touched`: the label's later pages read them again, no earlier than
    /// the latest head.
    pub fn headed(&self, label: &str, at: Instant, touched: impl IntoIterator<Item = String>) {
        let mut heads = lock(&self.heads);
        let headed = heads.entry(label.to_owned()).or_insert_with(|| Headed {
            at,
            touched: HashSet::new(),
        });
        headed.at = headed.at.max(at);
        headed.touched.extend(touched);
    }

    /// What the heads listed on `label` this run touched, `None` before
    /// any.
    pub fn head(&self, label: &str) -> Option<Headed> {
        lock(&self.heads).get(label).cloned()
    }

    /// Waits for room among the batches in flight and takes it until the
    /// pass is dropped.
    pub fn enter(&self) -> InFlight<'_> {
        let mut in_flight = self
            .in_flight
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        while *in_flight >= self.settings.batches_in_flight {
            in_flight = self
                .landed
                .wait(in_flight)
                .unwrap_or_else(PoisonError::into_inner);
        }
        *in_flight += 1;
        InFlight { run: self }
    }

    /// Keeps a message read at `at`, as `metadata` when it carries its
    /// headers, while another collection may list it.
    pub fn keep(&self, message: &GmailMessage, metadata: bool, at: Instant) {
        if let Some(size) = message.size_estimate {
            lock(&self.sizes).insert(message.id.clone(), size);
        }
        let mut metas = lock(&self.metas);
        // NOTE: labels read later do not replace the headers read before.
        if !metadata && metas.get(&message.id).is_some_and(|seen| seen.metadata) {
            return;
        }
        metas.insert(
            message.id.clone(),
            Seen {
                message: message.clone(),
                metadata,
                at,
            },
        );
    }

    /// Keeps a message's size alone, for its body read.
    pub fn keep_size(&self, id: &str, size: u64) {
        lock(&self.sizes).insert(id.to_owned(), size);
    }

    /// The message `id` as read at or after `since`, with its headers when
    /// `metadata`.
    pub fn seen(&self, id: &str, metadata: bool, since: Option<Instant>) -> Option<GmailMessage> {
        let metas = lock(&self.metas);
        let seen = metas.get(id)?;
        let fresh = since.is_none_or(|since| seen.at >= since);
        (fresh && (seen.metadata || !metadata)).then(|| seen.message.clone())
    }

    /// Takes the size of message `id`, read this run, for its body read; a
    /// message kept for its other labels keeps it for their bodies too.
    pub fn take_size(&self, id: &str) -> Option<u64> {
        lock(&self.sizes).remove(id).or_else(|| {
            let metas = lock(&self.metas);
            metas.get(id).and_then(|seen| seen.message.size_estimate)
        })
    }

    /// Runs `with` over the mailbox history since `start`, read from
    /// Gmail by `read` unless this run already read it from `start` or
    /// earlier.
    ///
    /// The lock is held while `read` runs, so the source's connections read
    /// it once.
    pub fn history<T, E>(
        &self,
        start: &str,
        read: impl FnOnce() -> Result<History, E>,
        with: impl FnOnce(&History) -> T,
    ) -> Result<T, E> {
        let mut history = lock(&self.history);
        let covered = match (history.as_ref(), start.parse::<u64>()) {
            (Some(read), Ok(start)) => read.start.is_some_and(|from| from <= start),
            _ => false,
        };
        if !covered {
            *history = Some(read()?);
        }
        let Some(history) = history.as_ref() else {
            unreachable!("the history was just read");
        };
        Ok(with(history))
    }
}

/// A batch in flight, landed when dropped.
pub struct InFlight<'a> {
    run: &'a GmailRun,
}

impl Drop for InFlight<'_> {
    fn drop(&mut self) {
        let mut in_flight = lock(&self.run.in_flight);
        *in_flight = in_flight.saturating_sub(1);
        drop(in_flight);
        self.run.landed.notify_one();
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A message read, and when.
#[derive(Debug)]
struct Seen {
    message: GmailMessage,
    /// Whether it was read with its headers rather than its labels alone.
    metadata: bool,
    at: Instant,
}

/// A map holding at most `cap` entries, the oldest inserted leaving first.
#[derive(Debug)]
struct Bounded<V> {
    cap: usize,
    entries: HashMap<String, V>,
    order: VecDeque<String>,
}

impl<V> Bounded<V> {
    fn new(cap: usize) -> Self {
        Self {
            cap,
            entries: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    fn insert(&mut self, key: String, value: V) {
        if self.entries.insert(key.clone(), value).is_none() {
            self.order.push_back(key);
        }
        while self.entries.len() > self.cap {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            self.entries.remove(&oldest);
        }
        // NOTE: keys removed leave their place in the order behind, swept
        // here once they outnumber the entries.
        if self.order.len() > 2 * self.cap.max(self.entries.len()) {
            let entries = &self.entries;
            self.order.retain(|key| entries.contains_key(key));
        }
    }

    fn get(&self, key: &str) -> Option<&V> {
        self.entries.get(key)
    }

    fn remove(&mut self, key: &str) -> Option<V> {
        self.entries.remove(key)
    }
}

/// How a history record touched a message.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Touch {
    /// It arrived.
    Added,
    /// It was deleted for good.
    Deleted,
    /// A label was added to it or removed from it.
    Labelled,
}

/// One message a history record touched, with the labels it named.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Change {
    /// The history record's id, ordered.
    pub record: Option<u64>,
    pub id: String,
    pub touch: Touch,
    /// The message's labels as the record states them, and the labels the
    /// record added or removed.
    pub labels: Vec<String>,
}

/// The mailbox history this run read, every label's.
#[derive(Debug)]
pub struct History {
    /// The history id it was read from; `None` when not a number, the
    /// history then serving no other start.
    pub start: Option<u64>,
    /// When it was read: a message read before is no evidence of what the
    /// history says it became.
    pub at: Instant,
    /// What it holds, in order.
    pub changes: Vec<Change>,
    /// The mailbox history id the next run starts from.
    pub next: String,
}

impl History {
    /// The history read from `start` at `at`, its pages' records folded
    /// into changes.
    pub fn new(start: &str, at: Instant, records: &[GmailHistory], next: String) -> Self {
        let mut changes = Vec::new();
        for record in records {
            let id = record.id.parse().ok();
            let mut push = |message: &GmailMessage, touch, changed: &[String]| {
                if message.id.is_empty() {
                    return;
                }
                let mut labels = message.label_ids.clone();
                labels.extend(changed.iter().cloned());
                changes.push(Change {
                    record: id,
                    id: message.id.clone(),
                    touch,
                    labels,
                });
            };
            for added in &record.messages_added {
                push(&added.message, Touch::Added, &[]);
            }
            for deleted in &record.messages_deleted {
                push(&deleted.message, Touch::Deleted, &[]);
            }
            for change in record.labels_added.iter().chain(&record.labels_removed) {
                push(&change.message, Touch::Labelled, &change.label_ids);
            }
        }

        Self {
            start: start.parse().ok(),
            at,
            changes,
            next,
        }
    }

    /// The changes of `label` past `start`: the ids to read again, in
    /// first-seen order, and the ids deleted for good.
    ///
    /// A message belongs to the label's changes when the record names the
    /// label, or when the collection holds it (`held`), whatever the
    /// record names; an arrival naming no label at all is read too. Reading
    /// one too many costs a read; missing one would lose its change.
    pub fn project(
        &self,
        label: &str,
        start: &str,
        held: impl Fn(&str) -> bool,
    ) -> (Vec<String>, Vec<String>) {
        let after = start.parse::<u64>().ok();
        let past = |change: &Change| match (change.record, after) {
            (Some(record), Some(after)) => record > after,
            _ => true,
        };
        let names = |change: &Change| change.labels.iter().any(|l| l == label);

        let gone: BTreeSet<&str> = self
            .changes
            .iter()
            .filter(|change| change.touch == Touch::Deleted && past(change))
            .map(|change| change.id.as_str())
            .collect();

        let mut seen = BTreeSet::new();
        let mut changed = Vec::new();
        let mut deleted = Vec::new();
        for change in self.changes.iter().filter(|change| past(change)) {
            let id = change.id.as_str();
            let ours = held(id)
                || names(change)
                || (change.touch == Touch::Added && change.labels.is_empty());
            if !ours || !seen.insert((id, change.touch == Touch::Deleted)) {
                continue;
            }
            match change.touch {
                Touch::Deleted => deleted.push(id.to_owned()),
                _ if gone.contains(id) => {}
                _ => changed.push(id.to_owned()),
            }
        }

        (changed, deleted)
    }
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        thread,
        time::Duration,
    };

    use super::*;

    fn records(json: serde_json::Value) -> Vec<GmailHistory> {
        serde_json::from_value(json).unwrap()
    }

    /// A mailbox's history: an arrival in the inbox, a star on a message
    /// filed under a label, a message moved out of the inbox, one deleted.
    fn mailbox() -> History {
        let message = |id: &str, labels: &[&str]| serde_json::json!({ "message": { "id": id, "labelIds": labels } });
        let change = |id: &str, labels: &[&str], changed: &[&str]| serde_json::json!({ "message": { "id": id, "labelIds": labels }, "labelIds": changed });
        let records = records(serde_json::json!([
            { "id": "101", "messagesAdded": [message("a", &["INBOX", "UNREAD"])] },
            { "id": "102", "labelsAdded": [change("b", &["Label_1", "STARRED"], &["STARRED"])] },
            { "id": "103", "labelsRemoved": [change("c", &["Label_1"], &["INBOX"])] },
            { "id": "104", "messagesAdded": [message("d", &["INBOX"])] },
            { "id": "105", "messagesDeleted": [message("d", &["INBOX"])] },
            { "id": "106", "messagesAdded": [message("e", &[])] },
        ]));
        History::new("100", Instant::now(), &records, String::from("106"))
    }

    #[test]
    fn a_history_projects_on_each_label_what_names_it() {
        let history = mailbox();
        let none = |_: &str| false;

        let (changed, deleted) = history.project("INBOX", "100", none);
        assert_eq!(
            changed,
            ["a", "c", "e"],
            "c left the inbox; e names no label"
        );
        assert_eq!(deleted, ["d"], "d arrived then went");

        let (changed, deleted) = history.project("Label_1", "100", none);
        assert_eq!(changed, ["b", "c", "e"]);
        assert!(deleted.is_empty());

        let (changed, _) = history.project("SENT", "100", none);
        assert_eq!(changed, ["e"]);
    }

    #[test]
    fn a_held_message_is_read_again_whatever_the_record_names() {
        let history = mailbox();
        let (changed, _) = history.project("SENT", "100", |id| id == "b");
        assert_eq!(changed, ["b", "e"]);
    }

    #[test]
    fn a_later_start_skips_the_records_it_saw() {
        let history = mailbox();
        let (changed, deleted) = history.project("INBOX", "103", |_| false);
        assert_eq!(changed, ["e"]);
        assert_eq!(deleted, ["d"]);
    }

    #[test]
    fn a_history_is_read_once_for_every_later_start() {
        let run = GmailRun::new(GmailSettings::default());
        let reads = AtomicUsize::new(0);
        let read = |start: &str| {
            reads.fetch_add(1, Ordering::Relaxed);
            Ok::<_, ()>(History::new(
                start,
                Instant::now(),
                &[],
                String::from("200"),
            ))
        };

        let next = run
            .history("150", || read("150"), |h| h.next.clone())
            .unwrap();
        assert_eq!(next, "200");
        run.history("180", || read("180"), |_| ()).unwrap();
        run.history("150", || read("150"), |_| ()).unwrap();
        assert_eq!(
            reads.load(Ordering::Relaxed),
            1,
            "one read serves later starts"
        );

        run.history("120", || read("120"), |_| ()).unwrap();
        assert_eq!(
            reads.load(Ordering::Relaxed),
            2,
            "an earlier start reads again"
        );
        run.history("150", || read("150"), |_| ()).unwrap();
        assert_eq!(reads.load(Ordering::Relaxed), 2);

        let failed = run.history("90", || Err::<History, _>("expired"), |_| ());
        assert_eq!(failed, Err("expired"));
        run.history("150", || read("150"), |_| ()).unwrap();
        assert_eq!(
            reads.load(Ordering::Relaxed),
            2,
            "a failed read keeps the last"
        );
    }

    #[test]
    fn a_message_seen_serves_a_later_read_only_as_fresh_and_as_full() {
        let run = GmailRun::new(GmailSettings::default());
        let before = Instant::now();
        let message = GmailMessage {
            id: "m1".into(),
            label_ids: vec!["INBOX".into(), "Label_1".into()],
            size_estimate: Some(2048),
            ..Default::default()
        };
        run.keep(&message, false, Instant::now());

        assert!(run.seen("m1", false, None).is_some());
        assert!(
            run.seen("m1", true, None).is_none(),
            "labels alone name nothing"
        );
        assert!(run.seen("m1", false, Some(before)).is_some());
        assert!(
            run.seen("m1", false, Some(Instant::now() + Duration::from_secs(1)))
                .is_none(),
            "read before the history"
        );
        assert_eq!(run.take_size("m1"), Some(2048));
        assert_eq!(run.take_size("m1"), Some(2048), "kept for its other label");
        run.keep_size("m2", 512);
        assert_eq!(run.take_size("m2"), Some(512));
        assert_eq!(run.take_size("m2"), None, "taken once");
    }

    #[test]
    fn a_cache_holds_its_newest_entries() {
        let mut bounded = Bounded::new(3);
        for n in 0..10 {
            bounded.insert(format!("m{n}"), n);
        }
        assert_eq!(bounded.entries.len(), 3);
        assert!(bounded.get("m6").is_none());
        assert_eq!(bounded.get("m9"), Some(&9));

        for n in 0..100 {
            bounded.insert(format!("k{n}"), n);
            bounded.remove(&format!("k{n}"));
        }
        assert!(bounded.order.len() <= 6, "{}", bounded.order.len());
    }

    #[test]
    fn one_batch_is_in_flight_at_a_time() {
        let run = Arc::new(GmailRun::new(GmailSettings::default()));
        let now = Arc::new(AtomicUsize::new(0));
        let most = Arc::new(AtomicUsize::new(0));
        let workers: Vec<_> = (0..4)
            .map(|_| {
                let (run, now, most) = (run.clone(), now.clone(), most.clone());
                thread::spawn(move || {
                    for _ in 0..5 {
                        let _pass = run.enter();
                        let inside = now.fetch_add(1, Ordering::SeqCst) + 1;
                        most.fetch_max(inside, Ordering::SeqCst);
                        thread::sleep(Duration::from_millis(2));
                        now.fetch_sub(1, Ordering::SeqCst);
                    }
                })
            })
            .collect();
        for worker in workers {
            worker.join().unwrap();
        }
        assert_eq!(most.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn settings_default_and_refuse_what_cannot_run() {
        assert_eq!(
            GmailSettings::new(None, None, None).unwrap(),
            GmailSettings::default()
        );
        assert_eq!(GmailSettings::default().batch_size, 10);
        assert!(GmailSettings::new(Some(0), None, None).is_err());
        assert!(GmailSettings::new(Some(101), None, None).is_err());
        assert!(GmailSettings::new(None, Some(0), None).is_err());
        assert!(GmailSettings::new(Some(10), None, Some(49)).is_err());
        assert!(GmailSettings::new(Some(100), Some(2), Some(500)).is_ok());
    }
}
