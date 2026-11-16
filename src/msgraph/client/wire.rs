//! # Graph connection
//!
//! What a [`GraphClient`](super::GraphClient) knows of its one HTTP/1.1
//! connection, watched ([`Watched`]) so that a late or unread answer is never
//! read as the next request's.
//!
//! The session drops a connection whose last exchange failed short of a whole
//! answer, that idled past [`IDLE`] (Graph closes one after about two
//! minutes), or that the server ended.
//!
//! A reused connection found dead before any octet of an answer never reached
//! the server: a read is sent again once (RFC 9112 section 9.3.1). A read that
//! timed out, or a request that creates, sends or moves, never is.
//!
//! [`Exchange::written`] says whether a request went whole: a `sendMail`
//! failing past that point with no status may have been delivered.

use std::{
    io::{self, ErrorKind, Read, Write},
    sync::{Arc, Mutex},
    time::Duration,
};

use io_msgraph::v1::{
    client::{MsgraphClientStdError, MsgraphStream},
    send::MsgraphSendError,
};

/// How long a connection may idle before it is dropped rather than reused:
/// well below the two minutes after which Graph closes one.
pub(super) const IDLE: Duration = Duration::from_secs(60);

/// Opens a new connection to the Graph API.
pub(super) type Connect = Box<dyn FnMut() -> anyhow::Result<Box<dyn MsgraphStream>> + Send>;

/// What the exchange in flight did on its stream.
#[derive(Debug, Default)]
pub(super) struct Exchange {
    /// The first octets the exchange wrote: its request line's method.
    head: Vec<u8>,
    /// Whether the request went whole (answer asked for, stream flushed): from
    /// then on the server may have acted on it, whatever comes back.
    written: bool,
    /// Whether any octet of an answer came.
    heard: bool,
    /// Whether a read met the end of the stream.
    ended: bool,
}

impl Exchange {
    /// Whether the request in flight is a `GET`, which reads only.
    pub(super) fn reads(&self) -> bool {
        self.head.starts_with(b"GET ")
    }

    /// Whether the request was written whole and flushed before the read.
    ///
    /// A failure past it, with no status read, cannot tell a request the
    /// server never acted on from one it did.
    pub(super) fn written(&self) -> bool {
        self.written
    }

    /// Whether any octet of an answer came.
    pub(super) fn heard(&self) -> bool {
        self.heard
    }

    /// Whether a read met the end of the stream: the server closed the
    /// connection, whatever its answer said.
    pub(super) fn ended(&self) -> bool {
        self.ended
    }
}

/// The exchange record a [`Watched`] stream writes and its session reads.
pub(super) type Shared = Arc<Mutex<Exchange>>;

/// A stream recording what its exchange wrote and read.
///
/// The first read flushes the request, so a TLS layer holding its tail fails
/// before it counts as written. The head is kept even when a write fails on a
/// connection already closed.
pub(super) struct Watched<S> {
    inner: S,
    exchange: Shared,
}

impl<S> Watched<S> {
    pub(super) fn new(inner: S, exchange: Shared) -> Self {
        Self { inner, exchange }
    }
}

impl<S: Read + Write> Read for Watched<S> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let written = self
            .exchange
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .written;
        if !written {
            self.inner.flush()?;
            let mut exchange = self.exchange.lock().unwrap_or_else(|err| err.into_inner());
            exchange.written = !exchange.head.is_empty();
        }
        let read = self.inner.read(buf)?;
        let mut exchange = self.exchange.lock().unwrap_or_else(|err| err.into_inner());
        match read {
            0 => exchange.ended = true,
            _ => exchange.heard = true,
        }
        Ok(read)
    }
}

impl<S: Write> Write for Watched<S> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        {
            let mut exchange = self.exchange.lock().unwrap_or_else(|err| err.into_inner());
            if exchange.head.is_empty() {
                exchange.head = buf[..buf.len().min(8)].to_vec();
            }
        }
        self.inner.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

/// Clears the record before a new exchange.
pub(super) fn begin(exchange: &Shared) {
    *exchange.lock().unwrap_or_else(|err| err.into_inner()) = Exchange::default();
}

/// Reads the record of the exchange that just ran.
pub(super) fn read<T>(exchange: &Shared, f: impl FnOnce(&Exchange) -> T) -> T {
    f(&exchange.lock().unwrap_or_else(|err| err.into_inner()))
}

/// Whether an exchange that failed with `err` read its answer whole.
///
/// That leaves the connection fit for the next one: an answer Graph refused
/// (its error envelope), one that did not parse, a redirect.
pub(super) fn answered(err: &MsgraphClientStdError) -> bool {
    matches!(
        err,
        MsgraphClientStdError::Send(
            MsgraphSendError::Api(_)
                | MsgraphSendError::ParseResponse(_)
                | MsgraphSendError::UnexpectedRedirect
        )
    )
}

/// Whether `err` is a connection found dead before any octet of an answer.
///
/// Reset, aborted, a broken pipe or the end of the stream; never a timed-out
/// read, the server maybe still at work. An HTTP failure counts by the
/// stream's end, io-http's own error not being linked here.
pub(super) fn died(err: &MsgraphClientStdError, exchange: &Exchange) -> bool {
    if exchange.heard() {
        return false;
    }
    match err {
        MsgraphClientStdError::Io(err) => matches!(
            err.kind(),
            ErrorKind::ConnectionReset
                | ErrorKind::ConnectionAborted
                | ErrorKind::BrokenPipe
                | ErrorKind::UnexpectedEof
                | ErrorKind::NotConnected
        ),
        MsgraphClientStdError::Send(MsgraphSendError::Send(_)) => exchange.ended(),
        _ => false,
    }
}

/// Whether `err` is a read that waited past
/// [`READ_TIMEOUT`](crate::http::READ_TIMEOUT).
pub(super) fn timed_out(err: &MsgraphClientStdError) -> bool {
    matches!(
        err,
        MsgraphClientStdError::Io(err)
            if matches!(err.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut)
    )
}
