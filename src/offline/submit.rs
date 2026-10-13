//! # The submit intent
//!
//! A queued submission carried by the pimdir action queue, and the send channel
//! it leaves through.
//!
//! Submission is the one mail-specific concept left in an otherwise
//! kind-neutral engine, confined here. It is not a store concept: `submit` is
//! an action kind neverest defines, and an owner draining the same queue
//! without the capability skips the row rather than parking it.
//!
//! A producer enqueues a `submit` row whose payload is the `v: 1` envelope
//! ([`SubmitMeta`]) and whose body is written into the object store before the
//! enqueue, so the row pins it and GC cannot sweep it in between. The anchor
//! collection is the producer's choice, every collection's queue being scanned.
//!
//! ```json
//! {"v":1,"from":"a@x.org","rcpts":["b@y.org","c@y.org"],"subject":"hi"}
//! ```
//!
//! Submission is at-most-once on its own: a message handed over whole with no
//! verdict back is *uncertain*, parked under [`UNCERTAIN`] and never sent
//! again by any run, since a second send may deliver it twice. The run marks
//! the intent it is handing over ([`InFlight`]) before the first byte leaves,
//! so a run killed mid-send leaves the mark, and the next run parks that
//! intent as uncertain instead of sending it again. Only a person, through
//! the producer, sends an uncertain message again.
//!
//! A build with no send channel skips the intents, leaving them pending rather
//! than parked, since another build can perform them.

#[cfg(feature = "smtp")]
use std::{borrow::Cow, net::Ipv4Addr};
use std::{
    fs, io,
    path::{Path, PathBuf},
};

#[cfg(feature = "smtp")]
use anyhow::Context;
use anyhow::{Result, anyhow};
#[cfg(any(feature = "smtp", feature = "msgraph"))]
use io_pimdir::client::blobs::PimdirBlobs;
use io_pimdir::{client::PimdirStore, codec::PimdirAction, object::PimdirHash};
#[cfg(feature = "smtp")]
use io_smtp::{
    client::{SmtpClient as _, SmtpClientError, SmtpClientStd, SmtpClientStdConnectOptions},
    coroutine::{SmtpCoroutine, SmtpCoroutineState, SmtpYield},
    message::{SmtpMessageSend, SmtpMessageSendError, SmtpMessageSendOptions},
    rfc5321::{
        SmtpDomain, SmtpEhloDomain, SmtpForwardPath, SmtpLocalPart, SmtpMailbox, SmtpReversePath,
        data::SmtpDataError, mail::SmtpMailError, rcpt::SmtpRcptError,
    },
    send::SmtpCommandSendError,
    session::{SmtpSessionOpenError, SmtpSessionOpenOptions},
};
#[cfg(feature = "smtp")]
use pimalaya_stream::proxy::Proxy;
use serde::Deserialize;

#[cfg(feature = "smtp")]
use crate::account::SmtpAccount;
#[cfg(feature = "msgraph")]
use crate::msgraph::client::{GraphClient, SendMimeFailure};

/// The queue action kind neverest defines for a submission.
///
/// pimdir knows nothing about it: it carries the kind and the payload, and an
/// owner that cannot perform it skips the row.
pub const SUBMIT: &str = "submit";

/// How the error of a parked uncertain submission begins, the rest being
/// for a person.
///
/// A producer reading the row alone (`himalaya pimdir queue show`) tells it
/// from a refusal by this prefix: a refused message was not delivered, an
/// uncertain one may have been.
pub const UNCERTAIN: &str = "uncertain: ";

/// The file beside `pimdir.db` naming the intent a run is handing over.
const IN_FLIGHT: &str = "submitting";

/// One pending `submit` row, as read from the queue.
///
/// The payload stays raw so a malformed one can be parked with its reason
/// rather than hiding the whole intent.
#[cfg_attr(not(any(feature = "smtp", feature = "msgraph")), allow(dead_code))]
#[derive(Clone, Debug)]
pub struct SubmitIntent {
    /// The queue row's append id, the handle for acknowledging or parking it.
    pub id: i64,
    /// The collection the producer anchored the intent on.
    pub collection: String,
    /// The pinned body blob.
    pub object: Option<PimdirHash>,
    /// The raw versioned JSON payload.
    pub payload: String,
}

#[cfg_attr(not(any(feature = "smtp", feature = "msgraph")), allow(dead_code))]
impl SubmitIntent {
    /// The decoded envelope.
    ///
    /// A payload that does not decode is a permanent failure: no later run
    /// decodes it any better.
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

    /// The collection the producer asked the sent copy filed in, best
    /// effort.
    pub fn copy(&self) -> Option<String> {
        serde_json::from_str::<SubmitMeta>(&self.payload)
            .ok()
            .and_then(|meta| meta.copy)
    }

    /// The source the producer named to perform it, best effort.
    pub fn source(&self) -> Option<String> {
        serde_json::from_str::<SubmitMeta>(&self.payload)
            .ok()
            .and_then(|meta| meta.source)
    }

