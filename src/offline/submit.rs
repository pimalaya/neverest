//! The `submit` intent: a queued submission carried by the pimdir action
//! queue, and the send channel it leaves through.
//!
//! # The contract
//!
//! Submission is the one mail-specific concept left in an otherwise
//! kind-neutral engine, and it is confined here, to the send channel. It is
//! **not** a store concept: pimdir carries an action kind and a versioned
//! JSON payload, and `submit` is a kind **neverest** defines. Any other
//! owner draining the same queue skips it (it lacks the capability), so the
//! row stays pending rather than parked.
//!
//! A producer (himalaya, today) enqueues one queue row:
//!
//! - **kind**: `submit`.
//! - **payload** (`v: 1`):
//!
//!   ```json
//!   {
//!     "v": 1,
//!     "object": "<body hash>",
//!     "from": "a@x.org",
//!     "rcpts": ["b@y.org"],
//!     "subject": "hi"
//!   }
//!   ```
//!
//!   `v`, `object` and `from` are required (an empty `from` is the null
//!   reverse path, for bounces), `rcpts` defaults to empty and `subject` is
//!   optional and for the report only. Unknown fields are ignored so the
//!   schema can grow.
//! - **the body**: written durably into the store's object store *before* the
//!   enqueue, and named by the payload's `object`, which is the pimdir
//!   convention every action kind follows. The queue row therefore pins it
//!   ("queued bodies are pinned"), so GC cannot sweep the body between the
//!   enqueue and the send. The body belongs to **no collection**: it is not an
//!   item, it is a payload in flight.
//! - **collection**: whatever the producer chose to anchor the intent on (a
//!   client typically picks `Sent`). Neverest scans every collection's
//!   pending actions, so there is no anchor rule and no schema change.
//!
//! Neverest sends it through the first side offering a channel: that side's
//! own `smtp` table (an SMTP submission session) or its native send (the
//! Graph `sendMail` action, which files the message in Sent itself). On
//! success the row is acknowledged (`drop_action`), which releases the
//! object pin and lets the next GC reclaim the body.
//!
//! # Known property: submission is at-least-once
//!
//! A crash between "the server accepted the message" and "the queue row is
//! acknowledged" resends on the next run. That was already true of the
//! Outbox collection this replaces, so it is not a regression, but as a
//! queue intent it is a visible contract: **deduplication is the provider's
//! job**, through `Message-ID` dedup on the receiving side. Neverest does
//! not, and cannot, close that window by itself (no transaction spans an
//! SMTP dialogue).
//!
//! A build compiled without any send channel (neither `smtp` nor `msgraph`)
//! **skips** submit intents: they stay pending, never parked, since another
//! build can perform them.

#[cfg(feature = "smtp")]
use std::{borrow::Cow, net::Ipv4Addr};

#[cfg(feature = "smtp")]
use anyhow::Context;
use anyhow::{Result, anyhow};
#[cfg(feature = "msgraph")]
use io_msgraph::v1::client::MsgraphClientStdError;
#[cfg(any(feature = "smtp", feature = "msgraph"))]
use io_pimdir::PimdirBlobs;
use io_pimdir::{PimdirStore, codec::PimdirAction};
use io_replica::object::ReplicaHash;
#[cfg(feature = "smtp")]
use io_sasl::{login::SaslLoginCreds, mechanism::Sasl};
#[cfg(feature = "smtp")]
use io_smtp::{
    client::{SmtpClient as _, SmtpClientError, SmtpClientStd},
    message::SmtpMessageSendError,
    rfc5321::{
        SmtpDomain, SmtpEhloDomain, SmtpForwardPath, SmtpLocalPart, SmtpMailbox, SmtpReversePath,
        data::SmtpDataError, mail::SmtpMailError, rcpt::SmtpRcptError,
    },
    session::SmtpSessionOpenOptions,
};
use log::warn;
use serde::Deserialize;
#[cfg(feature = "smtp")]
use url::Url;

#[cfg(feature = "smtp")]
use crate::config::SmtpConfig;
#[cfg(feature = "msgraph")]
use crate::msgraph::client::GraphClient;

/// The queue action kind neverest defines for a submission. pimdir knows
/// nothing about it: it carries the kind and the payload, an owner that
/// cannot perform it skips the row.
pub const SUBMIT: &str = "submit";

