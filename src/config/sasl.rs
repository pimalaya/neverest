//! # SASL configuration
//!
//! The `sasl` table of an IMAP source or an SMTP channel, naming one
//! mechanism.

use pimalaya_config::{secret::Secret, toml::shell_expanded_string};
use serde::{Deserialize, Serialize};

/// The one SASL mechanism a session authenticates with.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub enum SaslConfig {
    /// Authenticates nobody, for a relay taking anonymous mail.
    Anonymous(SaslAnonymousConfig),
    /// The legacy LOGIN exchange.
    Login(SaslLoginConfig),
    /// A username and a password in one message.
    Plain(SaslPlainConfig),
    /// A bearer token in the GS2 header.
    Oauthbearer(SaslOauthbearerConfig),
    /// A bearer token in the vendors' own spelling.
    Xoauth2(SaslXoauth2Config),
    /// A salted challenge, so the password never crosses.
    #[serde(rename = "scram-sha-256")]
    ScramSha256(SaslScramSha256Config),
}

/// The ANONYMOUS mechanism (RFC 4505), which authenticates nobody.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct SaslAnonymousConfig {
    /// The trace message the mechanism carries, if any.
    pub message: Option<String>,
}

/// The legacy LOGIN mechanism, a username and a password.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct SaslLoginConfig {
    /// The identity the mechanism authenticates as.
    #[serde(deserialize_with = "shell_expanded_string")]
    pub username: String,
    /// The password, resolved once per run.
    pub password: Secret,
}

/// The PLAIN mechanism (RFC 4616).
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct SaslPlainConfig {
    /// The authorization identity, where it differs from the
    /// authentication one.
    pub authzid: Option<String>,
    /// The authentication identity.
    #[serde(deserialize_with = "shell_expanded_string")]
    #[serde(alias = "username")]
    pub authcid: String,
    /// The password, resolved once per run.
    #[serde(alias = "password")]
    pub passwd: Secret,
}

/// The OAUTHBEARER mechanism (RFC 7628).
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct SaslOauthbearerConfig {
    /// The identity the mechanism authenticates as.
    #[serde(deserialize_with = "shell_expanded_string")]
    pub username: String,
    /// The access token, resolved once per run.
    pub token: Secret,
}

/// The XOAUTH2 mechanism, Google and Microsoft's predecessor to OAUTHBEARER.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct SaslXoauth2Config {
    /// The identity the mechanism authenticates as.
    #[serde(deserialize_with = "shell_expanded_string")]
    pub username: String,
    /// The access token, resolved once per run.
    pub token: Secret,
}

/// The SCRAM-SHA-256 mechanism (RFC 7677), which sends no password.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct SaslScramSha256Config {
    /// The identity the mechanism authenticates as.
    #[serde(deserialize_with = "shell_expanded_string")]
    pub username: String,
    /// The password, resolved once per run.
    pub password: Secret,
}