    /// The subject for the report, best effort.
    ///
    /// An intent whose payload is too broken to decode still has to be
    /// reportable.
    pub fn subject(&self) -> Option<String> {
        serde_json::from_str::<SubmitMeta>(&self.payload)
            .ok()
            .and_then(|meta| meta.subject)
    }
}

/// The `v: 1` submit payload: the SMTP envelope plus one display field.
///
/// A native sender (Graph) reads the addresses out of the MIME body itself and
/// only needs the blob, so a Graph-only build decodes the payload (a broken one
/// still parks) without reading the envelope back out.
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
    /// The source named to perform it (pimdir STORAGE §15.6), absent from
    /// a producer predating the field.
    #[serde(default)]
    pub source: Option<String>,
    /// The collection a copy of the sent message is filed in (pimdir
    /// STORAGE Annex B.2), none when the producer asked for none.
    #[serde(default)]
    pub copy: Option<String>,
}

/// How a failed submission is dispositioned.
///
/// The distinction is the whole point of putting submission in the queue: a
/// transient failure keeps the intent, a permanent one stops re-sending
/// forever while keeping the row queryable, and an uncertain one stops it
/// too, the message having perhaps left.
#[cfg_attr(not(any(feature = "smtp", feature = "msgraph")), allow(dead_code))]
#[derive(Debug)]
pub enum SubmitFailure {
    /// Retry: certainly not delivered, the row stays pending for the next
    /// run (a dropped link before the message went whole, a 4xx).
    Transient(anyhow::Error),
    /// Park: no run does better; the row keeps its error (5xx, bad payload).
    Permanent(anyhow::Error),
    /// Park, never resend: the message went whole and no verdict came back,
    /// or the provider answered with an error a gateway gives after the
    /// work was done. It may have been delivered.
    Uncertain(anyhow::Error),
}

#[cfg_attr(not(any(feature = "smtp", feature = "msgraph")), allow(dead_code))]
impl SubmitFailure {
    /// A permanent failure from any error.
    pub fn permanent(err: anyhow::Error) -> Self {
        Self::Permanent(err)
    }

    /// Whether this failure parks the row.
    pub fn parks(&self) -> bool {
        matches!(self, Self::Permanent(_) | Self::Uncertain(_))
    }

    /// Whether the message may have been delivered despite the failure.
    pub fn uncertain(&self) -> bool {
        matches!(self, Self::Uncertain(_))
    }

    /// The underlying error, for the log.
    pub fn error(&self) -> &anyhow::Error {
        match self {
            Self::Transient(err) | Self::Permanent(err) | Self::Uncertain(err) => err,
        }
    }

    /// The error the row is parked with and the report shows: an uncertain
    /// one begins with [`UNCERTAIN`].
    pub fn reason(&self) -> String {
        match self {
            Self::Uncertain(err) => uncertain_reason(&format!("{err:#}")),
            Self::Transient(err) | Self::Permanent(err) => format!("{err:#}"),
        }
    }
}

/// The parked error of an uncertain submission, `detail` saying why.
pub fn uncertain_reason(detail: &str) -> String {
    format!(
        "{UNCERTAIN}the message may have been delivered, so it is never sent again on its own: {detail}"
    )
}

/// What a failure tells about the message, before it is wrapped with its
/// error: the classifiers' pure half.
#[cfg_attr(not(any(feature = "smtp", feature = "msgraph")), allow(dead_code))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Disposition {
    /// Certainly not delivered, worth trying again.
    Transient,
    /// Refused for good.
    Permanent,
    /// Maybe delivered.
    Uncertain,
}

#[cfg_attr(not(any(feature = "smtp", feature = "msgraph")), allow(dead_code))]
impl Disposition {
    fn with(self, err: anyhow::Error) -> SubmitFailure {
        match self {
            Self::Transient => SubmitFailure::Transient(err),
            Self::Permanent => SubmitFailure::Permanent(err),
            Self::Uncertain => SubmitFailure::Uncertain(err),
        }
    }
}

/// An SMTP failure by its reply code (RFC 5321 §4.2.1), `None` when no
/// reply came, and whether the data's final `.` had been written whole.
///
/// A 5yz reply refuses the message for good and a 4yz one asks for it
/// later; both are a verdict, so the message was not taken. Without a reply,
/// a failure before the final `.` left the transaction unfinished, which the
/// server discards; after it, the server may have taken the message.
#[cfg_attr(not(feature = "smtp"), allow(dead_code))]
pub fn smtp_disposition(code: Option<u16>, handed_over: bool) -> Disposition {
    match code {
        Some(code) if (500..600).contains(&code) => Disposition::Permanent,
        Some(_) => Disposition::Transient,
        None if handed_over => Disposition::Uncertain,
        None => Disposition::Transient,
    }
}

