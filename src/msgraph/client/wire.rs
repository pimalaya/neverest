//! # Graph connection
//!
//! What a [`GraphClient`](super::GraphClient) knows of the one HTTP/1.1
//! connection it runs its requests over. Graph closes a connection it left
//! idle after about two minutes, and an answer that comes late, or that was
//! not read to its end, would be read as the next request's: so the stream
//! is watched ([`Watched`]), recording what each exchange wrote and whether
//! any byte of its answer came, and the session drops a connection whose
//! last exchange failed in any way but a whole answer, one that idled past
//! [`IDLE`], or one the server ended.
//!
//! A request that met a reused connection already dead (reset, broken pipe,
//! end of stream before any byte of an answer) never reached the server: a
//! read is sent again once on a new connection (RFC 9112 section 9.3.1). A
//! read that timed out may be a server still at work, and is never sent
//! again; nor is a request that creates, sends or moves.
//!
//! The record also says whether a request went whole ([`Exchange::written`]):
//! a `sendMail` failing past that point with no status may have been
//! delivered, one failing before it was not.

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
    /// Whether the request went whole: the exchange asked for its answer,
    /// the stream flushed. From then on the server may have acted on it,
    /// whatever comes back or fails to.
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

    /// Whether the request was written whole and flushed before the
    /// exchange read: a failure past it, with no status read, cannot tell
    /// a request the server never acted on from one it did.
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
        // NOTE: the first read of an exchange follows its whole request;
        // flushed first, so a TLS layer holding the tail fails here, the
        // request not written whole.
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
        // NOTE: the head is what the exchange meant to send, kept even when
        // the write fails on a connection already closed.
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

/// Whether an exchange that failed with `err` read its answer whole, which
/// leaves the connection fit for the next one: an answer Graph refused (its
/// error envelope), one that did not parse, a redirect.
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

/// Whether `err` is a connection found dead before its answer began: reset,
/// aborted, a broken pipe, or the end of the stream (an I/O error, or the
/// HTTP exchange meeting it) with no octet of an answer read. Never a read
/// that timed out, which may be a server still at work on the request.
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
        // NOTE: io-msgraph names no HTTP failure but through io-http's own
        // error, which this crate does not link for Graph: the stream says
        // the exchange met its end.
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
