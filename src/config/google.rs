//! # Google configuration
//!
//! The `gpeople` and `gcal` tables of a source, over the People and
//! Calendar APIs.

use pimalaya_config::secret::Secret;
use serde::{Deserialize, Serialize};

use crate::config::{default_http_alpn, is_default_http_alpn, tls::TlsConfig};

source_config! {
    /// A Google source, People contacts or Calendar events: the API host is
    /// fixed, so only TLS and the OAuth 2.0 credential are configurable.
    #[derive(Clone, Debug, Deserialize, Serialize)]
    #[serde(rename_all = "kebab-case", deny_unknown_fields)]
    pub struct GoogleConfig {
        /// How the connection is secured.
        #[serde(default)]
        pub tls: TlsConfig,
        /// ALPN identifiers offered during the TLS handshake, `["http/1.1"]`
        /// by default; `[]` skips ALPN.
        #[serde(
            default = "default_http_alpn",
            skip_serializing_if = "is_default_http_alpn"
        )]
        pub alpn: Vec<String>,
        /// How the session authenticates.
        pub auth: GoogleAuthConfig,
    }
}

/// Google authentication: OAuth 2.0 bearer tokens only, neverest never
/// running an OAuth flow itself.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct GoogleAuthConfig {
    /// OAuth 2.0 bearer token, the `Bearer ` prefix added by the client.
    ///
    /// It needs the `contacts` scope for People and the `calendar` scope
    /// for Calendar; point `token.command` at a broker such as ortie.
    pub token: Secret,
}