/// A Graph `sendMail` failure by its HTTP status, `None` when no answer was
/// read, and whether the request had gone whole with no answer yet.
///
/// 400, 403, 404, 413 and 422 refuse the message; 401 (an expired token),
/// 408 and 429 refuse it unprocessed, for later; 500, 502, 503 and 504 may
/// come from a gateway after the work was done. Any other 4xx refuses it,
/// any other status leaves it for later.
#[cfg_attr(not(feature = "msgraph"), allow(dead_code))]
pub fn graph_disposition(status: Option<u16>, unanswered: bool) -> Disposition {
    match status {
        Some(401 | 408 | 429) => Disposition::Transient,
        Some(400 | 403 | 404 | 413 | 422) => Disposition::Permanent,
        Some(500 | 502 | 503 | 504) => Disposition::Uncertain,
        Some(status) if (400..500).contains(&status) => Disposition::Permanent,
        Some(_) => Disposition::Transient,
        None if unanswered => Disposition::Uncertain,
        None => Disposition::Transient,
    }
}

/// Every pending `submit` intent in the store, in queue order.
///
/// The drain skips the kinds pimdir does not define (they read back as
/// [`PimdirAction::Unknown`]), so this reads exactly what it left behind.
pub fn pending(store: &PimdirStore) -> Result<Vec<SubmitIntent>> {
    let rows = store
        .list_pending_actions()
        .map_err(|err| anyhow!("Cannot read the queue: {err}"))?;

    let mut intents = Vec::new();
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
            collection: row.collection,
            object: object_hash,
            payload,
        });
    }
    Ok(intents)
}

/// The mark of the intent a run is handing over, a file beside the store.
///
/// Written durably before the first byte of a message leaves and removed
/// once its outcome is in the store, so a run killed in between (a caller's
/// deadline) leaves it behind: the next run reads it and parks that intent
/// as uncertain rather than sending it again. It is neverest's bookkeeping,
/// not part of the pimdir format, and one intent at a time is handed over.
#[derive(Clone, Debug)]
pub struct InFlight {
    path: PathBuf,
}

impl InFlight {
    /// The mark of the store at `dir`.
    pub fn new(dir: &Path) -> Self {
        Self {
            path: dir.join(IN_FLIGHT),
        }
    }

    /// The intent an earlier run was handing over when it stopped, if it
    /// left one.
    pub fn left(&self) -> Option<i64> {
        let raw = fs::read_to_string(&self.path).ok()?;
        raw.trim().parse().ok()
    }

    /// Marks `id` as being handed over, durably: written aside, synced,
    /// then renamed into place, so a mark is either whole or absent.
    #[cfg_attr(not(any(feature = "smtp", feature = "msgraph")), allow(dead_code))]
    pub fn begin(&self, id: i64) -> io::Result<()> {
        let staged = self.path.with_extension("new");
        let mut file = fs::File::create(&staged)?;
        io::Write::write_all(&mut file, format!("{id}\n").as_bytes())?;
        file.sync_all()?;
        fs::rename(&staged, &self.path)
    }

    /// Clears the mark once the outcome of the intent is recorded.
    pub fn end(&self) -> io::Result<()> {
        match fs::remove_file(&self.path) {
            Err(err) if err.kind() == io::ErrorKind::NotFound => Ok(()),
            other => other,
        }
    }
}

/// The send channel a submission leaves through, resolved per account.
///
/// Its links are the send-capable backends compiled in; a build with none of
/// them has no channel type at all and can only leave intents pending.
#[cfg(any(feature = "smtp", feature = "msgraph"))]
pub struct SendChannel<'a> {
    link: Link<'a>,
    /// Why the session cannot carry another message, once it cannot.
    lost: Option<String>,
}

#[cfg(any(feature = "smtp", feature = "msgraph"))]
enum Link<'a> {
    /// A fresh SMTP session, quit once every intent has been attempted.
    #[cfg(feature = "smtp")]
    Smtp(SmtpClientStd),
    /// The live Graph session: sendMail with the raw MIME body, filed in Sent.
    #[cfg(feature = "msgraph")]
    Graph(&'a mut GraphClient),
    /// Ties the lifetime down when the Graph variant is compiled out.
    #[cfg(not(feature = "msgraph"))]
    #[allow(dead_code)]
    Unused(core::marker::PhantomData<&'a ()>),
}

#[cfg(any(feature = "smtp", feature = "msgraph"))]
impl<'a> SendChannel<'a> {
    /// A channel over an open SMTP submission session.
    #[cfg(feature = "smtp")]
    pub fn smtp(client: SmtpClientStd) -> Self {
        Self {
            link: Link::Smtp(client),
            lost: None,
        }
    }

    /// A channel through a Graph mail session's `sendMail`.
    #[cfg(feature = "msgraph")]
    pub fn graph(client: &'a mut GraphClient) -> Self {
        Self {
            link: Link::Graph(client),
            lost: None,
        }
    }

    /// Why the session can carry no further message, once a failure broke
    /// it: the intents behind are left for the next run, untouched.
    pub fn lost(&self) -> Option<&str> {
        self.lost.as_deref()
    }

    /// Closes the channel once the run's intents are attempted.
    ///
    /// The SMTP session is ours; a Graph session belongs to its side. A
    /// failed `QUIT` after accepted messages fails none of them.
    pub fn close(&mut self) {
        #[cfg(feature = "smtp")]
        if let Link::Smtp(client) = &mut self.link
            && self.lost.is_none()
        {
            let _ = client.quit();
        }
    }
}

/// Why the SMTP channel did not open.
#[cfg(feature = "smtp")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OpenFailure {
    /// The server could not be reached, or the session broke before its
    /// credentials were answered: the endpoint is unreached.
    Unreached,
    /// The server answered and refused the credentials, or the account
    /// asks for a mechanism this build cannot speak.
    Refused,
}

