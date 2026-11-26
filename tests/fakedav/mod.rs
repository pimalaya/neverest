//! # Fake CardDAV server
//!
//! One address book at `/dav/book/` on a local socket, for the pair tests
//! that run the binary: enough of RFC 6352 and RFC 6578 for a sync, with
//! faults a test switches on. Every request line it reads is recorded.

#![allow(dead_code)]

use std::{
    collections::{BTreeMap, hash_map::DefaultHasher},
    hash::{Hash, Hasher},
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    sync::{Arc, Mutex},
    thread,
};

/// The sync token of the spare book, which never changes.
const SPARE_TOKEN: &str = "http://fake/spare/1";

/// What the fake answers differently from a sound server.
#[derive(Clone, Debug, Default)]
pub struct Faults {
    /// The status every `sync-collection` REPORT answers, when set.
    pub sync_status: Option<&'static str>,
    /// How many members a listing names before a 507 row cuts it, with no
    /// sync token, when set.
    pub truncate: Option<usize>,
    /// Whether the book refuses `sync-collection`, so it is listed by
    /// PROPFIND instead.
    pub no_sync: bool,
    /// The cards a listing names with a 403 row instead of their ETag, every
    /// `sync-collection` REPORT then listing every card.
    pub refused: Vec<&'static str>,
    /// The cards of another book, `/dav/other/`, a `sync-collection` delta
    /// lists as removed, as Stalwart leaks a sibling book's tombstones.
    pub foreign_removed: Vec<&'static str>,
    /// The cards of `/dav/other/` a `sync-collection` delta lists as
    /// changed.
    pub foreign_changed: Vec<&'static str>,
}

#[derive(Default)]
struct State {
    /// The book's `DAV:displayname` a `PROPPATCH` set, `Book` until then.
    name: Option<String>,
    cards: BTreeMap<String, String>,
    removed: Vec<String>,
    faults: Faults,
    requests: Vec<String>,
    /// Whether another client deleted the book, which is listed no more.
    deleted: bool,
    /// Whether the home set also lists an empty book at `/dav/spare/`.
    spare: bool,
}

/// A fake server running in the background, for as long as the test.
pub struct FakeDav {
    /// The home set URL, `http://127.0.0.1:<port>/dav/`.
    pub home: String,
    state: Arc<Mutex<State>>,
}

impl FakeDav {
    /// Starts a server holding no card.
    pub fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let state = Arc::new(Mutex::new(State::default()));

        let shared = state.clone();
        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let state = shared.clone();
                thread::spawn(move || serve(stream, &state));
            }
        });

        Self {
            home: format!("http://127.0.0.1:{port}/dav/"),
            state,
        }
    }

    /// Stores `body` under `name`, as another client would.
    pub fn put(&self, name: &str, body: &str) {
        let mut state = self.state.lock().unwrap();
        state.cards.insert(name.to_string(), body.to_string());
        state.removed.retain(|removed| removed != name);
    }

    /// Removes the card `name`, as another client would.
    pub fn remove(&self, name: &str) {
        let mut state = self.state.lock().unwrap();
        state.cards.remove(name);
        state.removed.push(name.to_string());
    }

    /// The names of the cards the book holds.
    pub fn names(&self) -> Vec<String> {
        self.state.lock().unwrap().cards.keys().cloned().collect()
    }

    /// Sets the book's `DAV:displayname`, as another client would.
    pub fn set_name(&self, name: &str) {
        self.state.lock().unwrap().name = Some(name.to_string());
    }

    /// Lists an empty book at `/dav/spare/` beside the book.
    pub fn add_spare(&self) {
        self.state.lock().unwrap().spare = true;
    }

    /// Deletes the book with its cards, as another client would.
    pub fn delete_book(&self) {
        let mut state = self.state.lock().unwrap();
        state.deleted = true;
        state.cards.clear();
    }

    /// Replaces the faults the server answers with.
    pub fn set_faults(&self, faults: Faults) {
        self.state.lock().unwrap().faults = faults;
    }

    /// The requests read with `method`, as `<method> <path>`.
    pub fn requests(&self, method: &str) -> Vec<String> {
        let prefix = format!("{method} ");
        let state = self.state.lock().unwrap();
        state
            .requests
            .iter()
            .filter(|line| line.starts_with(&prefix))
            .cloned()
            .collect()
    }
}

