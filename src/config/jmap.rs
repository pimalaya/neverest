//! # JMAP configuration
//!
//! The `jmap` table of a source (RFC 8620), which parses though no build
//! opens it.

use pimalaya_config::{secret::Secret, toml::shell_expanded_string};
use serde::{Deserialize, Serialize};

use crate::config::{default_http_alpn, is_default_http_alpn, tls::TlsConfig};

source_config! {
    /// A JMAP source (RFC 8620), each mailbox a collection.
    #[derive(Clone, Debug, Deserialize, Serialize)]
    #[serde(rename_all = "kebab-case", deny_unknown_fields)]
    pub struct JmapConfig {
        /// The server, a bare authority or a full URL.
        pub server: String,
        /// How the connection is secured.
        #[serde(default)]
        pub tls: TlsConfig,
        /// ALPN identifiers offered during the TLS handshake, `["http/1.1"]`
        /// by default; `[]` skips ALPN.
        #[serde(
            default = "default_http_alpn",
            skip_serializing_if = "is_default_http_alpn"
        )]
        /// ALPN identifiers offered during the TLS handshake.
        pub alpn: Vec<String>,
        /// How the JMAP session authenticates.
        pub auth: JmapAuthConfig,
        /// The JMAP identity a submission is sent under.
        pub identity_id: Option<String>,
        /// The mailbox a submitted draft is written to.
        pub drafts_mailbox_id: Option<String>,
    }
}

/// How a JMAP session authenticates.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub enum JmapAuthConfig {
    /// A verbatim `Authorization` header value.
    Header(Secret),
    /// A bearer token the client formats itself.
    Bearer { token: Secret },
    /// A username and a password, HTTP Basic.
    Basic {
        #[serde(deserialize_with = "shell_expanded_string")]
        username: String,
        password: Secret,
    },
}