/// Classifies a failure of [`connect_smtp`].
///
/// Only a refusal the server stated is [`OpenFailure::Refused`]: an
/// authentication exchange cut short by the network is the network's.
#[cfg(feature = "smtp")]
pub fn open_failure(err: &anyhow::Error) -> OpenFailure {
    let Some(SmtpClientError::SessionOpen(open)) = err.downcast_ref::<SmtpClientError>() else {
        return OpenFailure::Unreached;
    };
    match open {
        SmtpSessionOpenError::UnsupportedMechanism(_) => return OpenFailure::Refused,
        SmtpSessionOpenError::AuthAnonymous(_)
        | SmtpSessionOpenError::AuthLogin(_)
        | SmtpSessionOpenError::AuthPlain(_)
        | SmtpSessionOpenError::AuthOauthbearer(_)
        | SmtpSessionOpenError::AuthXoauth2(_)
        | SmtpSessionOpenError::AuthScramSha256(_) => {}
        _ => return OpenFailure::Unreached,
    }
    let refusal: &(dyn std::error::Error + 'static) = open;
    let cut = std::iter::successors(Some(refusal), |err| err.source())
        .any(|err| err.is::<SmtpCommandSendError>() || err.is::<io::Error>());
    match cut {
        true => OpenFailure::Unreached,
        false => OpenFailure::Refused,
    }
}

/// Connects the SMTP submission session, upgrading and authenticating.
///
/// It takes the resolved [`SmtpAccount`] rather than its configuration, so a
/// channel sharing its source's password entry costs no second unlock. A URL
/// with no scheme takes `smtps://`, the implicit TLS RFC 8314 §3.3 asks for.
#[cfg(feature = "smtp")]
pub fn connect_smtp(account: &SmtpAccount) -> Result<SmtpClientStd> {
    let opts = SmtpClientStdConnectOptions {
        tls: account.tls.clone(),
        proxy: Proxy::None,
        sasl: account.sasl.clone(),
        session: SmtpSessionOpenOptions {
            starttls: account.starttls,
        },
    };

    let (client, _capabilities) = SmtpClientStd::connect(&account.server, ehlo_domain(), opts)
        .context("Cannot connect to the SMTP submission server")?;

    Ok(client)
}

/// The EHLO identity: the loopback address literal of RFC 5321 §4.1.3.
///
/// A desktop behind a NAT has no resolvable domain name, and a bare
/// `localhost` is not one either: a server entitled to check (§4.1.4) refuses
/// it, Stalwart answering `550 5.5.0 Invalid EHLO domain`.
#[cfg(feature = "smtp")]
fn ehlo_domain() -> SmtpEhloDomain<'static> {
    Ipv4Addr::LOCALHOST.into()
}

/// One message read out of its intent, ready to hand over.
///
/// Read once its envelope decodes and its body is found, so every failure
/// before the network is told apart from the send's own.
#[cfg(any(feature = "smtp", feature = "msgraph"))]
pub struct Outgoing {
    #[cfg_attr(not(feature = "smtp"), allow(dead_code))]
    meta: SubmitMeta,
    bytes: Vec<u8>,
}

/// Reads the envelope and the pinned body of one intent, the one message
/// held in memory at a time.
#[cfg(any(feature = "smtp", feature = "msgraph"))]
pub fn prepare(blobs: &PimdirBlobs, intent: &SubmitIntent) -> Result<Outgoing, SubmitFailure> {
    let meta = intent.envelope()?;
    let hash = intent
        .object
        .as_ref()
        .ok_or_else(|| SubmitFailure::permanent(anyhow!("Submit intent has no stored body")))?;
    let bytes = blobs
        .get(hash)
        .map_err(|err| SubmitFailure::Transient(anyhow!("Cannot read the queued blob: {err}")))?
        .ok_or_else(|| SubmitFailure::permanent(anyhow!("The queued body is missing")))?;
    Ok(Outgoing { meta, bytes })
}

