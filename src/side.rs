//! Per-side abstraction.
//!
//! [`Side`] tags which half of the sync a value belongs to (used by
//! progress events, hunks, and the cache). [`SideClient`] holds the
//! protocol-specific client wrapper for that side, and exposes a
//! single [`SideClient::into_email_client`] entry point so the sync
//! engine only ever talks to [`io_email::client::EmailClientStd`].

use std::fmt;

use anyhow::{Result, bail};
use io_email::client::EmailClientStd;
use serde::{Deserialize, Serialize};

#[cfg(feature = "imap")]
use crate::imap::client::ImapClient;
#[cfg(feature = "jmap")]
use crate::jmap::client::JmapClient;
#[cfg(feature = "maildir")]
use crate::maildir::client::MaildirClient;
use crate::{
    account::context::Account,
    config::{AccountConfig, Config, SideConfig},
};

/// Which half of the sync a value belongs to. Pure tag; carried by
/// hunks, events, cache entries.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Side {
    Left,
    Right,
}

impl fmt::Display for Side {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Left => f.write_str("left"),
            Self::Right => f.write_str("right"),
        }
    }
}

/// Protocol-specific client wrapper for one side. Constructed via
/// [`SideClient::open`] and consumed by
/// [`SideClient::into_email_client`] right before the sync engine
/// starts dispatching ops through [`EmailClientStd`].
pub enum SideClient {
    #[cfg(feature = "imap")]
    Imap(ImapClient),
    #[cfg(feature = "jmap")]
    Jmap(JmapClient),
    #[cfg(feature = "maildir")]
    Maildir(MaildirClient),
}

impl SideClient {
    /// Opens the protocol client matching the populated slot of
    /// `side_config`. Exactly one of `imap`, `jmap`, `maildir` must be
    /// set; the error names `side` so the user knows which half of the
    /// config to fix.
    pub fn open(side_config: SideConfig, account: Account, side: Side) -> Result<Self> {
        let SideConfig {
            imap,
            jmap,
            maildir,
            ..
        } = side_config;

        let mut populated = 0;
        if imap.is_some() {
            populated += 1;
        }
        if jmap.is_some() {
            populated += 1;
        }
        if maildir.is_some() {
            populated += 1;
        }

        if populated == 0 {
            bail!("no backend configured for the {side} side");
        }
        if populated > 1 {
            bail!(
                "multiple backends configured for the {side} side; pick exactly one of imap/jmap/maildir"
            );
        }

        #[cfg(feature = "imap")]
        if let Some(config) = imap {
            return Ok(Self::Imap(ImapClient::new(config, account)?));
        }

        #[cfg(feature = "jmap")]
        if let Some(config) = jmap {
            return Ok(Self::Jmap(JmapClient::new(config, account)?));
        }

        #[cfg(feature = "maildir")]
        if let Some(config) = maildir {
            return Ok(Self::Maildir(MaildirClient::new(config, account)));
        }

        bail!("the {side} side requires a backend feature that is not compiled in");
    }

    /// Registers the protocol client onto a fresh [`EmailClientStd`]
    /// so the sync engine can dispatch every op through the shared
    /// surface (`list_mailboxes`, `list_envelopes`, `get_message`,
    /// `add_message`, `add_flags`/`delete_flags`).
    pub fn into_email_client(self) -> EmailClientStd {
        let client = EmailClientStd::new();
        match self {
            #[cfg(feature = "imap")]
            Self::Imap(c) => client.with_imap(c.into_inner()),
            #[cfg(feature = "jmap")]
            Self::Jmap(c) => client.with_jmap(c.into_inner()),
            #[cfg(feature = "maildir")]
            Self::Maildir(c) => client.with_maildir(c.into_inner()),
        }
    }
}

/// Resolves the `[accounts.<name>]` block and builds the merged
/// [`Account`]. Returns the resolved name (for error messages and
/// cache keying) alongside the per-account config the caller will
/// destructure into left/right [`SideClient`]s.
pub fn take_account(
    config: Config,
    account_name: Option<&str>,
) -> Result<(String, Account, AccountConfig)> {
    use pimalaya_config::toml::TomlConfig;

    let mut config = config;
    let Some((name, ac)) = config.take_account(account_name)? else {
        bail!("Cannot find account");
    };
    let account = Account::from(config).merge(Account::from(ac.clone()));
    Ok((name, account, ac))
}
