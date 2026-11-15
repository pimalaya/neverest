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

use anyhow::{Result, anyhow};
use io_imap::{
    client::{ImapClient as _, ImapClientStd as Inner, ImapClientStdConnectOptions},
    rfc3501::select::ImapMailboxSelectData,
    send::ImapOutput,
    session::ImapSessionOpenOptions,
    types::{
        IntoStatic,
        core::{Atom, IString, NString, Vec1},
        extensions::enable::CapabilityEnable,
        mailbox::Mailbox,
        response::Capability,
    },
};
#[cfg(test)]
use io_sasl::mechanism::Sasl;
use log::warn;
#[cfg(test)]
use pimalaya_stream::{proxy::Proxy, tls::Tls};
#[cfg(test)]
use url::Url;

use crate::{account::ImapAccount, config::ImapIdConfig};

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
    /// The UIDs a round still has to list, kept from the search its first
    /// page made so a later page of this run searches nothing.
    pub(crate) round: Option<RoundUids>,
    /// What the connection opened from, kept to open it again.
    opening: ImapAccount,
}

/// The UIDs left to list in a round of one mailbox, newest first.
#[derive(Debug)]
pub(crate) struct RoundUids {
    /// The mailbox the round lists.
    pub mailbox: String,
    /// The `UIDVALIDITY` the UIDs belong to.
    pub uid_validity: u32,
    /// The scope the search narrowed to, as `(since, until)`.
    pub scope: (Option<String>, Option<String>),
    /// The UIDs left, highest first.
    pub left: Vec<u32>,
}

impl ImapClient {
    /// Opens a direct IMAP connection to `server`, with no `ID` and the
    /// advertised `SASL-IR`: what a test server needs.
    #[cfg(test)]
    pub fn connect(server: &Url, tls: &Tls, starttls: bool, sasl: Option<Sasl>) -> Result<Self> {
        Self::open(&ImapAccount {
            server: server.clone(),
            tls: tls.clone(),
            starttls,
            sasl,
            proxy: Proxy::None,
            sasl_ir: None,
            auto_id: None,
        })
    }

    /// Opens the IMAP connection (proxy, TCP/TLS/STARTTLS, greeting, SASL,
    /// then the `ID` the account asks for), the ALPN identifiers riding its
    /// [`Tls`].
    pub fn open(account: &ImapAccount) -> Result<Self> {
        let opening = account.clone();
        let opts = ImapClientStdConnectOptions {
            tls: account.tls.clone(),
            proxy: account.proxy.clone(),
            sasl: account.sasl.clone(),
            session: ImapSessionOpenOptions {
                starttls: account.starttls,
                auto_id: account.auto_id.clone(),
                sasl_ir: account.sasl_ir,
                ..Default::default()
            },
        };
        let (inner, mut capabilities) = Inner::connect(&account.server, opts)?;
        // NOTE: a debug build can be told to read the server as one without
        // QRESYNC, which the integration tests take to list band heads
        // against a server offering it; a release build never reads it.
        if cfg!(debug_assertions) && std::env::var_os("NEVEREST_IMAP_NO_QRESYNC").is_some() {
            capabilities.retain(|capability| *capability != Capability::QResync);
        }
        let mut client = Self {
            inner,
            capabilities,
            selected: None,
            round: None,
            opening,
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

    /// Drops the connection and opens it again as it first opened, nothing
    /// selected; the UIDs a round has left are kept.
    pub fn reconnect(&mut self) -> Result<()> {
        let mut fresh = Self::open(&self.opening)?;
        fresh.round = self.round.take();
        *self = fresh;

        Ok(())
    }

    /// Whether the server advertises QRESYNC (RFC 7162).
    pub fn supports_qresync(&self) -> bool {
        self.capabilities.contains(&Capability::QResync)
    }

    /// Whether the server takes `UID EXPUNGE`: it advertises UIDPLUS
    /// (RFC 4315) or IMAP4rev2 (RFC 9051, where the command is built in).
    pub fn supports_uid_expunge(&self) -> bool {
        supports_uid_expunge(&self.capabilities)
    }

    /// A QRESYNC `SELECT (QRESYNC (uid_validity highest_mod_seq))`: the server
    /// returns only what changed since `highest_mod_seq` plus vanished UIDs.
    pub fn select_delta(
        &mut self,
        mailbox: Mailbox<'static>,
        uid_validity: NonZeroU32,
        highest_mod_seq: u64,
    ) -> Result<ImapOutput<ImapMailboxSelectData>> {
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

/// Whether `capabilities` hold UIDPLUS or IMAP4rev2, both of which carry
/// `UID EXPUNGE`. imap-types names no IMAP4rev2 variant, so it is read as
/// the atom, case-insensitively (RFC 9051 §7.2.2).
fn supports_uid_expunge(capabilities: &[Capability<'_>]) -> bool {
    capabilities.iter().any(|capability| {
        *capability == Capability::UidPlus
            || capability.to_string().eq_ignore_ascii_case("IMAP4rev2")
    })
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

/// The parameters the automatic `ID` (RFC 2971) of `config` sends, `None`
/// when `auto` is off.
///
/// A key set to `true` takes neverest's canned value, or `NIL` with a
/// warning when there is none; one set to `false` sends `NIL`.
pub fn auto_id(config: &ImapIdConfig) -> Result<Option<Vec<(IString<'static>, NString<'static>)>>> {
    if !config.auto {
        return Ok(None);
    }

    let mut params = Vec::with_capacity(config.fields.len());
    for (key, &canned) in &config.fields {
        let ikey = IString::try_from(key.clone())
            .map_err(|err| anyhow!("Invalid IMAP ID parameter key `{key}`: {err}"))?
            .into_static();

        let value = match canned.then(|| canned_id_value(key)).flatten() {
            Some(value) => NString::try_from(value)
                .map_err(|err| anyhow!("Invalid IMAP ID value `{value}` for `{key}`: {err}"))?
                .into_static(),
            None => {
                if canned {
                    warn!("imap.id.fields.{key} = true: no canned value defined, sending NIL");
                }
                NString::NIL
            }
        };

        params.push((ikey, value));
    }

    Ok(Some(params))
}

/// Neverest's value for a well-known `ID` key.
fn canned_id_value(key: &str) -> Option<&'static str> {
    match key {
        "name" => Some(env!("CARGO_PKG_NAME")),
        "version" => Some(env!("CARGO_PKG_VERSION")),
        "vendor" => Some("Pimalaya"),
        "support-url" => Some("https://github.com/pimalaya/neverest"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caps(atoms: &[&'static str]) -> Vec<Capability<'static>> {
        atoms
            .iter()
            .map(|atom| Capability::from(Atom::try_from(*atom).unwrap()))
            .collect()
    }

    #[test]
    fn uid_expunge_needs_uidplus_or_imap4rev2() {
        assert!(supports_uid_expunge(&caps(&["IMAP4rev1", "UIDPLUS"])));
        assert!(supports_uid_expunge(&caps(&["IMAP4rev2"])));
        assert!(supports_uid_expunge(&caps(&["imap4rev2"])));
        assert!(!supports_uid_expunge(&caps(&["IMAP4rev1", "MOVE", "IDLE"])));
    }
}
