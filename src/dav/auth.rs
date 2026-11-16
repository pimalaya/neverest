//! # DAV authentication
//!
//! Turns the `auth` table of a DAV source into io-webdav's auth, its secret
//! resolved.

use anyhow::Result;
use io_http::{rfc6750::bearer::HttpAuthBearer, rfc7617::basic::HttpAuthBasic};
use io_webdav::rfc4918::WebdavAuth;
use pimalaya_config::secret::SecretResolver;
use secrecy::ExposeSecret;

use crate::config::dav::DavAuthConfig;

impl DavAuthConfig {
    /// Resolves the configured secret and converts to io-webdav's auth.
    ///
    /// The resolver keeps an account's CardDAV and CalDAV sides, which
    /// usually name one password entry, from unlocking it twice. It runs
    /// where the account is built ([`crate::account`]), never per connection.
    pub fn try_into_dav_auth(self, resolver: &mut SecretResolver) -> Result<WebdavAuth> {
        Ok(match self {
            Self::None => WebdavAuth::None,
            Self::Basic { username, password } => WebdavAuth::Basic(HttpAuthBasic::new(
                username,
                resolver.resolve(password)?.expose_secret(),
            )),
            Self::Bearer { token } => WebdavAuth::Bearer(HttpAuthBearer::new(
                resolver.resolve(token)?.expose_secret(),
            )),
        })
    }
}