fn serve(stream: TcpStream, state: &Mutex<State>) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut writer = stream;

    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 {
            return;
        }
        let mut parts = line.split_whitespace();
        let method = parts.next().unwrap_or_default().to_string();
        let path = parts.next().unwrap_or_default().to_string();

        let mut length = 0;
        loop {
            let mut header = String::new();
            if reader.read_line(&mut header).unwrap_or(0) == 0 {
                return;
            }
            if header == "\r\n" {
                break;
            }
            let lower = header.to_ascii_lowercase();
            if let Some(value) = lower.strip_prefix("content-length:") {
                length = value.trim().parse().unwrap_or(0);
            }
        }
        let mut body = vec![0; length];
        if reader.read_exact(&mut body).is_err() {
            return;
        }
        let body = String::from_utf8_lossy(&body).into_owned();

        let (status, headers, out) = {
            let mut state = state.lock().unwrap();
            state.requests.push(format!("{method} {path}"));
            answer(&mut state, &method, &path, &body)
        };

        let mut response = format!("HTTP/1.1 {status}\r\n");
        for header in headers {
            response.push_str(&header);
            response.push_str("\r\n");
        }
        response.push_str(&format!("Content-Length: {}\r\n\r\n", out.len()));
        response.push_str(&out);
        if writer.write_all(response.as_bytes()).is_err() {
            return;
        }
    }
}

fn answer(
    state: &mut State,
    method: &str,
    path: &str,
    body: &str,
) -> (String, Vec<String>, String) {
    let xml = vec![String::from("Content-Type: application/xml; charset=utf-8")];
    let card = path
        .strip_prefix("/dav/book/")
        .filter(|name| !name.is_empty());

    match (method, path, card) {
        ("PROPFIND", "/dav/", _) => {
            let token = token(state);
            let reports = match state.faults.no_sync {
                true => "",
                false => {
                    "<d:supported-report><d:report><d:sync-collection/></d:report></d:supported-report>"
                }
            };
            let name = state.name.as_deref().unwrap_or("Book");
            let book = |href: &str, name: &str, token: &str| {
                format!(
                    "<d:response><d:href>{href}</d:href><d:propstat><d:prop>\
                     <d:resourcetype><d:collection/><c:addressbook/></d:resourcetype>\
                     <d:displayname>{name}</d:displayname>\
                     <d:sync-token>{token}</d:sync-token><d:supported-report-set>\
                     <d:supported-report><d:report><c:addressbook-multiget/></d:report></d:supported-report>\
                     {reports}</d:supported-report-set></d:prop>\
                     <d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>"
                )
            };
            let mut rows = String::from(
                "<d:response><d:href>/dav/</d:href><d:propstat><d:prop>\
                 <d:resourcetype><d:collection/></d:resourcetype></d:prop>\
                 <d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>",
            );
            if !state.deleted {
                rows.push_str(&book("/dav/book/", name, &token));
            }
            if state.spare {
                rows.push_str(&book("/dav/spare/", "Spare", SPARE_TOKEN));
            }
            ("207 Multi-Status".into(), xml, multistatus(&rows))
        }
        ("PROPFIND", "/dav/spare/", _) if state.spare => {
            let rows = "<d:response><d:href>/dav/spare/</d:href><d:propstat><d:prop>\
                        <d:resourcetype><d:collection/><c:addressbook/></d:resourcetype></d:prop>\
                        <d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>";
            ("207 Multi-Status".into(), xml, multistatus(rows))
        }
        ("REPORT", "/dav/spare/", _) if state.spare => {
            let rows = format!("<d:sync-token>{SPARE_TOKEN}</d:sync-token>");
            ("207 Multi-Status".into(), xml, multistatus(&rows))
        }
        ("PROPFIND", "/dav/book/", _) => {
            let mut rows = String::from(
                "<d:response><d:href>/dav/book/</d:href><d:propstat><d:prop>\
                 <d:resourcetype><d:collection/><c:addressbook/></d:resourcetype></d:prop>\
                 <d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>",
            );
            rows.push_str(&members(state));
            ("207 Multi-Status".into(), xml, multistatus(&rows))
        }
        ("REPORT", "/dav/book/", _) if body.contains("sync-collection") => {
            if state.faults.no_sync {
                return (
                    "403 Forbidden".into(),
                    xml,
                    String::from("<d:error xmlns:d=\"DAV:\"><d:supported-report/></d:error>"),
                );
            }
            if let Some(status) = state.faults.sync_status {
                return (status.into(), Vec::new(), String::from("fake failure"));
            }
            let token = token(state);
            let asked = between(body, "sync-token>", "<").unwrap_or_default();
            let listed = state.faults.truncate.is_some() || !state.faults.refused.is_empty();
            let mut rows = match asked == token && !listed {
                true => String::new(),
                false => members(state),
            };
            if !asked.is_empty() && asked != token {
                for name in &state.removed {
                    rows.push_str(&format!(
                        "<d:response><d:href>/dav/book/{name}</d:href>\
                         <d:status>HTTP/1.1 404 Not Found</d:status></d:response>"
                    ));
                }
                for name in &state.faults.foreign_removed {
                    rows.push_str(&format!(
                        "<d:response><d:href>/dav/other/{name}</d:href>\
                         <d:status>HTTP/1.1 404 Not Found</d:status></d:response>"
                    ));
                }
                for name in &state.faults.foreign_changed {
                    rows.push_str(&format!(
                        "<d:response><d:href>/dav/other/{name}</d:href><d:propstat><d:prop>\
                         <d:getetag>\"other\"</d:getetag></d:prop>\
                         <d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>"
                    ));
                }
            }
            match state.faults.truncate {
                Some(_) => {}
                None => rows.push_str(&format!("<d:sync-token>{token}</d:sync-token>")),
            }
            ("207 Multi-Status".into(), xml, multistatus(&rows))
        }
        ("REPORT", "/dav/book/", _) => {
            let mut rows = String::new();
            let mut rest = body;
            while let Some(start) = rest.find("href>") {
                let tail = &rest[start + 5..];
                let end = tail.find('<').unwrap_or(tail.len());
                let href = &tail[..end];
                rest = &tail[end..];
                let name = href.rsplit('/').next().unwrap_or_default();
                match state.cards.get(name) {
                    Some(data) => rows.push_str(&format!(
                        "<d:response><d:href>{href}</d:href><d:propstat><d:prop>\
                         <d:getetag>\"{etag}\"</d:getetag><c:address-data>{data}</c:address-data>\
                         </d:prop><d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>",
                        etag = etag(data),
                        data = escape(data),
                    )),
                    None => rows.push_str(&format!(
                        "<d:response><d:href>{href}</d:href>\
                         <d:status>HTTP/1.1 404 Not Found</d:status></d:response>"
                    )),
                }
            }
            ("207 Multi-Status".into(), xml, multistatus(&rows))
        }
        ("GET", _, Some(name)) => match state.cards.get(name) {
            Some(data) => (
                "200 OK".into(),
                vec![
                    format!("ETag: \"{}\"", etag(data)),
                    String::from("Content-Type: text/vcard"),
                ],
                data.clone(),
            ),
            None => ("404 Not Found".into(), Vec::new(), String::from("gone")),
        },
        ("PUT", _, Some(name)) => {
            let created = !state.cards.contains_key(name);
            state.cards.insert(name.to_string(), body.to_string());
            let status = match created {
                true => "201 Created",
                false => "204 No Content",
            };
            (
                status.into(),
                vec![format!("ETag: \"{}\"", etag(body))],
                String::new(),
            )
        }
        ("PROPPATCH", "/dav/book/", None) => {
            state.name = between(body, "displayname>", "<").map(str::to_owned);
            let rows = "<d:response><d:href>/dav/book/</d:href><d:propstat><d:prop>\
                        <d:displayname/></d:prop>\
                        <d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>";
            ("207 Multi-Status".into(), xml, multistatus(rows))
        }
        ("DELETE", "/dav/book/", None) => {
            state.cards.clear();
            ("204 No Content".into(), Vec::new(), String::new())
        }
        ("DELETE", _, Some(name)) => match state.cards.remove(name) {
            Some(_) => ("204 No Content".into(), Vec::new(), String::new()),
            None => ("404 Not Found".into(), Vec::new(), String::from("gone")),
        },
        _ => ("404 Not Found".into(), Vec::new(), String::from("unknown")),
    }
}

