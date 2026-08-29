//! # IMAP client
//!
//! Wrapper around [`io_imap::client::ImapClientStd`], opened once by the sync
//! engine and reached from the sibling `backend` adapter through the
//! [`Deref`]/[`DerefMut`] passthrough. Construction takes the already-resolved
//! connect arguments, so this module stays config-agnostic.

use std::{
    num::NonZeroU32,
    ops::{Deref, DerefMut},
};

use anyhow::Result;
use io_imap::{
    client::{ImapClient as _, ImapClientStd as Inner},
    rfc3501::select::ImapMailboxSelectData,
    session::ImapSessionOpenOptions,
    types::{
        core::{Atom, Vec1},
        extensions::enable::CapabilityEnable,
        mailbox::Mailbox,
        response::Capability,
    },
};
use io_sasl::mechanism::Sasl;
use pimalaya_stream::tls::Tls;
use url::Url;

/// Live IMAP client wrapping the io-imap session.
///
/// It keeps the server capabilities so the sync can pick QRESYNC/CONDSTORE
/// enumeration, and ENABLEs QRESYNC on connect: RFC 7162 requires ENABLE
/// before the QRESYNC SELECT parameter.
pub struct ImapClient {
    inner: Inner,
    capabilities: Vec<Capability<'static>>,
    /// The mailbox currently SELECTed on this connection.
    ///
    /// A run of fetches on one mailbox then re-SELECTs once rather than per
    /// command. Every select path records it here, so a cached skip is always
    /// correct.
    selected: Option<String>,
}

impl ImapClient {
    /// Opens the IMAP connection (TCP/TLS/STARTTLS, greeting, SASL), the ALPN
    /// identifiers riding the passed [`Tls`].
    pub fn connect(server: &Url, tls: &Tls, starttls: bool, sasl: Option<Sasl>) -> Result<Self> {
        let opts = ImapSessionOpenOptions {
            starttls,
            ..Default::default()
        };
        let (inner, capabilities) = Inner::connect(server, tls, sasl, opts)?;
        let mut client = Self {
            inner,
            capabilities,
            selected: None,
        };
        if client.supports_qresync() {
            let condstore = CapabilityEnable::CondStore;
            let qresync = CapabilityEnable::from(
                Atom::try_from("QRESYNC").expect("`QRESYNC` is a valid IMAP atom"),
            );
            let caps = Vec1::try_from(vec![condstore, qresync]).expect("two is non-empty");
            client.inner.enable(caps)?;
        }
        Ok(client)
    }

    /// Whether the server advertises QRESYNC (RFC 7162).
    pub fn supports_qresync(&self) -> bool {
        self.capabilities.contains(&Capability::QResync)
    }

    /// A QRESYNC `SELECT (QRESYNC (uid_validity highest_mod_seq))`: the server
    /// returns only what changed since `highest_mod_seq` plus vanished UIDs.
    pub fn select_delta(
        &mut self,
        mailbox: Mailbox<'static>,
        uid_validity: NonZeroU32,
        highest_mod_seq: u64,
    ) -> Result<ImapMailboxSelectData> {
        Ok(self
            .inner
            .select_qresync(mailbox, uid_validity, highest_mod_seq, &self.capabilities)?)
    }

    /// Records `mailbox` as the connection's current selection, after every
    /// successful select of it.
    pub fn mark_selected(&mut self, mailbox: &str) {
        self.selected = Some(mailbox.to_string());
    }

    /// Whether `mailbox` is the connection's current selection.
    pub fn is_selected(&self, mailbox: &str) -> bool {
        self.selected.as_deref() == Some(mailbox)
    }

    /// Lightweight liveness check: issues an IMAP `NOOP` round-trip.
    #[allow(dead_code)]
    pub fn ping(&mut self) -> Result<()> {
        self.inner.noop()?;
        Ok(())
    }
}

impl Deref for ImapClient {
    type Target = Inner;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl DerefMut for ImapClient {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}
