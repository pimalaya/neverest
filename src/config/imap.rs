//! # IMAP configuration
//!
//! The `imap` table of a source (RFC 9051).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::config::{is_default, sasl::SaslConfig, tls::TlsConfig};

source_config! {
    /// An IMAP source (RFC 9051), each mailbox a collection.
    #[derive(Clone, Debug, Deserialize, Serialize)]
    #[serde(rename_all = "kebab-case", deny_unknown_fields)]
    pub struct ImapConfig {
        /// The server, a bare authority or a full URL.
        pub server: String,
        /// How the connection is secured.
        #[serde(default)]
        pub tls: TlsConfig,
        /// Whether a plain connection is upgraded with STARTTLS.
        #[serde(default, skip_serializing_if = "is_default")]
        pub starttls: bool,
        /// ALPN identifiers offered during the TLS handshake.
        ///
        /// Unset takes io-imap's own default (`["imap"]`), which owns it;
        /// `[]` skips ALPN.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub alpn: Option<Vec<String>>,
        /// The one SASL mechanism the session authenticates with.
        pub sasl: Option<SaslConfig>,
        /// Forces the RFC 4959 SASL-IR initial response on or off.
        ///
        /// Unset follows the advertised `SASL-IR` capability, which Coremail
        /// (126.com, 163.com) advertises falsely: set it to `false` there.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub sasl_ir: Option<bool>,
        /// RFC 2971 `ID` quirks, `id.auto = true` opting in to the exchange
        /// mail.qq.com and Fastmail require straight after authentication.
        #[serde(default, skip_serializing_if = "is_default")]
        pub id: ImapIdConfig,
    }
}

/// The `imap.id.*` quirks.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct ImapIdConfig {
    /// Chains an `ID` round-trip after the tagged authentication response.
    #[serde(default, skip_serializing_if = "is_default")]
    pub auto: bool,
    /// The parameters the automatic `ID` sends, none sending `ID NIL`.
    ///
    /// `true` sends neverest's canned value for the well-known keys
    /// (`name`, `version`, `vendor`, `support-url`) and `NIL` for the
    /// others, `false` always `NIL`. An absent key is not sent.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub fields: BTreeMap<String, bool>,
}