/// One pending `submit` row, as read from the queue: the payload stays raw
/// so a malformed one can be parked with its reason rather than hiding the
/// whole intent.
//
#[cfg_attr(not(any(feature = "smtp", feature = "msgraph")), allow(dead_code))]
#[derive(Clone, Debug)]
pub struct SubmitIntent {
    /// The queue row's global append id, the handle for acknowledging
    /// (`drop_action`) or parking (`fail_action`) it.
    pub id: i64,
    /// The collection the producer anchored the intent on.
    pub collection: String,
    /// The pinned body blob.
    pub object: Option<ReplicaHash>,
    /// The raw versioned JSON payload.
    pub payload: String,
}

#[cfg_attr(not(any(feature = "smtp", feature = "msgraph")), allow(dead_code))]
impl SubmitIntent {
    /// The decoded envelope. A payload that does not decode is a
    /// **permanent** failure: no later run decodes it any better.
    pub fn envelope(&self) -> Result<SubmitMeta, SubmitFailure> {
        let meta: SubmitMeta = serde_json::from_str(&self.payload)
            .map_err(|err| SubmitFailure::permanent(anyhow!("Malformed submit payload: {err}")))?;
        if meta.v != 1 {
            return Err(SubmitFailure::permanent(anyhow!(
                "Unsupported submit payload version {}",
                meta.v
            )));
        }
        Ok(meta)
    }

    /// The subject for the report, best effort: an intent whose payload is
    /// too broken to decode still has to be reportable.
    pub fn subject(&self) -> Option<String> {
        serde_json::from_str::<SubmitMeta>(&self.payload)
            .ok()
            .and_then(|meta| meta.subject)
    }
}

/// The `v: 1` submit payload: the SMTP envelope plus one display field.
///
/// The envelope serves the SMTP channel; a native sender (Graph) reads the
/// addresses out of the MIME body itself and only needs the blob, so a
/// Graph-only build decodes the payload (a broken one still parks) without
/// reading the envelope back out.
#[cfg_attr(not(feature = "smtp"), allow(dead_code))]
#[derive(Debug, Deserialize)]
pub struct SubmitMeta {
    /// The schema version (1).
    pub v: u8,
    /// The envelope sender (`MAIL FROM`); empty means the null path.
    pub from: String,
    /// The envelope recipients (`RCPT TO`).
    #[serde(default)]
    pub rcpts: Vec<String>,
    /// The subject, for the report only.
    #[serde(default)]
    pub subject: Option<String>,
}

/// How a failed submission is dispositioned.
///
/// The distinction is the whole point of putting submission in the queue: a
/// transient failure keeps the intent, a permanent one stops re-sending
/// forever while keeping the row queryable.
#[cfg_attr(not(any(feature = "smtp", feature = "msgraph")), allow(dead_code))]
#[derive(Debug)]
pub enum SubmitFailure {
    /// Retry: the row stays pending and the next run tries again (a
    /// connection failure, an SMTP 4xx).
    Transient(anyhow::Error),
    /// Park: no run can do better (a malformed payload, a missing body, an
    /// SMTP 5xx). The row is kept with its error for an operator.
    Permanent(anyhow::Error),
}

#[cfg_attr(not(any(feature = "smtp", feature = "msgraph")), allow(dead_code))]
impl SubmitFailure {
    /// A permanent failure from any error.
    pub fn permanent(err: anyhow::Error) -> Self {
        Self::Permanent(err)
    }

    /// Whether this failure parks the row.
    pub fn parks(&self) -> bool {
        matches!(self, Self::Permanent(_))
    }

    /// The underlying error, for the report and the log.
    pub fn error(&self) -> &anyhow::Error {
        match self {
            Self::Transient(err) | Self::Permanent(err) => err,
        }
    }
}

