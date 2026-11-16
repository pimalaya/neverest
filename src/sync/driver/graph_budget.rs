//! # Graph connection budget
//!
//! Several Microsoft Graph connections of one source against a fake Graph
//! answering on every connection at once: the run's budget is what runs
//! side by side, and one throttle stops them all.

use std::{
    collections::VecDeque,
    io::{self, Read, Write},
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant, SystemTime},
};

use base64::{Engine, engine::general_purpose::STANDARD};
use crossbeam_queue::SegQueue;
use io_msgraph::v1::client::MsgraphStream;
use io_pimdir::{client::PimdirStore, placement::PimdirHandle};

use super::{HydrateBatch, connection_budget, hydrate_pool};
use crate::{
    client::Client,
    config::AccountConfig,
    kind::Kind,
    msgraph::client::{GraphClient, SizeHints},
    sync::remote::hydrate_batch,
    throttle::Throttle,
};

/// How long the fake serves a request: long enough for the
/// connections' requests to overlap.
const SERVE: Duration = Duration::from_millis(30);

/// The fake Graph: every request it served, and how many at once.
#[derive(Default)]
struct Graph {
    /// The message whose get is throttled, and the wait stated, in
    /// seconds, ten minutes by default.
    hot: Option<String>,
    retry_after: Option<&'static str>,
    connections: AtomicUsize,
    serving: AtomicUsize,
    most: AtomicUsize,
    /// Each request: its connection, when it reached the server, and
    /// the ids it asked.
    served: Mutex<Vec<(usize, Instant, Vec<String>)>>,
    /// When the throttled answer left.
    throttled: Mutex<Option<Instant>>,
    /// The source's throttle, when a test watches it: a request
    /// reaching the server after the throttled answer left is answered
    /// only once the source gave up, so what a connection does after
    /// the give-up does not hang on how the threads interleave.
    gate: OnceLock<Arc<Throttle>>,
}

fn raw(id: &str) -> Vec<u8> {
    format!("Message-ID: <{id}@example.org>\r\nSubject: {id}\r\n\r\nbody of {id}\r\n").into_bytes()
}

fn reply(status: u16, body: &[u8], content_type: &str) -> Vec<u8> {
    let mut out = format!(
        "HTTP/1.1 {status} X\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n\r\n",
        body.len()
    )
    .into_bytes();
    out.extend_from_slice(body);
    out
}

impl Graph {
    /// Answers one request, `line` its request line.
    fn serve(&self, connection: usize, line: &str, body: &[u8]) -> Vec<u8> {
        // NOTE: read under the lock the throttled answer is noted
        // under, so a request is after it exactly when it reached the
        // server once that answer had left.
        let (at, late) = {
            let throttled = self.throttled.lock().unwrap();
            (Instant::now(), throttled.is_some())
        };
        if let Some(throttle) = self.gate.get().filter(|_| late) {
            let deadline = at + Duration::from_secs(10);
            while throttle.gave_up().is_none() && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(1));
            }
        }

        let now = self.serving.fetch_add(1, Ordering::SeqCst) + 1;
        self.most.fetch_max(now, Ordering::SeqCst);
        thread::sleep(SERVE);

        let id_of = |url: &str| {
            url.split("messages/")
                .nth(1)
                .and_then(|rest| rest.split('/').next())
                .unwrap_or_default()
                .to_owned()
        };
        let (asked, answer) = if line.starts_with("POST ") && line.contains("$batch") {
            let batch: serde_json::Value = serde_json::from_slice(body).unwrap();
            let mut asked = Vec::new();
            let mut responses = Vec::new();
            for request in batch["requests"].as_array().unwrap() {
                let id = id_of(request["url"].as_str().unwrap());
                let rid = request["id"].clone();
                if self.hot.as_deref() == Some(id.as_str()) {
                    *self.throttled.lock().unwrap() = Some(Instant::now());
                    responses.push(serde_json::json!({
                        "id": rid,
                        "status": 429,
                        "headers": { "retry-after": self.retry_after.unwrap_or("600") },
                        "body": { "error": { "code": "TooManyRequests", "message": "slow" } },
                    }));
                } else {
                    responses.push(serde_json::json!({
                        "id": rid,
                        "status": 200,
                        "body": STANDARD.encode(raw(&id)),
                    }));
                }
                asked.push(id);
            }
            let json = serde_json::json!({ "responses": responses });
            let json = serde_json::to_vec(&json).unwrap();
            (asked, reply(200, &json, "application/json"))
        } else {
            let id = id_of(line);
            (vec![id.clone()], reply(200, &raw(&id), "text/plain"))
        };

        self.served.lock().unwrap().push((connection, at, asked));
        self.serving.fetch_sub(1, Ordering::SeqCst);
        answer
    }

    /// Opens a connection to the fake.
    fn connect(self: &Arc<Self>) -> anyhow::Result<Box<dyn MsgraphStream>> {
        let index = self.connections.fetch_add(1, Ordering::SeqCst);
        Ok(Box::new(Wire {
            graph: Arc::clone(self),
            index,
            input: Vec::new(),
            out: VecDeque::new(),
        }))
    }
}