/// Hands one message over through `channel`.
///
/// The payload provides the SMTP envelope, the pinned blob the raw bytes.
/// Message content is never logged. A failure leaving the session unusable
/// marks the channel [`lost`](SendChannel::lost), and a lost channel
/// attempts nothing.
#[cfg(any(feature = "smtp", feature = "msgraph"))]
pub fn deliver(channel: &mut SendChannel<'_>, outgoing: Outgoing) -> Result<(), SubmitFailure> {
    if let Some(lost) = &channel.lost {
        return Err(SubmitFailure::Transient(anyhow!(
            "Not attempted, the send channel broke on an earlier message: {lost}"
        )));
    }

    #[cfg_attr(not(feature = "smtp"), allow(unused_variables))]
    let Outgoing { meta, bytes } = outgoing;
    match &mut channel.link {
        #[cfg(feature = "smtp")]
        Link::Smtp(client) => {
            let reverse = reverse_path(&meta.from).map_err(SubmitFailure::permanent)?;
            let forwards = meta
                .rcpts
                .iter()
                .map(|rcpt| Ok(SmtpForwardPath(smtp_mailbox(rcpt)?)))
                .collect::<Result<Vec<_>>>()
                .map_err(SubmitFailure::permanent)?;

            let Err(failed) = hand_over_smtp(client, reverse, forwards, bytes) else {
                return Ok(());
            };
            let code = reply_code(&failed.error);
            let error = anyhow!(failed.error).context("SMTP submission error");
            let failure = smtp_disposition(code, failed.handed_over).with(error);
            // NOTE: a reply leaves the session usable once the transaction
            // is reset; a failure without one leaves it in no known state.
            if code.is_none() {
                channel.lost = Some(format!("{:#}", failure.error()));
            } else if let Err(err) = client.rset() {
                channel.lost = Some(format!("RSET after a refusal failed: {err}"));
            }
            Err(failure)
        }
        #[cfg(feature = "msgraph")]
        Link::Graph(client) => {
            let Err(SendMimeFailure { error, unanswered }) = client.send_mime(&bytes) else {
                return Ok(());
            };
            let status = match &error {
                io_msgraph::v1::client::MsgraphClientStdError::Send(send) => send.status(),
                _ => None,
            };
            let error = anyhow!(error).context("Graph sendMail error");
            let failure = graph_disposition(status, unanswered).with(error);
            if status.is_none() {
                channel.lost = Some(format!("{:#}", failure.error()));
            }
            Err(failure)
        }
        #[cfg(not(feature = "msgraph"))]
        Link::Unused(_) => unreachable!("the placeholder link is never constructed"),
    }
}

/// An SMTP submission that failed, and whether the data's final `.` had
/// been written whole when it did.
#[cfg(feature = "smtp")]
struct SmtpHandOverError {
    error: SmtpClientError,
    handed_over: bool,
}

/// Runs the `MAIL FROM`, `RCPT TO` and `DATA` exchange over the session,
/// noting when the data's final `.` is written whole.
///
/// io-smtp's own driver cannot say where a failure fell, which is what
/// tells a message never handed over from one perhaps taken. Every command
/// is one write: `MAIL FROM`, one `RCPT TO` a recipient, `DATA`, then the
/// dot-stuffed data ending with its `.` line, so the data is the write
/// numbered `rcpts + 2` from zero. The default options remove the `Bcc`
/// field before `DATA`, the envelope alone carrying its recipients
/// (RFC 5322 §3.6.3).
#[cfg(feature = "smtp")]
fn hand_over_smtp(
    client: &mut SmtpClientStd,
    reverse: SmtpReversePath<'static>,
    forwards: Vec<SmtpForwardPath<'static>>,
    message: Vec<u8>,
) -> Result<(), SmtpHandOverError> {
    use std::io::{Read as _, Write as _};

    let data_write = forwards.len() + 2;
    let opts = SmtpMessageSendOptions::default();
    let mut send = SmtpMessageSend::new(reverse, forwards, message, opts);
    let mut buf = vec![0u8; 16 * 1024];
    let mut arg: Option<&[u8]> = None;
    let mut writes = 0;
    let mut handed_over = false;
    let failed = |error: SmtpClientError, handed_over| SmtpHandOverError { error, handed_over };

    loop {
        match send.resume(arg.take()) {
            SmtpCoroutineState::Complete(Ok(())) => return Ok(()),
            SmtpCoroutineState::Complete(Err(err)) => return Err(failed(err.into(), handed_over)),
            SmtpCoroutineState::Yielded(SmtpYield::WantsRead) => {
                let n = client
                    .stream
                    .read(&mut buf)
                    .map_err(|err| failed(err.into(), handed_over))?;
                arg = Some(&buf[..n]);
            }
            SmtpCoroutineState::Yielded(SmtpYield::WantsWrite(bytes)) => {
                // NOTE: flushed, so a TLS layer buffering the tail reports
                // its failure here: a write or a flush that fails leaves
                // the final `.` unsent, and the server drops a transaction
                // whose data never ended (RFC 5321 §4.1.1.4).
                client
                    .stream
                    .write_all(&bytes)
                    .and_then(|()| client.stream.flush())
                    .map_err(|err| failed(err.into(), handed_over))?;
                handed_over |= writes == data_write;
                writes += 1;
            }
        }
    }
}