/// Every pending `submit` intent in the store, across every collection, in
/// queue order.
///
/// The store's drain applies the action kinds pimdir defines and **skips**
/// the ones it does not (they read back as [`PimdirAction::Unknown`]), so
/// this reads exactly what the drain deliberately left behind. A collection
/// whose queue cannot be read is warned about and skipped, so one broken row
/// never blocks the account's other submissions.
pub fn pending(store: &PimdirStore) -> Result<Vec<SubmitIntent>> {
    let collections = store
        .queued_collections()
        .map_err(|err| anyhow!("Cannot list queued collections: {err}"))?;

    let mut intents = Vec::new();
    for collection in collections {
        let rows = match store.pending_actions(&collection) {
            Ok(rows) => rows,
            Err(err) => {
                warn!("cannot read the queue of `{collection}`, skipping it: {err}");
                continue;
            }
        };
        for row in rows {
            let PimdirAction::Unknown {
                kind,
                payload,
                object_hash,
            } = row.action
            else {
                continue;
            };
            if kind != SUBMIT {
                continue;
            }
            intents.push(SubmitIntent {
                id: row.id,
                collection: collection.clone(),
                object: object_hash,
                payload,
            });
        }
    }
    Ok(intents)
}

/// The send channel a submission leaves through, resolved per account. Its
/// variants are the send-capable backends compiled in; a build with none of
/// them has no channel type at all and can only leave intents pending.
#[cfg(any(feature = "smtp", feature = "msgraph"))]
pub enum SendChannel<'a> {
    /// A fresh SMTP submission session (the sending side's `smtp` table),
    /// quit once every intent has been attempted.
    #[cfg(feature = "smtp")]
    Smtp(SmtpClientStd),
    /// The account's live Graph session: the sendMail action with the raw
    /// MIME body (Graph files it in Sent itself).
    #[cfg(feature = "msgraph")]
    Graph(&'a mut GraphClient),
    /// Ties the lifetime down when the Graph variant is compiled out.
    #[cfg(not(feature = "msgraph"))]
    #[allow(dead_code)]
    Unused(core::marker::PhantomData<&'a ()>),
}

#[cfg(any(feature = "smtp", feature = "msgraph"))]
impl SendChannel<'_> {
    /// Closes the channel cleanly once the run's intents are attempted (the
    /// SMTP session is ours; a Graph session belongs to its side).
    pub fn close(&mut self) {
        #[cfg(feature = "smtp")]
        if let SendChannel::Smtp(client) = self {
            let _ = client.quit();
        }
    }
}

/// Connects the SMTP submission session: TCP or implicit TLS per the URL
/// scheme, the optional STARTTLS upgrade, then the LOGIN SASL exchange when
/// a login is configured.
#[cfg(feature = "smtp")]
pub fn connect_smtp(config: &SmtpConfig) -> Result<SmtpClientStd> {
    let url = Url::parse(&config.server).context("Cannot parse the SMTP submission URL")?;
    let alpn = config
        .alpn
        .clone()
        .unwrap_or_else(SmtpClientStd::default_alpn);
    let tls = config.tls.clone().into_tls(alpn);
    let sasl = match (&config.login, &config.password) {
        (Some(login), Some(password)) => Some(Sasl::Login(SaslLoginCreds {
            username: login.clone(),
            password: password.clone().get()?,
        })),
        (None, None) => None,
        _ => anyhow::bail!("SMTP channel needs both `login` and `password`, or neither"),
    };
    let opts = SmtpSessionOpenOptions {
        starttls: config.starttls,
    };

    let (client, _capabilities) = SmtpClientStd::connect(&url, &tls, ehlo_domain(), sasl, opts)
        .context("Cannot connect to the SMTP submission server")?;
    Ok(client)
}

/// The EHLO identity of the submission sessions: the loopback address
/// literal RFC 5321 §4.1.3 reserves for a client with no resolvable domain
/// name of its own, which a desktop client behind a NAT never has.
///
/// A bare `localhost` is not a name either, and a server entitled to check
/// (RFC 5321 §4.1.4) refuses it: Stalwart answers `550 5.5.0 Invalid EHLO
/// domain`, so every queued intent stayed pending against it.
#[cfg(feature = "smtp")]
fn ehlo_domain() -> SmtpEhloDomain<'static> {
    Ipv4Addr::LOCALHOST.into()
}

