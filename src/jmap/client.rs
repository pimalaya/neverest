//! JMAP client wrapper. Resolves the `/.well-known/jmap` session
//! eagerly so the first sync op already has a populated session
//! (account id, API URL, download URL).

use anyhow::Result;
use base64::{Engine, prelude::BASE64_STANDARD};
use io_jmap::client::JmapClientStd as Inner;
use pimalaya_stream::tls::Tls;
use secrecy::{ExposeSecret, SecretString};
use url::Url;

use crate::{
    account::context::Account,
    config::{JmapAuthConfig, JmapConfig},
};

pub struct JmapClient {
    inner: Inner,
    #[allow(dead_code)]
    pub account: Account,
}

impl JmapClient {
    pub fn new(config: JmapConfig, account: Account) -> Result<Self> {
        let mut tls: Tls = config.tls.clone().into();
        tls.rustls.alpn = vec!["http/1.1".into()];

        let http_auth = jmap_http_auth(config.auth.clone())?;
        let url = parse_server_url(&config.server)?;

        let mut inner = Inner::connect(&url, &tls, http_auth)?;
        inner.session_get(&url)?;

        Ok(Self { inner, account })
    }

    pub fn into_inner(self) -> Inner {
        self.inner
    }
}

/// Parses the JMAP `server` field into a [`Url`], defaulting bare
/// authorities to `https://`.
pub fn parse_server_url(server: &str) -> Result<Url> {
    match Url::parse(server) {
        Ok(url) => Ok(url),
        Err(url::ParseError::RelativeUrlWithoutBase) => {
            Ok(Url::parse(&format!("https://{server}"))?)
        }
        Err(err) => Err(err.into()),
    }
}

/// Converts a [`JmapAuthConfig`] into the pre-formatted HTTP
/// `Authorization` header value `JmapClientStd::connect` expects.
pub fn jmap_http_auth(config: JmapAuthConfig) -> Result<SecretString> {
    match config {
        JmapAuthConfig::Header(token) => Ok(token.get()?),
        JmapAuthConfig::Bearer { token } => {
            let token = token.get()?;
            Ok(format!("Bearer {}", token.expose_secret()).into())
        }
        JmapAuthConfig::Basic { username, password } => {
            let creds = format!("{}:{}", username, password.get()?.expose_secret());
            let encoded = BASE64_STANDARD.encode(creds.into_bytes());
            Ok(format!("Basic {encoded}").into())
        }
    }
}