/// The reply code an SMTP failure carries, `None` when no reply came.
#[cfg(feature = "smtp")]
fn reply_code(err: &SmtpClientError) -> Option<u16> {
    let send = match err {
        SmtpClientError::MessageSend(send) => send,
        SmtpClientError::Mail(SmtpMailError::Rejected { code, .. })
        | SmtpClientError::Rcpt(SmtpRcptError::Rejected { code, .. })
        | SmtpClientError::Data(
            SmtpDataError::CommandRejected { code, .. } | SmtpDataError::BodyRejected { code, .. },
        ) => return Some(*code),
        _ => return None,
    };
    match send {
        SmtpMessageSendError::MailFrom(SmtpMailError::Rejected { code, .. })
        | SmtpMessageSendError::RcptTo(SmtpRcptError::Rejected { code, .. })
        | SmtpMessageSendError::Data(
            SmtpDataError::CommandRejected { code, .. } | SmtpDataError::BodyRejected { code, .. },
        ) => Some(*code),
        _ => None,
    }
}

/// The MAIL FROM reverse path, the null path for an empty sender (bounces).
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

#[cfg(test)]
mod dispositions {
    use super::*;

    #[test]
    fn an_smtp_reply_decides_and_only_silence_after_the_dot_is_uncertain() {
        assert_eq!(smtp_disposition(Some(554), false), Disposition::Permanent);
        assert_eq!(smtp_disposition(Some(550), true), Disposition::Permanent);
        assert_eq!(smtp_disposition(Some(451), false), Disposition::Transient);
        assert_eq!(
            smtp_disposition(Some(452), true),
            Disposition::Transient,
            "a 4yz after the data is a verdict: not taken"
        );
        assert_eq!(smtp_disposition(None, false), Disposition::Transient);
        assert_eq!(smtp_disposition(None, true), Disposition::Uncertain);
    }

    #[test]
    fn a_graph_status_decides_and_silence_after_the_request_is_uncertain() {
        for status in [400, 403, 404, 413, 422, 409] {
            assert_eq!(
                graph_disposition(Some(status), true),
                Disposition::Permanent,
                "{status}"
            );
        }
        for status in [401, 408, 429] {
            assert_eq!(
                graph_disposition(Some(status), true),
                Disposition::Transient,
                "{status}"
            );
        }
        for status in [500, 502, 503, 504] {
            assert_eq!(
                graph_disposition(Some(status), false),
                Disposition::Uncertain,
                "{status}"
            );
        }
        assert_eq!(graph_disposition(Some(507), false), Disposition::Transient);
        assert_eq!(graph_disposition(None, true), Disposition::Uncertain);
        assert_eq!(graph_disposition(None, false), Disposition::Transient);
    }

    #[test]
    fn an_uncertain_failure_parks_under_its_prefix() {
        let failure = Disposition::Uncertain.with(anyhow!("Reached unexpected EOF"));
        assert!(failure.parks());
        assert!(failure.uncertain());
        let reason = failure.reason();
        assert!(reason.starts_with(UNCERTAIN), "{reason}");
        assert!(reason.ends_with("Reached unexpected EOF"), "{reason}");

        let refused = Disposition::Permanent.with(anyhow!("554 no"));
        assert!(refused.parks() && !refused.uncertain());
        assert_eq!(refused.reason(), "554 no");
        let later = Disposition::Transient.with(anyhow!("451 later"));
        assert!(!later.parks() && !later.uncertain());
    }

    #[test]
    fn the_in_flight_mark_survives_until_cleared() {
        let dir = tempfile::tempdir().unwrap();
        let mark = InFlight::new(dir.path());
        assert_eq!(mark.left(), None);

        mark.begin(42).unwrap();
        assert_eq!(mark.left(), Some(42));
        mark.begin(43).unwrap();
        assert_eq!(InFlight::new(dir.path()).left(), Some(43));

        mark.end().unwrap();
        assert_eq!(mark.left(), None);
        mark.end().expect("clearing no mark is fine");
    }
}

#[cfg(all(test, feature = "smtp"))]
mod tests {
    use std::{
        io::{BufRead, BufReader, Read, Write as _},
        net::TcpListener,
        sync::mpsc,
        thread,
    };

    use io_pimdir::{client::blobs::PimdirBlobs, hash::PimdirHashAlgo};

    use super::*;
    use crate::config::SmtpConfig;

    /// What the sink captured: the envelope command lines and the DATA payload.
    struct Captured {
        commands: Vec<String>,
        data: Vec<u8>,
    }

