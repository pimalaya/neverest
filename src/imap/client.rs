//! IMAP client wrapper. Opens the connection (TCP/TLS/STARTTLS,
//! greeting, SASL) eagerly so any auth or network error surfaces
//! before the sync engine starts walking mailboxes.

use anyhow::Result;
use io_imap::client::ImapClientStd as Inner;
use pimalaya_stream::{sasl::Sasl, std::stream::StreamStd, tls::Tls};
use url::Url;

use crate::{account::context::Account, config::ImapConfig};

pub struct ImapClient {
    inner: Inner<StreamStd>,
    #[allow(dead_code)]
    pub account: Account,
}

impl ImapClient {
    pub fn new(config: ImapConfig, account: Account) -> Result<Self> {
        let mut tls: Tls = config.tls.into();
        tls.rustls.alpn = vec!["imap".into()];
        let sasl: Option<Sasl> = config.sasl.map(Sasl::try_from).transpose()?;
        let server = parse_imap_server(&config.server)?;
        let inner = Inner::<StreamStd>::connect(&server, &tls, config.starttls, sasl)?;
        Ok(Self { inner, account })
    }

    /// Hands the underlying io-imap client to
    /// [`io_email::client::EmailClientStd::with_imap`]. Discards the
    /// wrapped [`Account`] because the sync engine reads the merged
    /// account context from [`crate::side::SideClient`] earlier (this
    /// drop just unlocks the move).
    pub fn into_inner(self) -> Inner<StreamStd> {
        self.inner
    }
}

/// Parses an IMAP server string into a URL. Bare authorities default
/// to `imaps://`; explicit `imap://`/`imaps://` URLs are used as-is.
pub fn parse_imap_server(server: &str) -> Result<Url> {
    match Url::parse(server) {
        Ok(url) => Ok(url),
        Err(url::ParseError::RelativeUrlWithoutBase) => {
            Ok(Url::parse(&format!("imaps://{server}"))?)
        }
        Err(err) => Err(err.into()),
    }
}
