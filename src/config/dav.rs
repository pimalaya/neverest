//! # DAV configuration
//!
//! The `carddav` and `caldav` tables of a source (RFC 6352, RFC 4791).

use pimalaya_config::{secret::Secret, toml::shell_expanded_string};
use serde::{Deserialize, Serialize};

use crate::config::{default_http_alpn, is_default_http_alpn, tls::TlsConfig};

source_config! {
    /// A CardDAV source (RFC 6352), each address book a collection.
    ///
    /// The server URL is the entry point only: the principal and the
    /// address book home set are discovered from it, unless `home` names the
    /// home set itself. One of the two is required.
    #[derive(Clone, Debug, Deserialize, Serialize)]
    #[serde(rename_all = "kebab-case", deny_unknown_fields)]
    pub struct CarddavConfig {
        /// The DAV entry point.
        ///
        /// A bare authority (`dav.example.org[:port]`, read as
        /// `https://<authority>`) or a full URL, `http://` included for a
        /// server on a trusted network.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub server: Option<String>,
        /// The address book home set, as a full URL.
        ///
        /// The session lists its collections there and discovers nothing,
        /// `server` then being ignored.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub home: Option<String>,
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
        /// How the DAV session authenticates.
        pub auth: DavAuthConfig,
    }
}

source_config! {
    /// A CalDAV source (RFC 4791), each calendar a collection.
    ///
    /// The server URL is the entry point only: the principal and the
    /// calendar home set are discovered from it, unless `home` names the
    /// home set itself. One of the two is required.
    #[derive(Clone, Debug, Deserialize, Serialize)]
    #[serde(rename_all = "kebab-case", deny_unknown_fields)]
    pub struct CaldavConfig {
        /// The DAV entry point.
        ///
        /// A bare authority (`dav.example.org[:port]`, read as
        /// `https://<authority>`) or a full URL, `http://` included for a
        /// server on a trusted network.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub server: Option<String>,
        /// The calendar home set, as a full URL.
        ///
        /// The session lists its collections there and discovers nothing,
        /// `server` then being ignored.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub home: Option<String>,
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
        /// How the DAV session authenticates.
        pub auth: DavAuthConfig,
    }
}

/// DAV authentication: none, HTTP Basic, or a bearer token for a provider
/// fronting DAV with OAuth 2.0.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub enum DavAuthConfig {
    /// No credentials, for a server that asks for none: `auth = "none"`.
    None,
    /// A username and a password, HTTP Basic.
    Basic {
        #[serde(deserialize_with = "shell_expanded_string")]
        username: String,
        password: Secret,
    },
    /// A bearer token the client formats itself.
    Bearer { token: Secret },
}