    /// How the sink answers.
    #[derive(Clone, Copy)]
    enum Sink {
        /// Takes every message.
        Accept,
        /// Answers the DATA command with this reply line.
        RejectData(&'static str),
        /// Answers the end of the data with this reply line.
        RejectBody(&'static str),
        /// Reads the whole data, then closes the connection unanswered.
        HangUpAfterData,
        /// Closes the connection on the DATA command, before the data.
        HangUpOnData,
    }

    /// A minimal scripted SMTP sink on a random local port.
    ///
    /// One thread accepts one session, answers the canonical submission
    /// dialogue as `sink` says and captures the envelope and message bytes.
    fn spawn_smtp_sink(sink: Sink) -> (u16, mpsc::Receiver<Captured>) {
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
                } else if upper.starts_with("DATA") {
                    match sink {
                        Sink::RejectData(reply) => {
                            stream.write_all(reply.as_bytes()).expect("reject");
                            continue;
                        }
                        Sink::HangUpOnData => break,
                        _ => {}
                    }
                    stream.write_all(b"354 go\r\n").expect("go");
                    loop {
                        let mut data_line = Vec::new();
                        let mut byte = [0u8; 1];
                        loop {
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
                    match sink {
                        Sink::HangUpAfterData => break,
                        Sink::RejectBody(reply) => {
                            stream.write_all(reply.as_bytes()).expect("reject body")
                        }
                        _ => stream.write_all(b"250 queued\r\n").expect("queued"),
                    }
                } else if upper.starts_with("QUIT") {
                    let _ = stream.write_all(b"221 bye\r\n");
                    break;
                } else {
                    captured.commands.push(line.trim_end().to_owned());
                    stream.write_all(b"250 OK\r\n").expect("ok");
                }
            }
            let _ = tx.send(captured);
        });

        (port, rx)
    }

    /// Stages a body in the blob store and returns the intent pointing at it.
    fn stage_intent(blobs: &PimdirBlobs, id: i64, payload: &str, body: &[u8]) -> SubmitIntent {
        let hash = PimdirHash(format!("hash-{id}"));
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

    fn smtp_account(port: u16) -> SmtpAccount {
        let config: SmtpConfig = toml::from_str(&format!(
            "server = \"smtp://127.0.0.1:{port}\"\nstarttls = false\n"
        ))
        .unwrap();
        SmtpAccount::resolve(&config).expect("resolve sink")
    }

    fn channel_to(port: u16) -> SendChannel<'static> {
        SendChannel::smtp(connect_smtp(&smtp_account(port)).expect("connect sink"))
    }

    fn send_one(
        channel: &mut SendChannel<'_>,
        blobs: &PimdirBlobs,
        intent: &SubmitIntent,
    ) -> Result<(), SubmitFailure> {
        deliver(channel, prepare(blobs, intent)?)
    }

    const PAYLOAD: &str = r#"{"v":1,"from":"a@x.org","rcpts":["b@y.org"],"subject":"hi"}"#;

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

        let (port, captured) = spawn_smtp_sink(Sink::Accept);
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
    fn a_bcc_recipient_is_in_the_envelope_but_not_in_the_transmitted_body() {
        let dir = tempfile::tempdir().unwrap();
        let blobs = PimdirBlobs::open(dir.path(), PimdirHashAlgo::default());
        let body = b"To: b@y.org\r\nBcc: c@y.org\r\nSubject: hi\r\n\r\nhello".to_vec();
        let intent = stage_intent(
            &blobs,
            1,
            r#"{"v":1,"from":"a@x.org","rcpts":["b@y.org","c@y.org"],"subject":"hi"}"#,
            &body,
        );

        let (port, captured) = spawn_smtp_sink(Sink::Accept);
        let mut channel = channel_to(port);
        send_one(&mut channel, &blobs, &intent).expect("send");
        channel.close();

        let captured = captured.recv().expect("captured session");
        assert!(
            captured
                .commands
                .contains(&String::from("RCPT TO:<c@y.org>"))
        );
        assert_eq!(
            captured.data,
            b"To: b@y.org\r\nSubject: hi\r\n\r\nhello\r\n".as_slice()
        );
    }

    #[test]
    fn a_5xx_rejection_parks_the_intent_and_a_4xx_one_keeps_it() {
        let dir = tempfile::tempdir().unwrap();
        let blobs = PimdirBlobs::open(dir.path(), PimdirHashAlgo::default());

        let intent = stage_intent(&blobs, 1, PAYLOAD, b"body");
        let (port, _) = spawn_smtp_sink(Sink::RejectData("554 rejected\r\n"));
        let mut channel = channel_to(port);
        let failure = send_one(&mut channel, &blobs, &intent).expect_err("rejected");
        assert!(failure.parks(), "5xx must park: {}", failure.error());
        assert!(!failure.uncertain());
        assert!(
            channel.lost().is_none(),
            "a reply leaves the session usable"
        );

        let intent = stage_intent(&blobs, 2, PAYLOAD, b"body");
        let (port, _) = spawn_smtp_sink(Sink::RejectData("451 try later\r\n"));
        let mut channel = channel_to(port);
        let failure = send_one(&mut channel, &blobs, &intent).expect_err("deferred");
        assert!(!failure.parks(), "4xx must retry: {}", failure.error());
        assert!(!failure.uncertain());
    }

    #[test]
    fn a_reply_to_the_data_is_a_verdict_not_an_uncertainty() {
        let dir = tempfile::tempdir().unwrap();
        let blobs = PimdirBlobs::open(dir.path(), PimdirHashAlgo::default());

        let intent = stage_intent(&blobs, 1, PAYLOAD, b"body");
        let (port, _) = spawn_smtp_sink(Sink::RejectBody("452 mailbox full\r\n"));
        let mut channel = channel_to(port);
        let failure = send_one(&mut channel, &blobs, &intent).expect_err("deferred");
        assert!(
            matches!(failure, SubmitFailure::Transient(_)),
            "{failure:?}"
        );

        let intent = stage_intent(&blobs, 2, PAYLOAD, b"body");
        let (port, _) = spawn_smtp_sink(Sink::RejectBody("552 too large\r\n"));
        let mut channel = channel_to(port);
        let failure = send_one(&mut channel, &blobs, &intent).expect_err("refused");
        assert!(
            matches!(failure, SubmitFailure::Permanent(_)),
            "{failure:?}"
        );
    }

