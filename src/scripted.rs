//! # Scripted server
//!
//! A scripted HTTP server for the backend tests: each connection answers
//! its requests from its own script, in order, and records what was written
//! to it; once a script runs dry, the server's responder answers, when one
//! is set, else the stream ends as a server closing the connection.

#![cfg_attr(
    not(all(
        feature = "msgraph",
        feature = "gmail",
        feature = "gcal",
        feature = "gpeople",
        feature = "carddav",
        feature = "caldav"
    )),
    allow(dead_code)
)]

use std::{
    collections::VecDeque,
    io::{self, ErrorKind, Read, Write},
    sync::{Arc, Mutex},
};

/// What answers a request once its connection's script runs dry: its
/// request line in, the answer out.
type Responder = Box<dyn FnMut(&str) -> Vec<u8> + Send>;

/// What a connection does when a request asks for its answer.
#[derive(Clone, Debug)]
pub enum Step {
    /// Answers these octets.
    Reply(Vec<u8>),
    /// Ends the stream before any octet, as a connection the server
    /// closed while it idled.
    Close,
    /// Fails the read with a reset.
    Reset,
    /// Fails the read as one that waited too long.
    Timeout,
    /// Fails the write of the request with a broken pipe.
    Broken,
}

impl Step {
    /// A JSON answer of `status`.
    pub fn json(status: u16, body: &serde_json::Value) -> Self {
        let body = serde_json::to_vec(body).unwrap();
        let mut reply = format!(
            "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n",
            body.len()
        )
        .into_bytes();
        reply.extend_from_slice(&body);
        Self::Reply(reply)
    }

    /// A 200 answering `body` as JSON.
    pub fn ok(body: serde_json::Value) -> Self {
        Self::json(200, &body)
    }

    /// An error envelope of `status` and `code`, as Graph answers one.
    pub fn error(status: u16, code: &str) -> Self {
        Self::json(
            status,
            &serde_json::json!({ "error": { "code": code, "message": code } }),
        )
    }
}

/// An answer of `status`, carrying `headers` and no body.
pub fn reply(status: &str, headers: &[(&str, &str)]) -> String {
    answer(status, headers, "")
}

/// An answer of `status`, carrying `headers` and `body`.
pub fn answer(status: &str, headers: &[(&str, &str)], body: &str) -> String {
    let headers: String = headers
        .iter()
        .map(|(name, value)| format!("{name}: {value}\r\n"))
        .collect();
    format!(
        "HTTP/1.1 {status}\r\n{headers}Content-Length: {}\r\n\r\n{body}",
        body.len()
    )
}

/// A JSON answer of `status` (its code and reason) carrying `body`.
pub fn json(status: &str, body: &str) -> Vec<u8> {
    format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    )
    .into_bytes()
}

/// A 200 answering `body` as JSON.
pub fn ok(body: serde_json::Value) -> Vec<u8> {
    json("200 OK", &body.to_string())
}

#[derive(Default)]
struct Connection {
    script: VecDeque<Step>,
    written: Vec<u8>,
    requests: Vec<Vec<u8>>,
    out: VecDeque<u8>,
}

/// The server: the scripts of the connections still to open, every
/// connection opened, and the responder.
#[derive(Clone, Default)]
pub struct Server {
    pending: Arc<Mutex<VecDeque<Vec<Step>>>>,
    opened: Arc<Mutex<Vec<Arc<Mutex<Connection>>>>>,
    responder: Arc<Mutex<Option<Responder>>>,
}

impl Server {
    /// A server whose connections answer `scripts`, one each, in order.
    pub fn new(scripts: Vec<Vec<Step>>) -> Self {
        let server = Self::default();
        server.pending.lock().unwrap().extend(scripts);
        server
    }

    /// A server of one connection answering `replies` in order.
    pub fn replying<R: Into<Vec<u8>>>(replies: impl IntoIterator<Item = R>) -> Self {
        let script = replies
            .into_iter()
            .map(|reply| Step::Reply(reply.into()))
            .collect();
        Self::new(vec![script])
    }

    /// Opens the next scripted connection; refused when none is left.
    pub fn open(&self) -> io::Result<Stream> {
        let script =
            self.pending.lock().unwrap().pop_front().ok_or_else(|| {
                io::Error::new(ErrorKind::ConnectionRefused, "no connection left")
            })?;
        let connection = Arc::new(Mutex::new(Connection {
            script: script.into(),
            ..Default::default()
        }));
        self.opened.lock().unwrap().push(Arc::clone(&connection));
        Ok(Stream {
            connection,
            responder: Arc::clone(&self.responder),
        })
    }