/// One connection: what the client wrote, served once it reads.
struct Wire {
    graph: Arc<Graph>,
    index: usize,
    input: Vec<u8>,
    out: VecDeque<u8>,
}

impl Wire {
    /// Serves every whole request written so far.
    fn serve(&mut self) {
        while let Some(end) = self.input.windows(4).position(|w| w == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&self.input[..end]).into_owned();
            let length: usize = head
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse().ok())?
                })
                .unwrap_or(0);
            let start = end + 4;
            if self.input.len() < start + length {
                return;
            }
            let body: Vec<u8> = self.input[start..start + length].to_vec();
            self.input.drain(..start + length);
            let line = head.lines().next().unwrap_or_default().to_owned();
            let answer = self.graph.serve(self.index, &line, &body);
            self.out.extend(answer);
        }
    }
}

impl Read for Wire {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.out.is_empty() {
            self.serve();
        }
        let n = buf.len().min(self.out.len());
        for (slot, byte) in buf.iter_mut().zip(self.out.drain(..n)) {
            *slot = byte;
        }
        Ok(n)
    }
}

impl Write for Wire {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.input.extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// The budget a Graph mail source takes in a run of four connections,
/// its clients over the fake, sharing one throttle and the sizes the
/// listing stated for `ids`.
fn clients(graph: &Arc<Graph>, ids: &[String]) -> (Vec<Client>, Arc<Throttle>) {
    clients_waiting(graph, ids, crate::throttle::MAX_WAIT)
}

fn clients_waiting(
    graph: &Arc<Graph>,
    ids: &[String],
    max_wait: Duration,
) -> (Vec<Client>, Arc<Throttle>) {
    let config: AccountConfig =
        toml::from_str("msgraph.auth.token.raw = \"x\"\nmsgraph.mail = true").unwrap();
    let budget = connection_budget(&config.sources().unwrap()["msgraph"], 4);
    let throttle = Arc::new(Throttle::new("msgraph").waiting(max_wait));
    let sizes = SizeHints::default();
    sizes.noted(ids.iter().map(|id| (id.clone(), 1_000)));
    let clients = (0..budget)
        .map(|_| {
            let graph = Arc::clone(graph);
            let client = GraphClient::over_streams(
                move || graph.connect(),
                sizes.clone(),
                Arc::clone(&throttle),
            )
            .unwrap();
            Client::Msgraph(Box::new(client))
        })
        .collect();
    (clients, throttle)
}

fn ids(count: usize) -> Vec<String> {
    (0..count).map(|n| format!("m{n:03}")).collect()
}

/// Batches of `size` handles over `ids`.
fn batches(ids: &[String], size: usize) -> Vec<HydrateBatch> {
    ids.chunks(size)
        .map(|chunk| {
            let bodies = chunk
                .iter()
                .map(|id| (PimdirHandle(id.clone()), 0))
                .collect();
            HydrateBatch {
                collection: String::from("Inbox"),
                bodies,
            }
        })
        .collect()
}

/// The handles of `batch`.
fn handles_of_batch(batch: &HydrateBatch) -> Vec<PimdirHandle> {
    batch
        .bodies
        .iter()
        .map(|(handle, _)| handle.clone())
        .collect()
}

/// A Graph source downloads over its three connections side by side,
/// never more, through phase 2's pool, each body asked once.
#[test]
fn a_graph_source_runs_its_budget_side_by_side_and_no_more() {
    let graph = Arc::new(Graph::default());
    let ids = ids(60);
    let (mut clients, _) = clients(&graph, &ids);
    assert_eq!(clients.len(), 3, "Graph's default budget");
    let dir = tempfile::tempdir().unwrap();
    let blobs = PimdirStore::open(dir.path()).unwrap().blob_stage();
    let fetched = AtomicUsize::new(0);

    hydrate_pool(&mut clients, batches(&ids, 5), |client, batch| {
        let handles = handles_of_batch(&batch);
        let hydrated = hydrate_batch(
            Kind::Mail,
            client,
            &batch.collection,
            &handles,
            &|_| None,
            &blobs,
            None,
        )?;
        fetched.fetch_add(hydrated.items.len(), Ordering::Relaxed);
        Ok(())
    })
    .unwrap();

    assert_eq!(fetched.into_inner(), 60);
    assert_eq!(graph.connections.load(Ordering::SeqCst), 3);
    assert_eq!(
        graph.most.load(Ordering::SeqCst),
        3,
        "three at once, never four"
    );
    let served = graph.served.lock().unwrap();
    let mut asked: Vec<&String> = served.iter().flat_map(|(_, _, ids)| ids).collect();
    asked.sort();
    asked.dedup();
    assert_eq!(asked.len(), 60, "each body asked once");
    assert_eq!(
        served.iter().map(|(_, _, ids)| ids.len()).sum::<usize>(),
        60
    );
    for connection in 0..3 {
        assert!(
            served.iter().any(|(c, _, _)| *c == connection),
            "connection {connection} served"
        );
    }
}

/// One connection's batch throttled for ten minutes gives the source up,
/// and no connection sends anything after: those with batches left
/// fail them at once, as phase 1's idle connections go on to the next.
///
/// A connection that read the throttle before the source gave up may
/// have one request on its way when the throttled answer leaves; the
/// fake answers such a request only once the source gave up, so a
/// connection sending a second one would have sent it after the give-up.
#[test]
fn a_throttle_on_one_connection_stops_every_connection() {
    let graph = Arc::new(Graph {
        hot: Some(String::from("m012")),
        ..Default::default()
    });
    let ids = ids(60);
    let (mut clients, throttle) = clients(&graph, &ids);
    assert!(graph.gate.set(Arc::clone(&throttle)).is_ok());
    let dir = tempfile::tempdir().unwrap();
    let blobs = PimdirStore::open(dir.path()).unwrap().blob_stage();
    let queue = SegQueue::new();
    for batch in batches(&ids, 5) {
        queue.push(batch);
    }
    let failed = AtomicUsize::new(0);

    thread::scope(|scope| {
        for client in clients.iter_mut() {
            let (queue, blobs, failed) = (&queue, &blobs, &failed);
            scope.spawn(move || {
                while let Some(batch) = queue.pop() {
                    let handles = handles_of_batch(&batch);
                    let collection = &batch.collection;
                    if hydrate_batch(
                        Kind::Mail,
                        client,
                        collection,
                        &handles,
                        &|_| None,
                        blobs,
                        None,
                    )
                    .is_err()
                    {
                        failed.fetch_add(1, Ordering::Relaxed);
                    }
                }
            });
        }
    });

    let until = throttle.gave_up().expect("the source gave up");
    assert!(until > SystemTime::now() + Duration::from_secs(500));
    let throttled = graph.throttled.lock().unwrap().expect("a throttled answer");
    let served = graph.served.lock().unwrap();
    let after: Vec<_> = served.iter().filter(|(_, at, _)| *at > throttled).collect();
    for connection in 0..graph.connections.load(Ordering::SeqCst) {
        assert!(
            after.iter().filter(|(c, _, _)| *c == connection).count() <= 1,
            "connection {connection} sent again after the give-up: {after:?}"
        );
    }
    assert!(
        served.len() < 12,
        "the batches left were not sent: {} of 12",
        served.len()
    );
    assert!(failed.into_inner() > 0, "the batches left failed at once");
}

/// `--max-wait` bounds a batch's own waits: a get Graph asks to wait
/// three seconds, in a run waiting at most one, gives the source up
/// rather than being sent again.
#[test]
fn the_run_s_longest_wait_bounds_a_graph_batch() {
    let graph = Arc::new(Graph {
        hot: Some(String::from("m001")),
        retry_after: Some("3"),
        ..Default::default()
    });
    let ids = ids(5);
    let (mut clients, throttle) = clients_waiting(&graph, &ids, Duration::from_secs(1));
    let dir = tempfile::tempdir().unwrap();
    let blobs = PimdirStore::open(dir.path()).unwrap().blob_stage();

    let start = Instant::now();
    let handles: Vec<PimdirHandle> = ids.iter().map(|id| PimdirHandle(id.clone())).collect();
    let _ = hydrate_batch(
        Kind::Mail,
        &mut clients[0],
        "Inbox",
        &handles,
        &|_| None,
        &blobs,
        None,
    );
    assert!(
        start.elapsed() < Duration::from_secs(2),
        "no three-second wait"
    );
    assert!(throttle.gave_up().is_some(), "the source gave up");
    assert_eq!(graph.served.lock().unwrap().len(), 1, "never sent again");
}