    #[test]
    fn a_hang_up_after_the_dot_is_uncertain_and_before_it_is_not() {
        let dir = tempfile::tempdir().unwrap();
        let blobs = PimdirBlobs::open(dir.path(), PimdirHashAlgo::default());

        let intent = stage_intent(&blobs, 1, PAYLOAD, b"body");
        let (port, captured) = spawn_smtp_sink(Sink::HangUpAfterData);
        let mut channel = channel_to(port);
        let failure = send_one(&mut channel, &blobs, &intent).expect_err("no verdict");
        assert!(failure.uncertain(), "{failure:?}");
        assert!(failure.parks());
        assert!(channel.lost().is_some(), "the session is gone");
        assert_eq!(captured.recv().unwrap().data, b"body\r\n");

        // NOTE: the next message is not attempted on a lost session.
        let next = stage_intent(&blobs, 2, PAYLOAD, b"body");
        let failure = send_one(&mut channel, &blobs, &next).expect_err("not attempted");
        assert!(
            matches!(failure, SubmitFailure::Transient(_)),
            "{failure:?}"
        );
        channel.close();

        let intent = stage_intent(&blobs, 3, PAYLOAD, b"body");
        let (port, _) = spawn_smtp_sink(Sink::HangUpOnData);
        let mut channel = channel_to(port);
        let failure = send_one(&mut channel, &blobs, &intent).expect_err("cut before data");
        assert!(
            matches!(failure, SubmitFailure::Transient(_)),
            "{failure:?}"
        );
        assert!(channel.lost().is_some());
    }

    #[test]
    fn a_refusal_resets_the_transaction_for_the_next_message() {
        let dir = tempfile::tempdir().unwrap();
        let blobs = PimdirBlobs::open(dir.path(), PimdirHashAlgo::default());

        let intent = stage_intent(&blobs, 1, PAYLOAD, b"body");
        let (port, captured) = spawn_smtp_sink(Sink::RejectData("554 rejected\r\n"));
        let mut channel = channel_to(port);
        send_one(&mut channel, &blobs, &intent).expect_err("rejected");
        channel.close();

        let captured = captured.recv().unwrap();
        assert_eq!(captured.commands.last().map(String::as_str), Some("RSET"));
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
        let failure = prepare(&blobs, &bodyless).err().expect("no body to send");
        assert!(failure.parks() && !failure.uncertain());
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

    #[test]
    fn a_closed_port_is_unreached() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let err = connect_smtp(&smtp_account(port)).expect_err("nothing listens");
        assert_eq!(open_failure(&err), OpenFailure::Unreached, "{err:#}");
    }

    /// A server answering `AUTH` with `reply`, then closing.
    fn spawn_auth_sink(reply: &'static str) -> u16 {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind sink");
        let port = listener.local_addr().expect("sink addr").port();
        thread::spawn(move || {
            let (stream, _) = listener.accept().expect("accept");
            let mut reader = BufReader::new(stream.try_clone().expect("clone"));
            let mut stream = stream;
            stream.write_all(b"220 sink\r\n").unwrap();
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 {
                    break;
                }
                let upper = line.to_ascii_uppercase();
                if upper.starts_with("EHLO") {
                    stream.write_all(b"250-sink\r\n250 AUTH PLAIN\r\n").unwrap();
                } else if upper.starts_with("AUTH") {
                    let _ = stream.write_all(reply.as_bytes());
                    break;
                } else {
                    stream.write_all(b"250 OK\r\n").unwrap();
                }
            }
        });
        port
    }

    fn authenticated_account(port: u16) -> SmtpAccount {
        let config: SmtpConfig = toml::from_str(&format!(
            "server = \"smtp://127.0.0.1:{port}\"\n\
             starttls = false\n\
             sasl.plain.username = \"user\"\n\
             sasl.plain.password.raw = \"secret\"\n"
        ))
        .unwrap();
        SmtpAccount::resolve(&config).expect("resolve sink")
    }

    #[test]
    fn refused_credentials_are_not_an_unreached_server() {
        let port = spawn_auth_sink("535 5.7.8 authentication failed\r\n");
        let err = connect_smtp(&authenticated_account(port)).expect_err("credentials refused");
        assert_eq!(open_failure(&err), OpenFailure::Refused, "{err:#}");

        // NOTE: a server gone mid-authentication answered nothing.
        let port = spawn_auth_sink("");
        let err = connect_smtp(&authenticated_account(port)).expect_err("session cut");
        assert_eq!(open_failure(&err), OpenFailure::Unreached, "{err:#}");
    }
}