/// Sends one intent through `channel`: the payload provides the SMTP
/// envelope, the pinned blob the raw bytes. Message content is never
/// logged.
#[cfg(any(feature = "smtp", feature = "msgraph"))]
pub fn send_one(
    channel: &mut SendChannel<'_>,
    blobs: &PimdirBlobs,
    intent: &SubmitIntent,
) -> Result<(), SubmitFailure> {
    // NOTE: decoded whatever the channel is, since a payload this build cannot
    // read is a broken intent and parking it beats sending it blind. A native
    // sender then takes its addresses from the MIME body itself.
    #[cfg_attr(not(feature = "smtp"), allow(unused_variables))]
    let meta = intent.envelope()?;
    let hash = intent
        .object
        .as_ref()
        .ok_or_else(|| SubmitFailure::permanent(anyhow!("Submit intent has no stored body")))?;
    let bytes = blobs
        .get(hash)
        // NOTE: a blob read failure is a filesystem problem, not a bad intent.
        .map_err(|err| SubmitFailure::Transient(anyhow!("Cannot read the queued blob: {err}")))?
        .ok_or_else(|| SubmitFailure::permanent(anyhow!("The queued body is missing")))?;

    match channel {
        #[cfg(feature = "smtp")]
        SendChannel::Smtp(client) => {
            let reverse = reverse_path(&meta.from).map_err(SubmitFailure::permanent)?;
            let forwards = meta
                .rcpts
                .iter()
                .map(|rcpt| Ok(SmtpForwardPath(smtp_mailbox(rcpt)?)))
                .collect::<Result<Vec<_>>>()
                .map_err(SubmitFailure::permanent)?;
            client.send(reverse, forwards, bytes).map_err(classify_smtp)
        }
        #[cfg(feature = "msgraph")]
        SendChannel::Graph(client) => client.send_mime(&bytes).map_err(classify_graph),
        #[cfg(not(feature = "msgraph"))]
        SendChannel::Unused(_) => unreachable!("the placeholder channel is never constructed"),
    }
}

/// Classifies an SMTP send failure the way RFC 5321 §4.2.1 does: a 5xx
/// reply is permanent (the server will refuse it again), a 4xx reply and
/// anything else (a dropped connection, a TLS error) is transient.
#[cfg(feature = "smtp")]
fn classify_smtp(err: SmtpClientError) -> SubmitFailure {
    // NOTE: a `send` runs the composite coroutine, so its rejections arrive
    // nested under `MessageSend`. The flat variants are the same failures seen
    // through a single-command call.
    let code = match &err {
        SmtpClientError::MessageSend(SmtpMessageSendError::MailFrom(SmtpMailError::Rejected {
            code,
            ..
        }))
        | SmtpClientError::Mail(SmtpMailError::Rejected { code, .. }) => Some(*code),
        SmtpClientError::MessageSend(SmtpMessageSendError::RcptTo(SmtpRcptError::Rejected {
            code,
            ..
        }))
        | SmtpClientError::Rcpt(SmtpRcptError::Rejected { code, .. }) => Some(*code),
        SmtpClientError::MessageSend(SmtpMessageSendError::Data(
            SmtpDataError::CommandRejected { code, .. } | SmtpDataError::BodyRejected { code, .. },
        ))
        | SmtpClientError::Data(
            SmtpDataError::CommandRejected { code, .. } | SmtpDataError::BodyRejected { code, .. },
        ) => Some(*code),
        _ => None,
    };
    let err = anyhow!(err).context("SMTP submission error");
    match code {
        Some(code) if (500..600).contains(&code) => SubmitFailure::Permanent(err),
        _ => SubmitFailure::Transient(err),
    }
}

/// Classifies a Graph `sendMail` failure: a 4xx status is a rejection of
/// this message (permanent), except the two "come back later" ones; a 5xx,
/// a transport error or anything without a status is transient.
#[cfg(feature = "msgraph")]
fn classify_graph(err: MsgraphClientStdError) -> SubmitFailure {
    let status = match &err {
        MsgraphClientStdError::Send(send) => send.status(),
        _ => None,
    };
    let err = anyhow!(err).context("Graph sendMail error");
    match status {
        Some(408 | 429) => SubmitFailure::Transient(err),
        Some(status) if (400..500).contains(&status) => SubmitFailure::Permanent(err),
        _ => SubmitFailure::Transient(err),
    }
}

