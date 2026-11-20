//! # TLS configuration
//!
//! The `tls` table every network backend and send channel holds.

use std::path::PathBuf;

use pimalaya_cli::account::BackendCheckTls;
use pimalaya_config::toml::{opt_shell_expanded_path, shell_expanded_path};
use serde::{Deserialize, Serialize};

/// How a connection is secured, shared by every backend.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct TlsConfig {
    /// Which TLS provider to use, the build's own default when unset.
    pub provider: Option<TlsProviderConfig>,
    /// Options the rustls provider reads.
    #[serde(default)]
    pub rustls: RustlsConfig,
    /// Path to an extra CA certificate to trust, shell-expanded.
    #[serde(default, deserialize_with = "opt_shell_expanded_path")]
    pub cert: Option<PathBuf>,
    /// A client certificate, presented when the server asks for one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client: Option<TlsClientConfig>,
}

impl TlsConfig {
    /// Takes from `account` each field this table leaves unset, the client
    /// certificate as one unit.
    pub fn inherit(&mut self, account: &TlsConfig) {
        if self.provider.is_none() {
            self.provider.clone_from(&account.provider);
        }
        if self.rustls.crypto.is_none() {
            self.rustls.crypto.clone_from(&account.rustls.crypto);
        }
        if self.cert.is_none() {
            self.cert.clone_from(&account.cert);
        }
        if self.client.is_none() {
            self.client.clone_from(&account.client);
        }
    }

    /// The table as a check row reports it, `null` where the build's
    /// default answers.
    pub fn checked(&self) -> BackendCheckTls {
        let provider = self.provider.as_ref().map(|provider| match provider {
            TlsProviderConfig::Rustls => "rustls",
            TlsProviderConfig::NativeTls => "native-tls",
        });
        let crypto = self.rustls.crypto.as_ref().map(|crypto| match crypto {
            RustlsCryptoConfig::Aws => "aws",
            RustlsCryptoConfig::Ring => "ring",
        });

        BackendCheckTls {
            provider: provider.map(String::from),
            crypto: crypto.map(String::from),
            cert: self.cert.as_ref().map(|cert| cert.display().to_string()),
            client_cert: self
                .client
                .as_ref()
                .map(|client| client.cert.display().to_string()),
        }
    }
}

/// A client certificate and its key, both PEM, shell-expanded.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct TlsClientConfig {
    /// The certificate, optionally followed by its intermediates.
    #[serde(deserialize_with = "shell_expanded_path")]
    pub cert: PathBuf,
    /// The private key, PKCS#8 under native-tls.
    #[serde(deserialize_with = "shell_expanded_path")]
    pub key: PathBuf,
}

/// Which TLS implementation carries the connection.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub enum TlsProviderConfig {
    /// The rustls provider.
    Rustls,
    /// The platform's own TLS stack.
    NativeTls,
}

/// Options the rustls provider reads.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct RustlsConfig {
    /// Which crypto backend rustls uses, its default when unset.
    pub crypto: Option<RustlsCryptoConfig>,
}

/// Which crypto backend rustls uses.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub enum RustlsCryptoConfig {
    /// The aws-lc-rs backend.
    Aws,
    /// The ring backend.
    Ring,
}

/// Whether a connection to `server`, `default` the scheme of a bare
/// authority, runs over TLS: its scheme is no cleartext one, or it upgrades.
pub fn secured(server: &str, default: &str, starttls: bool) -> bool {
    let scheme = match server.split_once("://") {
        Some((scheme, _)) => scheme.to_ascii_lowercase(),
        None => default.to_owned(),
    };

    starttls || !matches!(scheme.as_str(), "imap" | "smtp" | "http")
}
