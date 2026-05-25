// This file is part of Neverest, a CLI to synchronize emails.
//
// Copyright (C) 2024-2026  soywod <pimalaya.org@posteo.net>
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affero General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

//! Side-agnostic protocol client construction for the sync engine.

use anyhow::{Result, bail};
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
#[cfg(any(feature = "imap", feature = "jmap"))]
use url::{ParseError, Url};

#[cfg(feature = "jmap")]
use crate::config::JmapAuthConfig;
use crate::config::SideConfig;

/// Opens the protocol client for `config` and registers it onto a fresh
/// [`EmailClientStd`].
///
/// For m2dir sides the store root must already exist; use [`init`] to
/// create it.
pub fn open(config: SideConfig) -> Result<EmailClientStd> {
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

            let mut client = ImapClientStd::connect(&server, &tls, config.starttls, sasl)?;
            // NOTE: sync engine pre-selects once per mailbox batch, so
            // every subsequent STORE / FETCH / COPY must skip its own
            // SELECT.
            client.auto_select = false;

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
        _ => bail!("Side requires a backend feature that is not compiled in"),
    }
}

/// Same as [`open`] plus any side-local bootstrap (e.g. m2dir store
/// root + marker creation).
pub fn init(config: SideConfig) -> Result<EmailClientStd> {
    #[cfg(feature = "m2dir")]
    if let SideConfig::M2dir(config) = &config {
        let client = M2dirClient::new(config.root.to_string_lossy().into_owned());
        client.init_store()?;
    }

    open(config)
}