/// The MAIL FROM reverse path of an envelope sender, the null path for an
/// empty one (bounces).
#[cfg(feature = "smtp")]
fn reverse_path(from: &str) -> Result<SmtpReversePath<'static>> {
    if from.is_empty() {
        return Ok(SmtpReversePath::Null);
    }
    Ok(SmtpReversePath::SmtpMailbox(smtp_mailbox(from)?))
}

/// Splits an address into the io-smtp mailbox shape at its last `@`.
#[cfg(feature = "smtp")]
fn smtp_mailbox(addr: &str) -> Result<SmtpMailbox<'static>> {
    let (local, domain) = addr
        .rsplit_once('@')
        .with_context(|| format!("Envelope address {addr} misses a domain"))?;
    Ok(SmtpMailbox {
        local_part: SmtpLocalPart(Cow::Owned(local.to_owned())),
        domain: SmtpEhloDomain::SmtpDomain(SmtpDomain(Cow::Owned(domain.to_owned()))),
    })
}

#[cfg(all(test, feature = "smtp"))]
mod tests {
    use std::{
        io::{BufRead, BufReader, Write as _},
        net::TcpListener,
        sync::mpsc,
        thread,
    };

    use io_pimdir::{PimdirBlobs, hash::PimdirHashAlgo};

    use super::*;

    /// What the scripted SMTP sink captured from one session: the envelope
    /// command lines and the DATA payload.
    struct Captured {
        commands: Vec<String>,
        data: Vec<u8>,
    }

    /// A minimal scripted SMTP sink on a random local port: one thread
    /// accepts one session, answers the canonical submission dialogue
    /// (greeting, EHLO, MAIL, RCPT, DATA, QUIT) and captures the envelope
    /// and message bytes. `reject` makes it answer the DATA command with
    /// that reply line instead of accepting.
    fn spawn_smtp_sink(reject: Option<&'static str>) -> (u16, mpsc::Receiver<Captured>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind sink");
        let port = listener.local_addr().expect("sink addr").port();
        let (tx, rx) = mpsc::channel();

        thread::spawn(move || {
            let (stream, _) = listener.accept().expect("accept");
            let mut reader = BufReader::new(stream.try_clone().expect("clone"));
            let mut stream = stream;
            let mut captured = Captured {
                commands: Vec::new(),
                data: Vec::new(),
            };

            stream.write_all(b"220 sink\r\n").expect("greet");
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 {
                    break;
                }
                let upper = line.to_ascii_uppercase();
                if upper.starts_with("EHLO") {
                    stream.write_all(b"250 sink\r\n").expect("ehlo");
                } else if upper.starts_with("MAIL") || upper.starts_with("RCPT") {
                    captured.commands.push(line.trim_end().to_owned());
                    stream.write_all(b"250 OK\r\n").expect("ok");
                } else if upper.starts_with("DATA") {
                    if let Some(reply) = reject {
                        stream.write_all(reply.as_bytes()).expect("reject");
                        continue;
                    }
                    stream.write_all(b"354 go\r\n").expect("go");
                    loop {
                        let mut data_line = Vec::new();
                        let mut byte = [0u8; 1];
                        loop {
                            use std::io::Read;
                            if reader.read_exact(&mut byte).is_err() {
                                break;
                            }
                            data_line.push(byte[0]);
                            if byte[0] == b'\n' {
                                break;
                            }
                        }
                        if data_line == b".\r\n" || data_line.is_empty() {
                            break;
                        }
                        captured.data.extend_from_slice(&data_line);
                    }
                    stream.write_all(b"250 queued\r\n").expect("queued");
                } else if upper.starts_with("QUIT") {
                    let _ = stream.write_all(b"221 bye\r\n");
                    break;
                } else {
                    stream.write_all(b"250 OK\r\n").expect("any");
                }
            }
            let _ = tx.send(captured);
        });