fn members(state: &State) -> String {
    let take = state.faults.truncate.unwrap_or(usize::MAX);
    let mut rows: String = state
        .cards
        .iter()
        .take(take)
        .map(
            |(name, data)| match state.faults.refused.contains(&name.as_str()) {
                true => format!(
                    "<d:response><d:href>/dav/book/{name}</d:href>\
                 <d:status>HTTP/1.1 403 Forbidden</d:status></d:response>"
                ),
                false => format!(
                    "<d:response><d:href>/dav/book/{name}</d:href><d:propstat><d:prop>\
                 <d:getetag>\"{etag}\"</d:getetag></d:prop>\
                 <d:status>HTTP/1.1 200 OK</d:status></d:propstat></d:response>",
                    etag = etag(data),
                ),
            },
        )
        .collect();
    if state.faults.truncate.is_some() {
        rows.push_str(
            "<d:response><d:href>/dav/book/</d:href>\
             <d:status>HTTP/1.1 507 Insufficient Storage</d:status></d:response>",
        );
    }
    rows
}

fn multistatus(rows: &str) -> String {
    format!(
        "<?xml version=\"1.0\" encoding=\"utf-8\"?>\n\
         <d:multistatus xmlns:d=\"DAV:\" xmlns:c=\"urn:ietf:params:xml:ns:carddav\">{rows}</d:multistatus>"
    )
}

fn etag(data: &str) -> String {
    let mut hasher = DefaultHasher::new();
    data.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

fn token(state: &State) -> String {
    let mut hasher = DefaultHasher::new();
    state.cards.hash(&mut hasher);
    format!("tok-{:016x}", hasher.finish())
}

fn between<'a>(text: &'a str, open: &str, close: &str) -> Option<&'a str> {
    let start = text.find(open)? + open.len();
    let end = text[start..].find(close)?;
    Some(&text[start..start + end])
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
