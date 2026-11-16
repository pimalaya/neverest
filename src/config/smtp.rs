//! # SMTP configuration
//!
//! The `smtp` table: the server a source's queued sends flush through.

use serde::{Deserialize, Serialize};

use crate::config::{is_default, proxy::ProxyConfig, sasl::SaslConfig, tls::TlsConfig};

/// The SMTP server a source's queued sends flush through.
///
/// It mirrors [`ImapConfig`](crate::config::imap::ImapConfig) field for
/// field, submission being the other half of the same mail account.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct SmtpConfig {
    /// The submission server.
    ///
    /// A bare authority (`smtp.example.org[:port]`, read as
    /// `smtps://<authority>`), a cleartext `smtp://` URL, usually with
    /// `starttls`, or an `smtps://` URL for implicit TLS.
    pub server: String,
    /// How the connection is secured.
    #[serde(default)]
    pub tls: TlsConfig,
    /// Upgrades a plain `smtp://` connection via STARTTLS.
    #[serde(default, skip_serializing_if = "is_default")]
    pub starttls: bool,
    /// ALPN identifiers offered during the TLS handshake.
    ///
    /// Unset takes io-smtp's own default (`["smtp"]`, the token RFC 7595
    /// registers), which owns it; `[]` skips ALPN.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alpn: Option<Vec<String>>,
    /// The mechanism the session authenticates with, one [`SaslConfig`].
    ///
    /// Omit it for an unauthenticated relay, which stops after `EHLO` and
    /// sends no `AUTH` at all.
    pub sasl: Option<SaslConfig>,
    /// The proxy the channel goes through, falling back to the account's,
    /// then to the `all_proxy` and `https_proxy` environment variables.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proxy: Option<ProxyConfig>,
}

#[cfg(test)]
mod tests {
    use crate::config::{AccountConfig, sasl::SaslConfig};

    /// The send channel spells its credentials as the sync side does, and
    /// omits the table for a relay that takes no `AUTH`.
    #[test]
    fn the_send_channel_names_a_sasl_mechanism() {
        let account: AccountConfig = toml::from_str(
            r#"
            imap.server = "imaps://imap.example.org:993"
            smtp.server = "smtp.example.org"
            smtp.sasl.xoauth2.username = "user@example.org"
            smtp.sasl.xoauth2.token.command = ["ortie", "token", "read", "example"]
            "#,
        )
        .unwrap();

        let smtp = account.smtp.as_ref().expect("a declared channel");
        assert_eq!(smtp.server, "smtp.example.org");
        assert!(matches!(smtp.sasl, Some(SaslConfig::Xoauth2(_))));

        let relay: AccountConfig = toml::from_str(
            r#"
            imap.server = "imaps://imap.example.org:993"
            smtp.server = "smtp://127.0.0.1:2525"
            "#,
        )
        .unwrap();
        assert!(relay.smtp.expect("a declared channel").sasl.is_none());
    }
}