        (port, rx)
    }

    /// Writes a body into the blob store and returns the intent pointing at
    /// it, the exact shape a producer's queue row lands as.
    fn stage_intent(blobs: &PimdirBlobs, id: i64, payload: &str, body: &[u8]) -> SubmitIntent {
        let hash = ReplicaHash(format!("hash-{id}"));
        let mut writer = blobs.writer().expect("blob writer");
        std::io::Write::write_all(&mut writer, body).expect("write body");
        writer.commit(&hash).expect("commit body");
        SubmitIntent {
            id,
            collection: String::from("Sent"),
            object: Some(hash),
            payload: payload.to_owned(),
        }
    }

    fn channel_to(port: u16) -> SendChannel<'static> {
        let config: SmtpConfig = toml::from_str(&format!(
            "server = \"smtp://127.0.0.1:{port}\"\nstarttls = false\n"
        ))
        .unwrap();
        SendChannel::Smtp(connect_smtp(&config).expect("connect sink"))
    }

    #[test]
    fn an_intent_sends_its_pinned_body_through_the_envelope_it_carries() {
        let dir = tempfile::tempdir().unwrap();
        let blobs = PimdirBlobs::open(dir.path(), PimdirHashAlgo::default());
        let body = b"Subject: hi\r\n\r\nhello".to_vec();
        let intent = stage_intent(
            &blobs,
            1,
            r#"{"v":1,"from":"a@x.org","rcpts":["b@y.org","c@y.org"],"subject":"hi"}"#,
            &body,
        );
        assert_eq!(intent.subject().as_deref(), Some("hi"));

        let (port, captured) = spawn_smtp_sink(None);
        let mut channel = channel_to(port);
        send_one(&mut channel, &blobs, &intent).expect("send");
        channel.close();

        let captured = captured.recv().expect("captured session");
        assert_eq!(
            captured.commands,
            [
                "MAIL FROM:<a@x.org>",
                "RCPT TO:<b@y.org>",
                "RCPT TO:<c@y.org>",
            ]
        );
        assert_eq!(captured.data, [body.as_slice(), b"\r\n"].concat());
    }

    #[test]
    fn a_5xx_rejection_parks_the_intent_and_a_4xx_one_keeps_it() {
        let dir = tempfile::tempdir().unwrap();
        let blobs = PimdirBlobs::open(dir.path(), PimdirHashAlgo::default());
        let payload = r#"{"v":1,"from":"a@x.org","rcpts":["b@y.org"],"subject":"hi"}"#;

        let intent = stage_intent(&blobs, 1, payload, b"body");
        let (port, _) = spawn_smtp_sink(Some("554 rejected\r\n"));
        let mut channel = channel_to(port);
        let failure = send_one(&mut channel, &blobs, &intent).expect_err("rejected");
        assert!(failure.parks(), "5xx must park: {}", failure.error());

        let intent = stage_intent(&blobs, 2, payload, b"body");
        let (port, _) = spawn_smtp_sink(Some("451 try later\r\n"));
        let mut channel = channel_to(port);
        let failure = send_one(&mut channel, &blobs, &intent).expect_err("deferred");
        assert!(!failure.parks(), "4xx must retry: {}", failure.error());
    }

    #[test]
    fn an_undecodable_or_bodyless_intent_parks_instead_of_looping() {
        let dir = tempfile::tempdir().unwrap();
        let blobs = PimdirBlobs::open(dir.path(), PimdirHashAlgo::default());

        let broken = stage_intent(&blobs, 1, "not json", b"body");
        assert!(broken.envelope().expect_err("malformed").parks());
        assert!(broken.subject().is_none());
        let future = stage_intent(&blobs, 2, r#"{"v":9,"from":"a@x.org"}"#, b"body");
        assert!(future.envelope().expect_err("v9").parks());

        let bodyless = SubmitIntent {
            object: None,
            ..stage_intent(&blobs, 3, r#"{"v":1,"from":"a@x.org"}"#, b"body")
        };
        let (port, _) = spawn_smtp_sink(None);
        let mut channel = channel_to(port);
        assert!(
            send_one(&mut channel, &blobs, &bodyless)
                .expect_err("no body")
                .parks()
        );
    }

    #[test]
    fn envelope_addresses_map_to_smtp_paths() {
        assert!(matches!(reverse_path("").unwrap(), SmtpReversePath::Null));

        let SmtpReversePath::SmtpMailbox(mailbox) = reverse_path("a@example.org").unwrap() else {
            panic!("expected a mailbox path");
        };
        assert_eq!(mailbox.local_part.as_ref(), "a");
        assert!(smtp_mailbox("no-domain").is_err());
    }
}