    /// Adds `reply` to the script of the connection opened last, or of the
    /// next one when none is open yet.
    pub fn answer(&self, reply: impl Into<Vec<u8>>) {
        let step = Step::Reply(reply.into());
        if let Some(connection) = self.opened.lock().unwrap().last() {
            connection.lock().unwrap().script.push_back(step);
            return;
        }
        let mut pending = self.pending.lock().unwrap();
        match pending.front_mut() {
            Some(script) => script.push(step),
            None => pending.push_back(vec![step]),
        }
    }

    /// Answers every request past the scripts by `responder`, given the
    /// request's line.
    pub fn respond(&self, responder: impl FnMut(&str) -> Vec<u8> + Send + 'static) {
        *self.responder.lock().unwrap() = Some(Box::new(responder));
    }

    /// How many steps are left to play, on every connection.
    pub fn left(&self) -> usize {
        let opened: usize = self
            .opened
            .lock()
            .unwrap()
            .iter()
            .map(|connection| connection.lock().unwrap().script.len())
            .sum();
        let pending: usize = self.pending.lock().unwrap().iter().map(Vec::len).sum();
        opened + pending
    }

    /// Everything written to the connection opened `index`th.
    pub fn written(&self, index: usize) -> String {
        let opened = self.opened.lock().unwrap();
        let written = opened[index].lock().unwrap().written.clone();
        String::from_utf8_lossy(&written).into_owned()
    }

    /// The request lines each opened connection was sent, in order, each
    /// its method and target.
    pub fn requests(&self) -> Vec<Vec<String>> {
        self.opened
            .lock()
            .unwrap()
            .iter()
            .map(|connection| {
                let connection = connection.lock().unwrap();
                connection.requests.iter().map(|r| line(r)).collect()
            })
            .collect()
    }

    /// The request lines of every connection, in order.
    pub fn lines(&self) -> Vec<String> {
        self.requests().into_iter().flatten().collect()
    }

    /// Every request of every connection, head and body, in order.
    pub fn sent(&self) -> Vec<String> {
        self.opened
            .lock()
            .unwrap()
            .iter()
            .flat_map(|connection| {
                let connection = connection.lock().unwrap();
                connection
                    .requests
                    .iter()
                    .map(|request| String::from_utf8_lossy(request).into_owned())
                    .collect::<Vec<_>>()
            })
            .collect()
    }
}

/// The method and target of `request`, from its request line.
fn line(request: &[u8]) -> String {
    let request = String::from_utf8_lossy(request);
    let line = request.split("\r\n").next().unwrap_or_default();
    line.trim_end_matches(" HTTP/1.1").to_owned()
}

/// Whether `chunk` begins a request: its first line is a request line.
fn begins_request(chunk: &[u8]) -> bool {
    let end = chunk
        .windows(2)
        .position(|pair| pair == b"\r\n")
        .unwrap_or(chunk.len());
    chunk[..end].ends_with(b" HTTP/1.1")
}

/// The client's side of one scripted connection.
pub struct Stream {
    connection: Arc<Mutex<Connection>>,
    responder: Arc<Mutex<Option<Responder>>>,
}

impl Read for Stream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut connection = self.connection.lock().unwrap();
        if connection.out.is_empty() {
            match connection.script.pop_front() {
                Some(Step::Reply(reply)) => connection.out.extend(reply),
                Some(Step::Close) | Some(Step::Broken) => return Ok(0),
                Some(Step::Reset) => {
                    return Err(io::Error::new(ErrorKind::ConnectionReset, "reset"));
                }
                Some(Step::Timeout) => {
                    return Err(io::Error::new(ErrorKind::WouldBlock, "timed out"));
                }
                None => {
                    let mut responder = self.responder.lock().unwrap();
                    let Some(responder) = responder.as_mut() else {
                        return Ok(0);
                    };
                    let request = connection.requests.last().map(|r| line(r));
                    let reply = responder(request.as_deref().unwrap_or_default());
                    connection.out.extend(reply);
                }
            }
        }
        let n = buf.len().min(connection.out.len());
        for (slot, byte) in buf.iter_mut().zip(connection.out.drain(..n)) {
            *slot = byte;
        }
        Ok(n)
    }
}

impl Write for Stream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut connection = self.connection.lock().unwrap();
        if connection.out.is_empty() && matches!(connection.script.front(), Some(Step::Broken)) {
            connection.script.pop_front();
            return Err(io::Error::new(ErrorKind::BrokenPipe, "broken pipe"));
        }
        connection.written.extend_from_slice(buf);
        match connection.requests.last_mut() {
            Some(request) if !begins_request(buf) => request.extend_from_slice(buf),
            _ => connection.requests.push(buf.to_vec()),
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
