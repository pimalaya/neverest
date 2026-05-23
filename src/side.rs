//! Per-side abstraction.
//!
//! [`Side`] tags which half of the sync a value belongs to (used by progress
//! events, hunks, and the cache). [`Side::open`] is the single entry point that
//! consumes a [`SideConfig`] and returns a ready-to-use [`EmailClientStd`]:
//! TLS/SASL/JMAP-session/m2dir-open setup is inlined into the
//! [`EmailClientStd::new`] builder chain so the sync engine only ever talks to
//! the shared dispatcher.

use std::fmt;

use anyhow::{Context, Result, bail};
#[cfg(feature = "jmap")]
use base64::{Engine, prelude::BASE64_STANDARD};
use io_email::client::EmailClientStd;
#[cfg(feature = "imap")]
use io_imap::client::ImapClientStd;
#[cfg(feature = "jmap")]
use io_jmap::client::JmapClientStd;
#[cfg(feature = "m2dir")]
use io_m2dir::client::M2dirClient;
#[cfg(feature = "imap")]
use pimalaya_stream::sasl::Sasl;
#[cfg(any(feature = "imap", feature = "jmap"))]
use pimalaya_stream::tls::Tls;
#[cfg(feature = "jmap")]
use secrecy::ExposeSecret;
use serde::{Deserialize, Serialize};
#[cfg(any(feature = "imap", feature = "jmap"))]
use url::{ParseError, Url};

#[cfg(feature = "jmap")]
use crate::config::JmapAuthConfig;
use crate::config::SideConfig;

/// Which half of the sync a value belongs to. Pure tag; carried by hunks,
/// events, cache entries.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Side {
    Left,
    Right,
}

impl Side {
    /// Picks the matching client out of a `(left, right)` mutable pair.
    /// Used by hunk apply paths so the engine never branches on `Side`
    /// itself.
    pub fn client_mut<'a>(
        self,
        left: &'a mut EmailClientStd,
        right: &'a mut EmailClientStd,
    ) -> &'a mut EmailClientStd {
        match self {
            Side::Left => left,
            Side::Right => right,
        }
    }

    /// Picks the `(source, target)` client pair out of a `(left, right)`
    /// mutable pair for a `Copy` hunk. Source and target must differ;
    /// `diff_messages` only emits `Copy` hunks with distinct sides.
    pub fn pair_mut<'a>(
        source: Side,
        target: Side,
        left: &'a mut EmailClientStd,
        right: &'a mut EmailClientStd,
    ) -> (&'a mut EmailClientStd, &'a mut EmailClientStd) {
        debug_assert_ne!(source, target, "copy hunks have distinct sides");
        match (source, target) {
            (Side::Left, Side::Right) => (left, right),
            (Side::Right, Side::Left) => (right, left),
            _ => unreachable!("copy hunks have distinct sides"),
        }
    }

    /// Same as [`Side::open`] but bootstraps any side-local state the sync run
    /// will expect later.
    ///
    /// For m2dir, calls [`io_m2dir::client::M2dirClient::init_store`] to create
    /// the root directory and write the `.m2store` marker (idempotent at the
    /// io-m2dir level; neverest's "already initialized" check lives at the
    /// cache-file layer in [`crate::account::init`]). For IMAP/JMAP the
    /// connection handshake doubles as the init: CAPABILITY (IMAP) and session
    /// GET (JMAP) both run inside [`Side::open`].
    pub fn init(self, config: SideConfig) -> Result<EmailClientStd> {
        #[cfg(feature = "m2dir")]
        if let SideConfig::M2dir(config) = &config {
            let client = M2dirClient::new(config.root.to_string_lossy().into_owned());
            client.init_store().context(format!(
                "Initialize m2store at {} for the {self} side",
                client.root()
            ))?;
        }

        self.open(config)
    }

    /// Opens the protocol client matching the populated slot of `side_config`
    /// and registers it onto a fresh [`EmailClientStd`].
    ///
    /// Exactly one of `imap`, `jmap`, `m2dir` must be set; the error names
    /// `side` so the user knows which half of the config to fix. TLS, SASL,
    /// JMAP session resolution and m2dir client construction are inlined here
    /// so there is no intermediate wrapper between the user config and the
    /// shared dispatcher.
    ///
    /// For m2dir sides, the store root must already exist (sync precondition).
    /// Use [`Side::init`] from `neverest init` to create it.
    pub fn open(self, config: SideConfig) -> Result<EmailClientStd> {
        match config {
            #[cfg(feature = "imap")]
            SideConfig::Imap(config) => {
                let mut tls = Tls::from(config.tls);
                tls.rustls.alpn = vec!["imap".into()];

                let sasl = config.sasl.map(Sasl::try_from).transpose()?;

                let server = match Url::parse(&config.server) {
                    Ok(url) => url,
                    Err(ParseError::RelativeUrlWithoutBase) => {
                        Url::parse(&format!("imaps://{}", &config.server))?
                    }
                    Err(err) => {
                        return Err(err.into());
                    }
                };

                let client = ImapClientStd::connect(&server, &tls, config.starttls, sasl)?;

                Ok(EmailClientStd::new().with_imap(client))
            }
            #[cfg(feature = "jmap")]
            SideConfig::Jmap(config) => {
                let mut tls = Tls::from(config.tls);
                tls.rustls.alpn = vec!["http/1.1".into()];

                let http_auth = match config.auth {
                    JmapAuthConfig::Header(token) => token.get()?,
                    JmapAuthConfig::Bearer { token } => {
                        let token = token.get()?;
                        format!("Bearer {}", token.expose_secret()).into()
                    }
                    JmapAuthConfig::Basic { username, password } => {
                        let creds = format!("{}:{}", username, password.get()?.expose_secret());
                        let encoded = BASE64_STANDARD.encode(creds.into_bytes());
                        format!("Basic {encoded}").into()
                    }
                };

                let url = match Url::parse(&config.server) {
                    Ok(url) => url,
                    Err(ParseError::RelativeUrlWithoutBase) => {
                        Url::parse(&format!("https://{}", config.server))?
                    }
                    Err(err) => {
                        return Err(err.into());
                    }
                };

                let mut client = JmapClientStd::connect(&url, &tls, http_auth)?;
                client.session_get(&url)?;

                Ok(EmailClientStd::new().with_jmap(client))
            }
            #[cfg(feature = "m2dir")]
            SideConfig::M2dir(config) => {
                let client = M2dirClient::new(config.root.to_string_lossy().into_owned());
                Ok(EmailClientStd::new().with_m2dir(client))
            }
            #[allow(unreachable_patterns)]
            _ => bail!("The {self} side requires a backend feature that is not compiled in"),
        }
    }
}

impl fmt::Display for Side {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Left => write!(f, "left"),
            Self::Right => write!(f, "right"),
        }
    }
}
